use crate::models::AppSettings;
use anyhow::{anyhow, Result};
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

struct CompatEndpoint<'a> {
    api_key: &'a str,
    base_url: &'a str,
    label: &'a str,
}

fn compat_endpoint<'a>(settings: &'a AppSettings, model: &str) -> CompatEndpoint<'a> {
    if let Some(p) = settings.resolve_compat(model) {
        CompatEndpoint {
            api_key: p.api_key.as_str(),
            base_url: p.base_url.as_str(),
            label: if p.label.trim().is_empty() {
                "OpenAI"
            } else {
                p.label.as_str()
            },
        }
    } else {
        CompatEndpoint {
            api_key: "",
            base_url: "",
            label: "OpenAI",
        }
    }
}

fn chat_completions_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/chat/completions")
    } else {
        format!("{b}/v1/chat/completions")
    }
}

/// Kimi / Moonshot 部分模型（如 K2）只允许 temperature=1。
fn chat_temperature_with_hint(model: &str, label: &str, base_url: &str, preferred: f32) -> f32 {
    let blob = format!("{model} {label} {base_url}").to_ascii_lowercase();
    if blob.contains("kimi") || blob.contains("moonshot") {
        1.0
    } else {
        preferred
    }
}

/// OpenAI-compatible chat via configured compat_providers.
/// `json_object`: 请求 `response_format=json_object`（Kimi 等模型更稳出可解析 JSON）。
pub async fn complete(
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
    json_object: bool,
) -> Result<CompletionResult> {
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(settings.default_model.as_str())
        .to_string();

    if cancelled(&cancel) {
        return Err(anyhow!("cancelled"));
    }

    let ep = compat_endpoint(settings, &model);
    if ep.api_key.trim().is_empty() {
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

    let url = chat_completions_url(ep.base_url);
    let mut body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": if json_object {
            0.2
        } else {
            chat_temperature_with_hint(&model, ep.label, ep.base_url, 0.8)
        },
    });
    if json_object {
        body["response_format"] = json!({"type": "json_object"});
    }

    let client = reqwest::Client::new();
    let send = client
        .post(&url)
        .bearer_auth(ep.api_key)
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
        return Err(anyhow!("{} API {status}: {text}", ep.label));
    }

    let parsed: Value = tokio::select! {
        r = resp.json() => r?,
        _ = wait_cancel(&cancel) => return Err(anyhow!("cancelled")),
    };
    let content = extract_assistant_text(&parsed);

    let (prompt_tokens, completion_tokens, total_tokens) = if let Some(u) = parsed.get("usage") {
        let p = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let c = u
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let total = u
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .map(|t| t as u32)
            .unwrap_or_else(|| p.saturating_add(c));
        (p, c, total)
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

/// 从 chat.completions 响应取出助手正文（兼容 string / multipart content）。
fn extract_assistant_text(parsed: &Value) -> String {
    let msg = parsed.pointer("/choices/0/message").unwrap_or(&Value::Null);
    let from_content = match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if !from_content.trim().is_empty() {
        return from_content;
    }
    // 少数兼容网关把终稿放在其它字段；仍不使用 reasoning_content（那是思维链）
    msg.get("output_text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
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

mod rig_bridge {
    use crate::models::AppSettings;
    use rig_core::providers::openai;

    /// Wire rig-core OpenAI-compatible client to first configured provider.
    pub fn warm_client(settings: &AppSettings) {
        let Some(p) = settings
            .compat_providers
            .iter()
            .find(|p| !p.api_key.trim().is_empty())
        else {
            return;
        };
        let _ = openai::Client::builder()
            .api_key(&p.api_key)
            .base_url(&p.base_url)
            .build()
            .or_else(|_| openai::Client::new(&p.api_key));
    }
}

mod adk_bridge {
    /// Skills injection lives in `crate::skills` (adk-rust `skills` feature).
    pub fn describe() -> &'static str {
        "adk-rust skills enabled for chat"
    }
}

#[cfg(test)]
mod tests {
    use super::chat_temperature_with_hint;

    #[test]
    fn kimi_forces_temperature_one() {
        assert_eq!(chat_temperature_with_hint("kimi-k2.5", "", "", 0.8), 1.0);
        assert_eq!(
            chat_temperature_with_hint("custom", "Kimi", "https://api.moonshot.ai", 0.7),
            1.0
        );
        assert_eq!(
            chat_temperature_with_hint("deepseek-chat", "DeepSeek", "https://api.deepseek.com", 0.8),
            0.8
        );
    }
}
