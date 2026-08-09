use crate::models::AppSettings;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub used_mock: bool,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

fn cancelled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(Ordering::Relaxed))
        .unwrap_or(false)
}

async fn wait_cancel(cancel: &Option<Arc<AtomicBool>>) {
    let Some(flag) = cancel else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        if flag.load(Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

/// Thin DeepSeek client. Uses OpenAI-compatible chat API.
pub async fn complete(
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<CompletionResult> {
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(settings.default_model.as_str())
        .to_string();

    if cancelled(&cancel) {
        return Err(anyhow!("cancelled"));
    }

    if settings.deepseek_api_key.trim().is_empty() {
        let content = mock_complete(system, user);
        let prompt_tokens = ((system.len() + user.len()) / 4) as u32;
        let completion_tokens = (content.len() / 4) as u32;
        return Ok(CompletionResult {
            content,
            used_mock: true,
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        });
    }

    // ponytail: 仅预热，失败不影响主 HTTP 路径；切勿 expect（会拖垮整个进程）
    rig_bridge::warm_client(settings);
    let _ = adk_bridge::describe();

    let base = settings.deepseek_base_url.trim_end_matches('/');
    let url = format!("{base}/v1/chat/completions");
    let body = ChatRequest {
        model: model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: system.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: user.into(),
            },
        ],
        temperature: 0.8,
    };

    let client = reqwest::Client::new();
    let send = client
        .post(&url)
        .bearer_auth(&settings.deepseek_api_key)
        .json(&body)
        .send();

    let resp = tokio::select! {
        r = send => r?,
        _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
    };

    if cancelled(&cancel) {
        return Err(anyhow!("cancelled"));
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let text = tokio::select! {
            r = resp.text() => r.unwrap_or_default(),
            _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
        };
        return Err(anyhow!("DeepSeek API {status}: {text}"));
    }

    let parsed: ChatResponse = tokio::select! {
        r = resp.json() => r?,
        _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
    };
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let (prompt_tokens, completion_tokens, total_tokens) = if let Some(u) = parsed.usage {
        let total = if u.total_tokens > 0 {
            u.total_tokens
        } else {
            u.prompt_tokens.saturating_add(u.completion_tokens)
        };
        (u.prompt_tokens, u.completion_tokens, total)
    } else {
        let p = ((system.len() + user.len()) / 4) as u32;
        let c = (content.len() / 4) as u32;
        (p, c, p + c)
    };

    Ok(CompletionResult {
        content,
        used_mock: false,
        model,
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct ToolRoundResult {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub used_mock: bool,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// One OpenAI-compatible chat turn that may return tool_calls.
pub async fn complete_tool_round(
    settings: &AppSettings,
    messages: &[Value],
    tools: &[Value],
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<ToolRoundResult> {
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(settings.default_model.as_str())
        .to_string();

    if cancelled(&cancel) {
        return Err(anyhow!("cancelled"));
    }

    if settings.deepseek_api_key.trim().is_empty() {
        let user = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .unwrap_or("");
        let system = messages
            .iter()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .unwrap_or("");
        let content = mock_complete(system, user);
        let prompt_tokens = ((system.len() + user.len()) / 4) as u32;
        let completion_tokens = (content.len() / 4) as u32;
        return Ok(ToolRoundResult {
            content,
            tool_calls: vec![],
            used_mock: true,
            model,
            prompt_tokens,
            completion_tokens,
        });
    }

    let base = settings.deepseek_base_url.trim_end_matches('/');
    let url = format!("{base}/v1/chat/completions");
    let body = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "temperature": 0.7,
    });

    let client = reqwest::Client::new();
    let send = client
        .post(&url)
        .bearer_auth(&settings.deepseek_api_key)
        .json(&body)
        .send();

    let resp = tokio::select! {
        r = send => r?,
        _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
    };

    if cancelled(&cancel) {
        return Err(anyhow!("cancelled"));
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let text = tokio::select! {
            r = resp.text() => r.unwrap_or_default(),
            _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
        };
        return Err(anyhow!("DeepSeek API {status}: {text}"));
    }

    let parsed: Value = tokio::select! {
        r = resp.json() => r?,
        _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
    };

    let choice = parsed
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or(json!({}));
    let content = choice
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(arr) = choice.get("tool_calls").and_then(|v| v.as_array()) {
        for (i, tc) in arr.iter().enumerate() {
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("call_{i}"));
            let name = tc
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let arguments = tc
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}")
                .to_string();
            if !name.is_empty() {
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }
    }

    let (prompt_tokens, completion_tokens) = if let Some(u) = parsed.get("usage") {
        (
            u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            u.get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        )
    } else {
        let approx: usize = messages.iter().map(|m| m.to_string().len()).sum();
        (((approx) / 4) as u32, (content.len() / 4) as u32)
    };

    Ok(ToolRoundResult {
        content,
        tool_calls,
        used_mock: false,
        model,
        prompt_tokens,
        completion_tokens,
    })
}

fn mock_complete(system: &str, user: &str) -> String {
    format!(
        "【Mock 生成 · 未配置 DeepSeek API Key】\n\n\
         系统约束摘要：{}\n\n\
         用户请求：{}\n\n\
         ——\n\
         夜色刚落，主角推开木门。风里带着尘土与旧书的味道。\n\
         他（她）想起树图上的大纲，决定先按节点约束把这一幕写完。\n\
         「座右铭还在耳边回响。」配角低声说，像是提醒，也像是警告。\n\n\
         （请在设置页填写 DeepSeek API Key 后重新生成。）",
        truncate(system, 240),
        truncate(user, 400)
    )
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

mod rig_bridge {
    use crate::models::AppSettings;
    use rig_core::providers::openai;

    /// Wire rig-core OpenAI-compatible client to DeepSeek base URL.
    pub fn warm_client(settings: &AppSettings) {
        let _ = openai::Client::builder()
            .api_key(&settings.deepseek_api_key)
            .base_url(&settings.deepseek_base_url)
            .build()
            .or_else(|_| openai::Client::new(&settings.deepseek_api_key));
    }
}

mod adk_bridge {
    /// Skills injection lives in `crate::skills` (adk-rust `skills` feature).
    pub fn describe() -> &'static str {
        "adk-rust skills enabled for chat"
    }
}
