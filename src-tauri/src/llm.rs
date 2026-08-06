use crate::models::AppSettings;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub used_mock: bool,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Thin DeepSeek client. Uses OpenAI-compatible chat API.
pub async fn complete(
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
) -> Result<CompletionResult> {
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(settings.default_model.as_str())
        .to_string();

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

    let _ = rig_bridge::warm_client(settings);
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
    let resp = client
        .post(&url)
        .bearer_auth(&settings.deepseek_api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("DeepSeek API {status}: {text}"));
    }

    let parsed: ChatResponse = resp.json().await?;
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
    pub fn warm_client(settings: &AppSettings) -> openai::Client {
        openai::Client::builder()
            .api_key(&settings.deepseek_api_key)
            .base_url(&settings.deepseek_base_url)
            .build()
            .or_else(|_| openai::Client::new(&settings.deepseek_api_key))
            .expect("rig OpenAI-compatible client")
    }
}

mod adk_bridge {
    /// ponytail: ADK agent graph deferred; crate linked so chapter pipeline can migrate later.
    pub fn describe() -> &'static str {
        "adk-rust linked for future chapter/agent workflows"
    }
}
