//! Fetch live model IDs from providers that have API keys configured.
use crate::db::Db;
use crate::models::{self, ModelCatalog};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Deserialize;

pub fn models_list_url(base: &str) -> String {
    format!("{}/models", base.trim().trim_end_matches('/'))
}

fn strip_slash(base: &str) -> &str {
    base.trim().trim_end_matches('/')
}

pub async fn fetch_openai_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    fetch_openai_compat(&models_list_url(base_url), api_key).await
}

pub async fn fetch_provider_models(
    protocol: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>> {
    match protocol {
        models::PROTOCOL_ANTHROPIC => fetch_claude(api_key, base_url).await,
        models::PROTOCOL_GEMINI => fetch_gemini(api_key, base_url).await,
        models::PROTOCOL_OLLAMA => fetch_ollama(base_url).await,
        models::PROTOCOL_AZURE => fetch_azure(base_url, api_key).await,
        models::PROTOCOL_LLAMAFILE => {
            let root = strip_slash(base_url);
            let url = if root.ends_with("/v1") {
                format!("{root}/models")
            } else {
                format!("{root}/v1/models")
            };
            fetch_openai_compat(&url, api_key).await
        }
        models::PROTOCOL_MISTRAL | models::PROTOCOL_XAI | models::PROTOCOL_HYPERBOLIC
        | models::PROTOCOL_HUGGINGFACE | models::PROTOCOL_MIRA | models::PROTOCOL_PERPLEXITY => {
            let root = strip_slash(base_url);
            let url = if root.contains("/v1") {
                models_list_url(root)
            } else {
                format!("{root}/v1/models")
            };
            fetch_openai_compat(&url, api_key).await
        }
        _ => fetch_openai_models(base_url, api_key).await,
    }
}

pub async fn refresh(db: &Db) -> Result<ModelCatalog> {
    let mut settings = db.get_settings()?;
    let mut catalog = db.get_model_catalog().unwrap_or_default();
    catalog.errors.clear();

    for p in settings.compat_providers.iter_mut() {
        if !p.is_ready() {
            continue;
        }
        match fetch_provider_models(p.chat_protocol(), &p.base_url, &p.api_key).await {
            Ok(ids) => {
                p.models = merge_intersect_models(&p.models, ids);
            }
            Err(e) => {
                catalog
                    .errors
                    .insert(format!("compat:{}", p.label), e.to_string());
            }
        }
    }

    catalog.compat = settings.all_compat_model_ids();
    catalog.deepseek.clear();
    catalog.grok.clear();
    catalog.kimi.clear();
    catalog.gemini.clear();
    catalog.claude.clear();

    catalog.updated_at = Utc::now().to_rfc3339();
    db.save_settings(&settings)?;
    db.save_model_catalog(&catalog)?;
    Ok(catalog)
}

/// Names in `previous` that the `/models` API did not return (manual extras).
fn unlisted_models(previous: &[String], fetched: &[String]) -> Vec<String> {
    let set: std::collections::HashSet<&str> = fetched.iter().map(String::as_str).collect();
    previous
        .iter()
        .filter(|m| {
            let t = m.trim();
            !t.is_empty() && !set.contains(t)
        })
        .cloned()
        .collect()
}

fn append_unlisted(mut out: Vec<String>, extras: Vec<String>) -> Vec<String> {
    for e in extras {
        if !out.iter().any(|x| x == &e) {
            out.push(e);
        }
    }
    out
}

/// Full API list plus names the catalog omitted.
fn merge_replace_models(previous: &[String], fetched: Vec<String>) -> Vec<String> {
    let extras = unlisted_models(previous, &fetched);
    append_unlisted(fetched, extras)
}

/// Keep the user's subset that still exists, plus names the catalog omitted.
fn merge_intersect_models(previous: &[String], fetched: Vec<String>) -> Vec<String> {
    if previous.is_empty() {
        return fetched;
    }
    let extras = unlisted_models(previous, &fetched);
    let set: std::collections::HashSet<&str> = fetched.iter().map(String::as_str).collect();
    let mut kept: Vec<String> = previous
        .iter()
        .filter(|m| set.contains(m.as_str()))
        .cloned()
        .collect();
    if kept.is_empty() {
        kept = fetched;
    }
    append_unlisted(kept, extras)
}

/// Refresh models for one saved provider; API result plus previously listed extras.
pub async fn refresh_provider_models(db: &Db, provider_id: &str) -> Result<Vec<String>> {
    let mut settings = db.get_settings()?;
    let p = settings
        .compat_providers
        .iter_mut()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| anyhow!("provider not found"))?;
    if !p.is_ready() {
        return Err(anyhow!("api key not configured"));
    }
    let ids = fetch_provider_models(p.chat_protocol(), &p.base_url, &p.api_key).await?;
    p.models = merge_replace_models(&p.models, ids);
    let models = p.models.clone();
    db.save_settings(&settings)?;

    let mut catalog = db.get_model_catalog().unwrap_or_default();
    catalog.compat = settings.all_compat_model_ids();
    catalog.updated_at = Utc::now().to_rfc3339();
    db.save_model_catalog(&catalog)?;
    Ok(models)
}

async fn fetch_openai_compat(url: &str, api_key: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Resp {
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let client = reqwest::Client::new();
    let mut req = client.get(url);
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{status}: {text}"));
    }
    let parsed: Resp = resp.json().await?;
    let mut ids: Vec<_> = parsed.data.into_iter().map(|i| i.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn fetch_gemini(api_key: &str, base_url: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Resp {
        models: Option<Vec<Item>>,
    }
    #[derive(Deserialize)]
    struct Item {
        name: String,
        #[serde(default, rename = "supportedGenerationMethods")]
        supported: Vec<String>,
    }
    let root = strip_slash(base_url)
        .strip_suffix("/v1beta")
        .unwrap_or(strip_slash(base_url));
    let url = format!("{root}/v1beta/models?key={}&pageSize=100", api_key);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{status}: {text}"));
    }
    let parsed: Resp = resp.json().await?;
    let mut ids = Vec::new();
    for m in parsed.models.unwrap_or_default() {
        if !m.supported.is_empty() && !m.supported.iter().any(|s| s == "generateContent") {
            continue;
        }
        let id = m
            .name
            .strip_prefix("models/")
            .unwrap_or(&m.name)
            .to_string();
        if !id.is_empty() {
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn fetch_claude(api_key: &str, base_url: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Resp {
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let root = strip_slash(base_url);
    let url = if root.ends_with("/v1") {
        format!("{root}/models")
    } else {
        format!("{root}/v1/models")
    };
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{status}: {text}"));
    }
    let parsed: Resp = resp.json().await?;
    let mut ids: Vec<_> = parsed.data.into_iter().map(|i| i.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn fetch_ollama(base_url: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Resp {
        models: Option<Vec<Item>>,
    }
    #[derive(Deserialize)]
    struct Item {
        name: Option<String>,
        model: Option<String>,
    }
    let root = strip_slash(base_url)
        .strip_suffix("/v1")
        .unwrap_or(strip_slash(base_url));
    let url = format!("{root}/api/tags");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{status}: {text}"));
    }
    let parsed: Resp = resp.json().await?;
    let mut ids = Vec::new();
    for m in parsed.models.unwrap_or_default() {
        let id = m.name.or(m.model).unwrap_or_default();
        if !id.is_empty() {
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn fetch_azure(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let root = strip_slash(base_url);
    let url = format!("{root}/openai/models?api-version=2024-10-21");
    #[derive(Deserialize)]
    struct Resp {
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let client = reqwest::Client::new();
    let resp = client.get(&url).header("api-key", api_key).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("{status}: {text}"));
    }
    let parsed: Resp = resp.json().await?;
    let mut ids: Vec<_> = parsed.data.into_iter().map(|i| i.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::{merge_intersect_models, merge_replace_models, models_list_url};

    #[test]
    fn models_url_uses_base_as_is() {
        assert_eq!(
            models_list_url("https://open.bigmodel.cn/api/paas/v4/"),
            "https://open.bigmodel.cn/api/paas/v4/models"
        );
        assert_eq!(
            models_list_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/models"
        );
    }

    #[test]
    fn merge_keeps_manual_names() {
        let prev = vec!["gpt-4".into(), "my-custom".into()];
        let fetched = vec!["gpt-4".into(), "gpt-5".into()];
        assert_eq!(
            merge_replace_models(&prev, fetched.clone()),
            vec!["gpt-4", "gpt-5", "my-custom"]
        );
        assert_eq!(
            merge_intersect_models(&prev, fetched),
            vec!["gpt-4", "my-custom"]
        );
    }

    #[test]
    fn merge_intersect_only_manual_takes_fetched_plus_manual() {
        let prev = vec!["custom".into()];
        let fetched = vec!["a".into(), "b".into()];
        assert_eq!(
            merge_intersect_models(&prev, fetched),
            vec!["a", "b", "custom"]
        );
    }
}
