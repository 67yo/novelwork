//! Fetch live model IDs from providers that have API keys configured.
use crate::db::Db;
use crate::models::ModelCatalog;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Deserialize;

pub async fn refresh(db: &Db) -> Result<ModelCatalog> {
    let settings = db.get_settings()?;
    let mut catalog = db.get_model_catalog().unwrap_or_default();
    catalog.errors.clear();

    if !settings.deepseek_api_key.trim().is_empty() {
        match fetch_openai_compat(
            &format!(
                "{}/models",
                settings.deepseek_base_url.trim_end_matches('/')
            ),
            &settings.deepseek_api_key,
        )
        .await
        {
            Ok(ids) => catalog.deepseek = ids,
            Err(e) => {
                catalog.errors.insert("deepseek".into(), e.to_string());
            }
        }
    }

    if !settings.gemini_api_key.trim().is_empty() {
        match fetch_gemini(&settings.gemini_api_key).await {
            Ok(ids) => catalog.gemini = ids,
            Err(e) => {
                catalog.errors.insert("gemini".into(), e.to_string());
            }
        }
    }

    if !settings.claude_api_key.trim().is_empty() {
        match fetch_claude(&settings.claude_api_key).await {
            Ok(ids) => catalog.claude = ids,
            Err(e) => {
                catalog.errors.insert("claude".into(), e.to_string());
            }
        }
    }

    if !settings.grok_api_key.trim().is_empty() {
        match fetch_openai_compat("https://api.x.ai/v1/models", &settings.grok_api_key).await {
            Ok(ids) => catalog.grok = ids,
            Err(e) => {
                catalog.errors.insert("grok".into(), e.to_string());
            }
        }
    }

    catalog.updated_at = Utc::now().to_rfc3339();
    db.save_model_catalog(&catalog)?;
    Ok(catalog)
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
    let resp = client.get(url).bearer_auth(api_key).send().await?;
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

async fn fetch_gemini(api_key: &str) -> Result<Vec<String>> {
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
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}&pageSize=100",
        api_key
    );
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

async fn fetch_claude(api_key: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Resp {
        data: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.anthropic.com/v1/models")
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
