//! rig Extractor：把模型输出钉成 JSON Schema，失败自动再问一轮。
use crate::compat_messages::EnsureMessageContent;
use crate::models::AppSettings;
use anyhow::{anyhow, Result};
use rig_agent::agent::ModelHandle;
use rig_agent::extractor::ExtractorBuilder;
use rig_core::client::CompletionClient;
use rig_core::providers::deepseek;
use rig_core::providers::openai::CompletionsClient;
use rig_core::providers::zai;
use rig_core::providers::{
    anthropic, azure, cohere, doubleword, gemini, groq, huggingface, hyperbolic, llamafile, minimax,
    mira, mistral, moonshot, ollama, openai, openrouter, perplexity, together, venice, xai,
    xiaomimimo,
};
use rig_core::wasm_compat::{WasmCompatSend, WasmCompatSync};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChapterMemoryExtract {
    /// 整体情节 2–6 句
    #[serde(default)]
    pub plot: String,
    /// 要点（人物/行为/结果）
    #[serde(default)]
    pub facts: Vec<String>,
    /// 本章已向读者交代、后文勿再解说的观点/设定
    #[serde(default)]
    pub revealed: Vec<String>,
}

fn fact_chunk(t: &str) -> Option<String> {
    let t = t.trim();
    if t.chars().count() < 4 {
        return None;
    }
    Some(if t.starts_with('-') {
        t.to_string()
    } else {
        format!("- {t}")
    })
}

fn revealed_chunk(t: &str) -> Option<String> {
    let t = t.trim();
    if t.chars().count() < 4 {
        return None;
    }
    let body = t.trim_start_matches(['-', '*', '•', ' ']).trim();
    let lower = body.to_ascii_lowercase();
    if body.contains("无新交代")
        || body.contains("無新交代")
        || lower.contains("no new reveal")
        || lower.contains("no core memory")
    {
        return None;
    }
    if body.starts_with("已交代")
        || body.starts_with("既出")
        || lower.starts_with("already shown")
        || lower.starts_with("already-told")
        || lower.starts_with("revealed:")
        || lower.starts_with("bereits gezeigt")
        || lower.starts_with("déjà dit")
        || lower.starts_with("deja dit")
    {
        return Some(if t.starts_with('-') {
            t.to_string()
        } else {
            format!("- {body}")
        });
    }
    Some(format!("- 已交代：{body}"))
}

impl ChapterMemoryExtract {
    pub fn into_chunks(self) -> Vec<String> {
        let mut out = Vec::new();
        let plot = self.plot.trim();
        if plot.chars().count() >= 4 {
            out.push(format!("【整体情节】\n{plot}"));
        }
        for f in self.facts {
            if let Some(c) = fact_chunk(&f) {
                out.push(c);
            }
        }
        for r in self.revealed {
            if let Some(c) = revealed_chunk(&r) {
                out.push(c);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutlineItemExtract {
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub outline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutlinesExtract {
    #[serde(default)]
    pub outlines: Vec<OutlineItemExtract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DetailedOutlineExtract {
    #[serde(default)]
    pub detailed_outline: Vec<String>,
}

fn openai_compat_base(base: &str) -> String {
    base.trim().trim_end_matches('/').to_string()
}

fn handle_from<C: CompletionClient + 'static>(client: C, api_model: &str) -> ModelHandle
where
    C::CompletionModel: 'static,
{
    ModelHandle::new(EnsureMessageContent(client.completion_model(api_model)))
}

fn ok_handle<C, E>(c: Result<C, E>, api_model: &str) -> Result<ModelHandle>
where
    C: CompletionClient + 'static,
    C::CompletionModel: 'static,
    E: std::fmt::Display,
{
    c.map_err(|e| anyhow!("{e}")).map(|c| handle_from(c, api_model))
}

fn model_handle(settings: &AppSettings, model: &str) -> Result<ModelHandle> {
    let ep = settings
        .resolve_compat(model)
        .ok_or_else(|| anyhow!("未配置可用的 AI API"))?;
    if !ep.is_ready() {
        return Err(anyhow!("未配置可用的 AI API"));
    }
    let api_model = crate::models::api_model_id(model).to_string();
    let key = ep.api_key.as_str();
    let base = openai_compat_base(&ep.base_url);
    let proto = ep.chat_protocol();
    let m = api_model.as_str();
    match proto {
        crate::models::PROTOCOL_DEEPSEEK => ok_handle(deepseek::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_ZAI => ok_handle(zai::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_ANTHROPIC => {
            ok_handle(anthropic::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_GEMINI => ok_handle(gemini::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_GROQ => ok_handle(groq::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_MOONSHOT => {
            ok_handle(moonshot::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_MISTRAL => {
            ok_handle(mistral::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_OPENROUTER => {
            ok_handle(openrouter::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_TOGETHER => {
            ok_handle(together::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_XAI => ok_handle(xai::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_OLLAMA => ok_handle(ollama::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_HYPERBOLIC => {
            ok_handle(hyperbolic::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_HUGGINGFACE => {
            ok_handle(huggingface::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_MINIMAX => {
            ok_handle(minimax::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_MIRA => ok_handle(mira::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_PERPLEXITY => {
            ok_handle(perplexity::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_VENICE => ok_handle(venice::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_COHERE => ok_handle(cohere::Client::builder().api_key(key).base_url(&base).build(), m),
        crate::models::PROTOCOL_XIAOMIMIMO => {
            ok_handle(xiaomimimo::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_DOUBLEWORD => {
            ok_handle(doubleword::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_OPENAI_RESPONSES => {
            ok_handle(openai::Client::builder().api_key(key).base_url(&base).build(), m)
        }
        crate::models::PROTOCOL_AZURE => ok_handle(azure::Client::builder()
            .api_key(azure::AzureOpenAIAuth::ApiKey(ep.api_key.clone()))
            .azure_endpoint(base.clone())
            .build(), m),
        crate::models::PROTOCOL_LLAMAFILE => ok_handle(llamafile::Client::from_url(&base), m),
        _ => ok_handle(CompletionsClient::builder().api_key(key).base_url(&base).build(), m),
    }
}

/// `submit` 工具抽取；失败则 `Err`（调用方回退旧解析）。
pub async fn extract<T>(
    settings: &AppSettings,
    model: &str,
    preamble: &str,
    text: &str,
) -> Result<T>
where
    T: JsonSchema
        + for<'de> Deserialize<'de>
        + Serialize
        + WasmCompatSend
        + WasmCompatSync
        + 'static,
{
    let handle = model_handle(settings, model)?;
    let extractor = ExtractorBuilder::<T>::from_model_handle(handle)
        .preamble(preamble)
        .retries(1)
        .build();
    extractor.extract(text.to_string()).await.map_err(|e| anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::ChapterMemoryExtract;

    #[test]
    fn memory_extract_to_chunks() {
        let c = ChapterMemoryExtract {
            plot: "甲在雾港发现旧地图。".into(),
            facts: vec!["人物：甲｜行为：发现地图".into(), "x".into()],
            revealed: vec!["旧地图能指向沉船".into()],
        }
        .into_chunks();
        assert!(c[0].contains("整体情节") && c[0].contains("旧地图"));
        assert!(c.iter().any(|x| x.contains("甲") && x.starts_with('-')));
        assert!(c.iter().any(|x| x.contains("已交代") && x.contains("沉船")));
        assert_eq!(c.len(), 3);
    }
}
