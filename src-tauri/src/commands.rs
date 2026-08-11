use crate::chapter_constraints::{self, LandBeat};
use crate::chapter_memory;
use crate::db::Db;
use crate::knowledge::{
    build_knowledge_chunks, fetch_urls_context, load_source, load_source_from_url,
    with_fetched_url_context, upsert_chunks,
};
use crate::llm;
use crate::models::*;
use crate::paths::*;
use crate::prompts::{self, PromptLocale};
use crate::AppState;
use chrono::{Local, Timelike, Utc};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Emitter, State};
use tauri_plugin_dialog::DialogExt;

const CREATE_CHAT_CANCEL_KEY: &str = "__create__";
const KNOWLEDGE_EXTRACT_CHAT_CANCEL_KEY: &str = "__knowledge_extract__";

struct ClearChatCancel<'a> {
    state: &'a AppState,
    key: String,
}

impl Drop for ClearChatCancel<'_> {
    fn drop(&mut self) {
        self.state.clear_chat_cancel(&self.key);
    }
}

fn emit_chat_progress(app: &tauri::AppHandle, novel_id: &str, step: &str) {
    let _ = app.emit(
        "chat-progress",
        serde_json::json!({ "novelId": novel_id, "step": step }),
    );
}

/// 预生成 / 精修 / 下一章 / 重抽记忆：阶段 + 进度（index 从 1 起）。
fn emit_chapter_progress(
    app: &tauri::AppHandle,
    novel_id: &str,
    step: &str,
    index: u32,
    total: u32,
) {
    let _ = app.emit(
        "chapter-progress",
        serde_json::json!({
            "novelId": novel_id,
            "step": step,
            "index": index,
            "total": total,
        }),
    );
}

/// 当前 LLM 请求的 token（发送前为估算，返回后为 API/实算）。
fn emit_chapter_tokens(
    app: &tauri::AppHandle,
    novel_id: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
    confirmed: bool,
) {
    let _ = app.emit(
        "chapter-tokens",
        serde_json::json!({
            "novelId": novel_id,
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
            "confirmed": confirmed,
        }),
    );
}

fn estimate_prompt_tokens(system: &str, user: &str) -> u32 {
    ((system.len() + user.len()) / 4) as u32
}

/// LLM 调用并按本地小时累计 token；`novel_id` 为空则只计入全局（不归入小说报表）。
/// 若同时传入 `app` + `novel_id`，会推送 `chapter-tokens` 供等待界面展示。
async fn llm_complete(
    app: Option<&tauri::AppHandle>,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
    novel_id: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(String, bool), String> {
    llm_complete_ex(
        app, state, settings, system, user, model, novel_id, cancel, false,
    )
    .await
}

async fn llm_complete_ex(
    app: Option<&tauri::AppHandle>,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
    novel_id: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
    json_object: bool,
) -> Result<(String, bool), String> {
    if let (Some(app), Some(nid)) = (app, novel_id) {
        emit_chapter_tokens(app, nid, estimate_prompt_tokens(system, user), 0, false);
    }
    let r = llm::complete(settings, system, user, model, cancel, json_object)
        .await
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("cancelled") {
                "cancelled".into()
            } else {
                s
            }
        })?;
    if let (Some(app), Some(nid)) = (app, novel_id) {
        emit_chapter_tokens(app, nid, r.prompt_tokens, r.completion_tokens, true);
    }
    let now = Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let hour = now.hour() as u8;
    let _ = state.db.add_token_usage(
        &day,
        hour,
        &r.model,
        novel_id.unwrap_or(""),
        r.prompt_tokens,
        r.completion_tokens,
        r.total_tokens,
    );
    Ok((r.content, r.used_mock))
}

/// If `text` contains http(s) links, fetch main body and append for the LLM.
async fn enrich_user_with_urls(text: &str) -> String {
    let fetched = fetch_urls_context(text, 12_000).await;
    with_fetched_url_context(text, &fetched)
}

async fn enrich_user_with_urls_progress(
    app: &tauri::AppHandle,
    novel_id: &str,
    text: &str,
) -> String {
    if crate::knowledge::extract_http_urls(text).is_empty() {
        return text.to_string();
    }
    emit_chat_progress(app, novel_id, "fetch_web");
    enrich_user_with_urls(text).await
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        if chars.is_empty() {
            String::new()
        } else {
            "*".repeat(chars.len())
        }
    } else {
        let head: String = chars.iter().take(3).collect();
        let tail: String = chars.iter().rev().take(4).rev().collect();
        format!("{head}****{tail}")
    }
}

fn apply_optional_key(target: &mut String, incoming: Option<String>) {
    let Some(k) = incoming else { return };
    let t = k.trim().to_string();
    // ignore masked paste / empty (empty = leave unchanged; keys may stay blank)
    if t.is_empty() || t.contains('*') {
        return;
    }
    *target = t;
}

/// 设置任务模型；空则回退 default_model。
fn task_model(preferred: &str, fallback: &str) -> String {
    let p = preferred.trim();
    if p.is_empty() {
        fallback.trim().to_string()
    } else {
        p.to_string()
    }
}

/// Chat 面板覆盖：非空则写入本轮 settings 对应字段，供下游 task_model 使用。
fn apply_chat_model_override(target: &mut String, model: Option<&str>) {
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        *target = m.to_string();
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    let s = state.db.get_settings().map_err(|e| e.to_string())?;
    let mut catalog = state
        .db
        .get_model_catalog()
        .unwrap_or_else(|_| ModelCatalog::seed());
    if catalog.compat.is_empty() {
        catalog.compat = s.all_compat_model_ids();
    }
    Ok(SettingsView {
        compat_providers: s
            .compat_providers
            .iter()
            .map(|p| CompatProviderView {
                id: p.id.clone(),
                label: p.label.clone(),
                protocol: p.protocol.clone(),
                base_url: p.base_url.clone(),
                api_key_masked: mask_key(&p.api_key),
                api_key_configured: !p.api_key.trim().is_empty(),
                models: p.models.clone(),
            })
            .collect(),
        gemini_api_key_masked: mask_key(&s.gemini_api_key),
        gemini_api_key_configured: !s.gemini_api_key.trim().is_empty(),
        claude_api_key_masked: mask_key(&s.claude_api_key),
        claude_api_key_configured: !s.claude_api_key.trim().is_empty(),
        default_model: s.default_model,
        create_model: s.create_model,
        generate_model: s.generate_model,
        chat_model: s.chat_model,
        refine_model: s.refine_model,
        knowledge_model: s.knowledge_model,
        model_catalog: catalog,
        ui_locale: s.ui_locale,
    })
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    input: SaveSettingsInput,
) -> Result<SettingsView, String> {
    let mut s = state.db.get_settings().map_err(|e| e.to_string())?;
    if let Some(providers) = input.compat_providers {
        let prev: std::collections::HashMap<_, _> = s
            .compat_providers
            .iter()
            .map(|p| (p.id.clone(), p.api_key.clone()))
            .collect();
        let mut next = Vec::with_capacity(providers.len());
        for p in providers {
            let id = if p.id.trim().is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                p.id.trim().to_string()
            };
            let mut api_key = prev.get(&id).cloned().unwrap_or_default();
            apply_optional_key(&mut api_key, p.api_key);
            next.push(CompatProvider {
                id,
                label: p.label.trim().to_string(),
                protocol: {
                    let t = p.protocol.trim();
                    if t.is_empty() {
                        "openai".into()
                    } else {
                        t.to_string()
                    }
                },
                base_url: p.base_url.trim().to_string(),
                api_key,
                models: p.models,
            });
        }
        s.compat_providers = next;
    }
    apply_optional_key(&mut s.gemini_api_key, input.gemini_api_key);
    apply_optional_key(&mut s.claude_api_key, input.claude_api_key);
    let set_model = |slot: &mut String, v: Option<String>| {
        if let Some(m) = v {
            if !m.trim().is_empty() {
                *slot = m.trim().to_string();
            }
        }
    };
    set_model(&mut s.default_model, input.default_model);
    set_model(&mut s.create_model, input.create_model);
    set_model(&mut s.generate_model, input.generate_model);
    set_model(&mut s.chat_model, input.chat_model);
    set_model(&mut s.refine_model, input.refine_model);
    set_model(&mut s.knowledge_model, input.knowledge_model);
    if let Some(loc) = input.ui_locale {
        let t = loc.trim();
        if !t.is_empty() {
            s.ui_locale = t.to_string();
        }
    }
    state.db.save_settings(&s).map_err(|e| e.to_string())?;
    get_settings(state)
}

#[tauri::command]
pub async fn refresh_model_catalog(state: State<'_, AppState>) -> Result<ModelCatalog, String> {
    crate::catalog::refresh(&state.db)
        .await
        .map_err(|e| e.to_string())
}

/// Probe OpenAI-compat `/v1/models` with a raw key (add-provider flow).
#[tauri::command]
pub async fn fetch_compat_models(
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let key = api_key.trim();
    if key.is_empty() || key.contains('*') {
        return Err("api key required".into());
    }
    if base_url.trim().is_empty() {
        return Err("base url required".into());
    }
    crate::catalog::fetch_openai_models(base_url.trim(), key)
        .await
        .map_err(|e| e.to_string())
}

/// Re-fetch models for a saved provider (uses stored key).
#[tauri::command]
pub async fn refresh_compat_provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    crate::catalog::refresh_provider_models(&state.db, provider_id.trim())
        .await
        .map_err(|e| e.to_string())
}

fn collect_known_genres(state: &State<'_, AppState>) -> Vec<String> {
    let mut out: Vec<String> = GENRES.iter().map(|s| (*s).to_string()).collect();
    if let Ok(books) = state.db.list_knowledge() {
        for b in books {
            for g in b.genres {
                let g = g.trim().to_string();
                if g.is_empty() {
                    continue;
                }
                if !out.iter().any(|x| x == &g) {
                    out.push(g);
                }
            }
        }
    }
    out
}

#[tauri::command]
pub fn list_genres(state: State<'_, AppState>) -> Vec<String> {
    collect_known_genres(&state)
}

#[tauri::command]
pub fn list_knowledge_bases(state: State<'_, AppState>) -> Result<Vec<KnowledgeBook>, String> {
    state.db.list_knowledge().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_knowledge(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    state
        .db
        .rename_knowledge(&id, &title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_knowledge(
    state: State<'_, AppState>,
    id: String,
    title: String,
    author: String,
    extract_prompt: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let known = collect_known_genres(&state);
    let genres = prompts::merge_genre_tags(genres, Vec::new(), &known);
    state
        .db
        .update_knowledge(&id, &title, &author, &extract_prompt, &genres)
        .map_err(|e| e.to_string())?;
    state
        .db
        .get_knowledge(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())
}

#[tauri::command]
pub fn list_knowledge_chunks(
    state: State<'_, AppState>,
    book_id: String,
) -> Result<Vec<KnowledgeChunk>, String> {
    state
        .db
        .list_knowledge_chunks(&book_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_knowledge(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<KnowledgeBook, String> {
    state
        .db
        .set_knowledge_archived(&id, archived)
        .map_err(|e| e.to_string())?;
    state
        .db
        .get_knowledge(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())
}

#[tauri::command]
pub async fn delete_knowledge(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let book = state
        .db
        .get_knowledge(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    if !book.archived {
        return Err("请先归档后再删除".into());
    }
    let _ = crate::knowledge::delete_chunks(&id).await;
    state.db.delete_knowledge(&id).map_err(|e| e.to_string())?;
    let _ = state.db.unlink_knowledge_from_novels(&id);
    Ok(())
}

#[tauri::command]
pub async fn knowledge_extract_chat(
    state: State<'_, AppState>,
    input: KnowledgeExtractChatInput,
) -> Result<KnowledgeExtractChatResult, String> {
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.knowledge_model, input.model.as_deref());
    let user_bits: Vec<&str> = input
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect();
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &user_bits);
    let transcript = input
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");
    let system = prompts::knowledge_extract_chat_system(loc, input.from_url);
    let mut user = if transcript.is_empty() {
        prompts::knowledge_extract_chat_welcome(loc, input.from_url).to_string()
    } else {
        transcript
    };
    user = enrich_user_with_urls(&user).await;
    if let Some((_name, injected)) = crate::skills::maybe_inject_skill(&user) {
        user = injected;
    }

    let model = task_model(&settings.knowledge_model, &settings.default_model);
    let cancel = state.arm_chat_cancel(KNOWLEDGE_EXTRACT_CHAT_CANCEL_KEY);
    let _clear = ClearChatCancel {
        state: &*state,
        key: KNOWLEDGE_EXTRACT_CHAT_CANCEL_KEY.into(),
    };
    let (reply, used_mock) = llm_complete(
        None,
        &state,
        &settings,
        &system,
        &user,
        Some(&model),
        None,
        Some(cancel),
    )
    .await?;

    let extract_prompt = extract_json_objects(&reply).into_iter().find_map(|v| {
        v.get("extract_prompt")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    });

    Ok(KnowledgeExtractChatResult {
        reply,
        used_mock,
        extract_prompt,
    })
}

#[tauri::command]
pub async fn import_knowledge_text(
    state: State<'_, AppState>,
    path: String,
    extract_prompt: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let src = load_source(&path).map_err(|e| e.to_string())?;
    ingest_knowledge_source(&state, src, path, extract_prompt, genres, false).await
}

#[tauri::command]
pub async fn import_knowledge_url(
    state: State<'_, AppState>,
    url: String,
    extract_prompt: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let src = load_source_from_url(&url).await.map_err(|e| e.to_string())?;
    ingest_knowledge_source(&state, src, url, extract_prompt, genres, true).await
}

/// LLM analysis + chunk build. Returns (title, author, genres, prompt, chunks).
async fn analyze_knowledge_source(
    state: &State<'_, AppState>,
    src: &mut crate::knowledge::ParsedSource,
    extract_prompt: String,
    genres: Vec<String>,
    from_url: bool,
) -> Result<(String, String, Vec<String>, String, Vec<String>), String> {
    let mut title = src.title.clone();
    let mut author = src.author.clone();

    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let toc = src.toc_lines();
    let samples = src.style_samples(900, 12_000);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[&extract_prompt, &title, &author, &toc, &samples],
    );

    let known_genres = collect_known_genres(state);
    let known_joined = known_genres.join("、");

    let mut analysis: Option<String> = None;
    let mut auto_genres: Vec<String> = src.subjects.clone();
    let prompt = if extract_prompt.trim().is_empty() {
        if from_url {
            prompts::default_knowledge_url_prompt(loc)
        } else {
            prompts::default_knowledge_book_prompt(loc)
        }
        .to_string()
    } else {
        extract_prompt
    };

    if settings.any_compat_key()
        || !settings.gemini_api_key.trim().is_empty()
        || !settings.claude_api_key.trim().is_empty()
    {
        let model = task_model(&settings.knowledge_model, &settings.default_model);
        let sys = if from_url {
            prompts::knowledge_url_extract_system(loc)
        } else {
            prompts::knowledge_extract_system(loc)
        };
        let user = prompts::knowledge_extract_user(
            loc,
            &prompt,
            &title,
            &author,
            &toc,
            &samples,
            &known_joined,
        );
        if let Ok((out, _)) =
            llm_complete(None, state, &settings, sys, &user, Some(&model), None, None).await
        {
            prompts::apply_analysis_meta(&out, &mut title, &mut author);
            auto_genres.extend(prompts::extract_genres_from_analysis(&out));
            analysis = Some(out);
        }
    }

    let genres = prompts::merge_genre_tags(genres, auto_genres, &known_genres);
    src.title = title.clone();
    src.author = author.clone();
    let chunks = build_knowledge_chunks(src, analysis.as_deref());
    Ok((title, author, genres, prompt, chunks))
}

async fn ingest_knowledge_source(
    state: &State<'_, AppState>,
    mut src: crate::knowledge::ParsedSource,
    source_path: String,
    extract_prompt: String,
    genres: Vec<String>,
    from_url: bool,
) -> Result<KnowledgeBook, String> {
    let (title, author, genres, prompt, chunks) =
        analyze_knowledge_source(state, &mut src, extract_prompt, genres, from_url).await?;
    let book = KnowledgeBook {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        author,
        genres,
        source_path,
        extract_prompt: prompt,
        created_at: Utc::now().to_rfc3339(),
        chunk_count: chunks.len() as i64,
        archived: false,
    };
    state
        .db
        .insert_knowledge(&book, &chunks)
        .map_err(|e| e.to_string())?;
    let _ = upsert_chunks(&book.id, &chunks).await;
    maybe_index_book_embeddings(state, &book.id).await;
    Ok(book)
}

async fn maybe_index_book_embeddings(state: &State<'_, AppState>, book_id: &str) {
    let Ok(settings) = state.db.get_settings() else {
        return;
    };
    let _ = state.db.delete_knowledge_embeddings(book_id);
    let _ = crate::kb_context::index_book_embeddings(&state.db, &settings, book_id).await;
}

fn is_http_source(path: &str) -> bool {
    let p = path.trim().to_ascii_lowercase();
    p.starts_with("http://") || p.starts_with("https://")
}

/// Re-read source (file or URL) and replace stored knowledge chunks for an existing book.
#[tauri::command]
pub async fn reextract_knowledge(
    state: State<'_, AppState>,
    id: String,
    path: Option<String>,
    extract_prompt: Option<String>,
) -> Result<KnowledgeBook, String> {
    let book = state
        .db
        .get_knowledge(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    let source_path = path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| book.source_path.clone());
    if source_path.trim().is_empty() {
        return Err("无源路径，请重新选择文件或填写网址".into());
    }
    let from_url = is_http_source(&source_path);
    let mut src = if from_url {
        load_source_from_url(&source_path)
            .await
            .map_err(|e| e.to_string())?
    } else {
        if !Path::new(&source_path).is_file() {
            return Err("源文件不存在或已移动，请重新选择文件".into());
        }
        load_source(&source_path).map_err(|e| e.to_string())?
    };
    let focus = extract_prompt
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| book.extract_prompt.clone());
    let (title, author, genres, prompt, chunks) = analyze_knowledge_source(
        &state,
        &mut src,
        focus,
        book.genres.clone(),
        from_url,
    )
    .await?;
    let _ = crate::knowledge::delete_chunks(&id).await;
    state
        .db
        .replace_knowledge_content(
            &id,
            &title,
            &author,
            &genres,
            &source_path,
            &prompt,
            &chunks,
        )
        .map_err(|e| e.to_string())?;
    let _ = upsert_chunks(&id, &chunks).await;
    maybe_index_book_embeddings(&state, &id).await;
    state
        .db
        .get_knowledge(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeIndexStatus {
    pub book_id: String,
    pub chunk_count: i64,
    pub embedding_count: i64,
}

#[tauri::command]
pub fn knowledge_index_status(
    state: State<'_, AppState>,
    book_id: String,
) -> Result<KnowledgeIndexStatus, String> {
    let book = state
        .db
        .get_knowledge(&book_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    let embedding_count = state
        .db
        .count_knowledge_embeddings(&book_id)
        .map_err(|e| e.to_string())?;
    Ok(KnowledgeIndexStatus {
        book_id,
        chunk_count: book.chunk_count,
        embedding_count,
    })
}

#[tauri::command]
pub async fn rebuild_knowledge_index(
    state: State<'_, AppState>,
    book_id: String,
) -> Result<KnowledgeIndexStatus, String> {
    let book = state
        .db
        .get_knowledge(&book_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let _ = state.db.delete_knowledge_embeddings(&book_id);
    let n = crate::kb_context::index_book_embeddings(&state.db, &settings, &book_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(KnowledgeIndexStatus {
        book_id,
        chunk_count: book.chunk_count,
        embedding_count: n as i64,
    })
}

#[tauri::command]
pub fn list_novels(state: State<'_, AppState>) -> Result<Vec<NovelProject>, String> {
    state.db.list_novels().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_novel(state: State<'_, AppState>, id: String) -> Result<Option<NovelProject>, String> {
    state.db.get_novel(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_novel(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    novel.archived = archived;
    novel.updated_at = Utc::now().to_rfc3339();
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    let _ = fs::write(
        novel_meta_path(&id),
        serde_json::to_string_pretty(&novel).unwrap_or_default(),
    );
    Ok(novel)
}

#[tauri::command]
pub fn delete_novel(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let novel = state
        .db
        .get_novel(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    if !novel.archived {
        return Err("请先归档后再删除".into());
    }
    state.db.delete_novel(&id).map_err(|e| e.to_string())?;
    let lance_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = chapter_memory::delete_lance_novel(&lance_id).await;
    });
    let dir = novel_dir(&id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn create_novel(
    state: State<'_, AppState>,
    input: CreateNovelInput,
) -> Result<NovelProject, String> {
    create_novel_with_tree(
        &state,
        &input.title,
        &input.synopsis,
        &input.knowledge_ids,
        &input.knowledge_strategy,
        2000,
        3000,
        20,
        None,
        None,
    )
}

/// 统计正文有效字数（去掉空白）。
fn count_words(text: &str) -> u32 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u32
}

/// 相对根节点每章目标字数的允许误差（非空白字符）。
/// 仅写入预生成/精修 Prompt，并由完成消息标注；落地补写不做字数验证。
const WORD_COUNT_TOLERANCE: u32 = 60;

fn normalize_word_range(min: u32, max: u32) -> (u32, u32) {
    if min == 0 && max == 0 {
        (2000, 3000)
    } else if max < min {
        (max.max(1), min)
    } else {
        (min.max(1), max.max(min.max(1)))
    }
}

/// 优先用树根节点上的每章字数；未设时回退小说项目字段。
fn root_chapter_word_target(tree: &NovelTree, novel: &NovelProject) -> (u32, u32) {
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel));
    let (a, b) = root
        .map(|r| (r.word_count_min, r.word_count_max))
        .unwrap_or((0, 0));
    if a > 0 || b > 0 {
        normalize_word_range(a, b)
    } else {
        normalize_word_range(novel.word_count_min, novel.word_count_max)
    }
}

fn word_tolerance_bounds(wmin: u32, wmax: u32) -> (u32, u32) {
    (
        wmin.saturating_sub(WORD_COUNT_TOLERANCE),
        wmax.saturating_add(WORD_COUNT_TOLERANCE),
    )
}

fn word_count_ok(words: u32, wmin: u32, wmax: u32) -> bool {
    let (lo, hi) = word_tolerance_bounds(wmin, wmax);
    words >= lo && words <= hi
}

/// 从用户话里抠每章目标字数（不依赖模型是否吐 JSON）。
fn parse_word_target_from_text(text: &str) -> Option<(u32, u32)> {
    let lower = text.to_lowercase();
    // 收集 3–5 位数字
    let mut nums: Vec<u32> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if (3..=5).contains(&cur.len()) {
                if let Ok(n) = cur.parse::<u32>() {
                    if (500..=20000).contains(&n) {
                        nums.push(n);
                    }
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && (3..=5).contains(&cur.len()) {
        if let Ok(n) = cur.parse::<u32>() {
            if (500..=20000).contains(&n) {
                nums.push(n);
            }
        }
    }
    if nums.is_empty() {
        return None;
    }

    let mentions_chapter_words = text.contains('字')
        || lower.contains("word")
        || lower.contains("字数")
        || text.contains("每章")
        || lower.contains("per chapter")
        || lower.contains("chapter length")
        || lower.contains("target");
    if !mentions_chapter_words {
        return None;
    }

    // 区间：出现两个数字且文中有 –/-/到/至/~
    if nums.len() >= 2
        && (text.contains('–')
            || text.contains('-')
            || text.contains('—')
            || text.contains('~')
            || text.contains('到')
            || text.contains('至')
            || lower.contains(" to "))
    {
        return Some(normalize_word_range(nums[0], nums[1]));
    }
    let n = *nums.last()?;
    Some((n, n))
}

fn word_target_from_json(v: &serde_json::Value) -> Option<(u32, u32)> {
    if let Some(wc) = v.get("word_count") {
        if let Some(n) = wc.as_u64() {
            let n = n as u32;
            return Some(normalize_word_range(n, n));
        }
        let min = wc
            .get("min")
            .or_else(|| wc.get("word_count_min"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let max = wc
            .get("max")
            .or_else(|| wc.get("word_count_max"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let target = wc.get("target").and_then(|x| x.as_u64()).map(|x| x as u32);
        if let (Some(a), Some(b)) = (min, max) {
            return Some(normalize_word_range(a, b));
        }
        if let Some(t) = target.or(min).or(max) {
            return Some(normalize_word_range(t, t));
        }
    }
    let min = v
        .get("word_count_min")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    let max = v
        .get("word_count_max")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    match (min, max) {
        (Some(a), Some(b)) => Some(normalize_word_range(a, b)),
        (Some(a), None) => Some(normalize_word_range(a, a)),
        (None, Some(b)) => Some(normalize_word_range(b, b)),
        _ => None,
    }
}

fn apply_novel_word_target(
    state: &State<'_, AppState>,
    novel: &mut NovelProject,
    tree: &mut NovelTree,
    min: u32,
    max: u32,
) -> Result<(), String> {
    let (wmin, wmax) = normalize_word_range(min, max);
    novel.word_count_min = wmin;
    novel.word_count_max = wmax;
    novel.updated_at = Utc::now().to_rfc3339();
    if let Some(root) = tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        root.word_count_min = wmin;
        root.word_count_max = wmax;
    }
    state.db.upsert_novel(novel).map_err(|e| e.to_string())
}

fn normalize_chapter_count(n: u32) -> u32 {
    n.clamp(1, 500)
}

fn chapter_count_from_json(v: &serde_json::Value) -> Option<u32> {
    if let Some(n) = v.get("chapter_count").and_then(|x| x.as_u64()) {
        return Some(normalize_chapter_count(n as u32));
    }
    if let Some(c) = v.get("chapters") {
        if let Some(n) = c.as_u64() {
            return Some(normalize_chapter_count(n as u32));
        }
        if let Some(n) = c
            .get("total")
            .or_else(|| c.get("count"))
            .and_then(|x| x.as_u64())
        {
            return Some(normalize_chapter_count(n as u32));
        }
    }
    None
}

/// 从用户话里抠全书计划章数（如「一共20章」「计划 30 章」「20 chapters」）。
fn parse_chapter_count_from_text(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let hint = text.contains("一共")
        || text.contains("总共")
        || text.contains("全书")
        || text.contains("整本")
        || (text.contains("计划") && text.contains("章"))
        || (text.contains("预计") && text.contains("章"))
        || lower.contains("total chapters")
        || lower.contains("chapter count")
        || lower.contains("chapters total")
        || (lower.contains("chapters")
            && (lower.contains("total") || lower.contains("about") || lower.contains("plan")));
    if !hint {
        return None;
    }
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<u32>() {
                if (1..=500).contains(&n) {
                    nums.push(n);
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(n) = cur.parse::<u32>() {
            if (1..=500).contains(&n) {
                nums.push(n);
            }
        }
    }
    // 优先取 ≤200 的数（避开误抓字数）；否则取第一个合法值
    nums.iter()
        .copied()
        .find(|n| (1..=200).contains(n))
        .or_else(|| nums.first().copied())
        .map(normalize_chapter_count)
}

fn apply_novel_chapter_count(
    state: &State<'_, AppState>,
    novel: &mut NovelProject,
    tree: &mut NovelTree,
    count: u32,
) -> Result<(), String> {
    let n = normalize_chapter_count(count);
    novel.chapter_count = n;
    novel.updated_at = Utc::now().to_rfc3339();
    if let Some(root) = tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        root.chapter_count = n;
    }
    state.db.upsert_novel(novel).map_err(|e| e.to_string())
}

/// Manual edit: per-chapter word range + planned total chapters (also syncs root node).
#[tauri::command]
pub fn update_novel_plan(
    state: State<'_, AppState>,
    novel_id: String,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut tree = get_tree(novel_id)?;
    apply_novel_word_target(
        &state,
        &mut novel,
        &mut tree,
        word_count_min,
        word_count_max,
    )?;
    apply_novel_chapter_count(&state, &mut novel, &mut tree, chapter_count)?;
    let _ = save_tree(tree);
    Ok(novel)
}

/// 根节点：绑定公共知识库 + 严格/参考模式 + 可选策略文案。
#[tauri::command]
pub fn update_novel_canon(
    state: State<'_, AppState>,
    novel_id: String,
    knowledge_ids: Vec<String>,
    canon_mode: String,
    knowledge_strategy: Option<String>,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut ids = knowledge_ids;
    ids.retain(|id| !id.trim().is_empty());
    ids.sort();
    ids.dedup();
    // Drop unknown / archived
    ids.retain(|id| {
        state
            .db
            .get_knowledge(id)
            .ok()
            .flatten()
            .map(|b| !b.archived)
            .unwrap_or(false)
    });
    novel.knowledge_ids = ids;
    novel.canon_mode = if canon_mode.eq_ignore_ascii_case("strict") {
        "strict".into()
    } else {
        "reference".into()
    };
    if let Some(s) = knowledge_strategy {
        novel.knowledge_strategy = s;
    }
    novel.updated_at = Utc::now().to_rfc3339();
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    let _ = fs::write(
        novel_meta_path(&novel.id),
        serde_json::to_string_pretty(&novel).unwrap_or_default(),
    );
    Ok(novel)
}

fn set_node_word_count(tree: &mut NovelTree, node_id: &str, words: u32) {
    if let Some(n) = tree.nodes.iter_mut().find(|n| n.id == node_id) {
        n.word_count = words;
    }
}

fn create_novel_with_tree(
    state: &State<'_, AppState>,
    title: &str,
    synopsis: &str,
    knowledge_ids: &[String],
    knowledge_strategy: &str,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
    _chapters: Option<&[serde_json::Value]>,
    characters: Option<&[serde_json::Value]>,
) -> Result<NovelProject, String> {
    let now = Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let (wmin, wmax) = normalize_word_range(word_count_min, word_count_max);
    let chapters_n = normalize_chapter_count(if chapter_count == 0 {
        20
    } else {
        chapter_count
    });
    let novel = NovelProject {
        id: id.clone(),
        title: title.to_string(),
        synopsis: synopsis.to_string(),
        cover_path: None,
        knowledge_ids: knowledge_ids.to_vec(),
        knowledge_strategy: if knowledge_strategy.is_empty() {
            "参考对话中确定的叙事与人物约束。".into()
        } else {
            knowledge_strategy.to_string()
        },
        canon_mode: "reference".into(),
        archived: false,
        word_count_min: wmin,
        word_count_max: wmax,
        chapter_count: chapters_n,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    fs::write(
        novel_meta_path(&id),
        serde_json::to_string_pretty(&novel).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let tree = build_initial_tree(&id, title, synopsis, wmin, wmax, chapters_n, characters);
    fs::write(
        novel_tree_path(&id),
        serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(novel)
}

fn build_initial_tree(
    novel_id: &str,
    title: &str,
    synopsis: &str,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
    characters: Option<&[serde_json::Value]>,
) -> NovelTree {
    let root = "root".to_string();
    let mut nodes = vec![TreeNode {
        id: root.clone(),
        kind: NodeKind::Novel,
        label: title.to_string(),
        outline: synopsis.to_string(),
        character: None,
        knowledge: None,
        side_plot: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition { x: 280.0, y: 0.0 },
        word_count: 0,
        word_count_min,
        word_count_max,
        chapter_count,
    }];
    let mut edges = vec![];

    // ponytail: 开树不建章节；用户定内容后经 Chat 生成大纲再挂章节节点
    if let Some(chars) = characters {
        for (i, c) in chars.iter().enumerate() {
            let cid = format!("char-{}", i + 1);
            let label = c
                .get("label")
                .and_then(|x| x.as_str())
                .unwrap_or("新角色")
                .to_string();
            let y = 80.0 + 140.0 * (i as f64);
            nodes.push(TreeNode {
                id: cid.clone(),
                kind: NodeKind::Character,
                label,
                outline: String::new(),
                character: Some(CharacterCard {
                    role: c
                        .get("role")
                        .and_then(|x| x.as_str())
                        .unwrap_or("配角")
                        .into(),
                    personality: c
                        .get("personality")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                    gender: c
                        .get("gender")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                    alignment: c
                        .get("alignment")
                        .and_then(|x| x.as_str())
                        .unwrap_or("中立")
                        .into(),
                }),
                knowledge: None,
                side_plot: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition {
                    x: 40.0,
                    y: y + 20.0,
                },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            });
            if let Some(root_node) = nodes.iter_mut().find(|n| n.id == root) {
                root_node.linked_character_ids.push(cid.clone());
            }
            edges.push(TreeEdge {
                id: format!("e-{root}-{cid}"),
                source: root.clone(),
                target: cid,
                kind: "character".into(),
                source_handle: Some("left".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            });
        }
    }

    NovelTree {
        novel_id: novel_id.into(),
        nodes,
        edges,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChapterResolveMode {
    /// 更新已有同号章节的标题/大纲，缺失则追加
    Overwrite,
    /// 只追加树上还没有的编号
    Skip,
    /// 一律追加新节点（允许重复章号）
    ForceAppend,
}

fn root_id(tree: &NovelTree) -> String {
    tree.nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
        .unwrap_or_else(|| "root".into())
}

fn cn_digit(ch: char) -> Option<u32> {
    match ch {
        '零' | '〇' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '两' | '兩' | '贰' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' | '陸' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn is_cn_num_char(ch: char) -> bool {
    cn_digit(ch).is_some() || ch == '十' || ch == '百'
}

/// 中文数字 → u32（常见章号：一…九十九、一百…）。
fn parse_chinese_numeral(s: &str) -> Option<u32> {
    let chars: Vec<char> = s.chars().filter(|c| is_cn_num_char(*c)).collect();
    if chars.is_empty() {
        return None;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '百' {
            if n == 0 {
                n = 1;
            }
            n = n.saturating_mul(100);
            i += 1;
            continue;
        }
        if c == '十' {
            if n == 0 {
                n = 1;
            }
            n = n.saturating_mul(10);
            i += 1;
            continue;
        }
        let Some(v) = cn_digit(c) else {
            return None;
        };
        if v == 0 {
            i += 1;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == '百' {
            n = n.saturating_add(v.saturating_mul(100));
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == '十' {
            n = n.saturating_add(v.saturating_mul(10));
            i += 2;
            continue;
        }
        n = n.saturating_add(v);
        i += 1;
    }
    if (1..=500).contains(&n) {
        Some(n)
    } else {
        None
    }
}

fn parse_chapter_num_token(tok: &str) -> Option<u32> {
    let tok = tok.trim();
    if tok.is_empty() {
        return None;
    }
    if let Ok(n) = tok.parse::<u32>() {
        return (1..=500).contains(&n).then_some(n);
    }
    parse_chinese_numeral(tok)
}

pub(crate) fn chapter_number_from_label(label: &str) -> Option<u32> {
    if let Some(i) = label.find('第') {
        let rest = &label[i + '第'.len_utf8()..];
        let mut body = String::new();
        let mut rem = rest;
        for (idx, ch) in rest.char_indices() {
            if ch == '章' {
                return parse_chapter_num_token(&body);
            }
            if ch.is_whitespace() {
                continue;
            }
            if ch.is_ascii_digit() || is_cn_num_char(ch) {
                body.push(ch);
                rem = &rest[idx + ch.len_utf8()..];
            } else {
                rem = &rest[idx..];
                break;
            }
        }
        // 允许「第1」「第一」省略「章」；若后面是区间词则不是单章号
        let rem = rem.trim_start();
        if !body.is_empty()
            && (rem.is_empty()
                || rem.starts_with('·')
                || rem.starts_with('・')
                || rem.starts_with('—'))
        {
            return parse_chapter_num_token(&body);
        }
    }
    // 「10章」「十二章」无「第」
    if let Some(j) = label.find('章') {
        let head = label[..j].trim();
        // 避免把「第一到第十章」整段误解析
        if !head.contains('到') && !head.contains('至') && !head.contains('-') {
            if let Some(n) = parse_chapter_num_token(head) {
                return Some(n);
            }
        }
    }
    let lower = label.to_lowercase();
    if let Some(i) = lower.find("chapter") {
        let rest = lower[i + "chapter".len()..].trim_start();
        let mut body = String::new();
        for ch in rest.chars() {
            if ch.is_ascii_digit() {
                body.push(ch);
            } else if body.is_empty() && (ch.is_whitespace() || ch == '.' || ch == '#') {
                continue;
            } else {
                break;
            }
        }
        return parse_chapter_num_token(&body);
    }
    None
}

/// 从指令参数里解析章号：`3` / `第三章` / `第三章 · 夜` / `一`。
fn parse_chapter_ref(args: &str) -> Option<u32> {
    let a = args.trim();
    if a.is_empty() {
        return None;
    }
    if let Some(n) = chapter_number_from_label(a) {
        return Some(n);
    }
    // 取第一个空白前的 token
    let token = a.split_whitespace().next().unwrap_or(a);
    if let Some(n) = chapter_number_from_label(token) {
        return Some(n);
    }
    if token.ends_with('章') {
        if let Some(n) = chapter_number_from_label(&format!("第{token}")) {
            return Some(n);
        }
    } else if let Some(n) = chapter_number_from_label(&format!("第{token}章")) {
        return Some(n);
    }
    // token 自身可能是「一」「十二」
    parse_chapter_num_token(token).or_else(|| extract_small_chapter_nums(a).into_iter().next())
}

/// 章号 → 节点 id（同号多个时取 y 最小的一个）。
/// 标题解析不出章号时，按画布自上而下顺序填入空缺编号（避免第 1 章标题缺「第1章」时漏检，重跑从第 2 章起）。
fn existing_chapter_nums(tree: &NovelTree) -> std::collections::HashMap<u32, String> {
    let chapters = sorted_chapter_nodes(tree);
    let mut map = std::collections::HashMap::new();
    for n in &chapters {
        if let Some(num) = chapter_number_from_label(&n.label) {
            map.entry(num).or_insert_with(|| n.id.clone());
        }
    }
    let mut next = 1u32;
    for n in &chapters {
        if chapter_number_from_label(&n.label).is_some() {
            continue;
        }
        while map.contains_key(&next) {
            next = next.saturating_add(1);
        }
        map.insert(next, n.id.clone());
        next = next.saturating_add(1);
    }
    map
}

/// 写入树时保证 label 带可解析章号，便于下次 `/章节卡` 冲突/覆盖命中。
fn normalize_chapter_label(n: u32, label: &str) -> String {
    let t = label.trim();
    if t.is_empty() {
        return format!("第{n}章");
    }
    if chapter_number_from_label(t) == Some(n) {
        return t.to_string();
    }
    format!("第{n}章 · {t}")
}

fn last_chapter_anchor(tree: &NovelTree) -> (String, f64, f64) {
    let root = root_id(tree);
    let mut chapters: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(last) = chapters.last() {
        (last.id.clone(), last.position.x, last.position.y)
    } else if let Some(r) = tree.nodes.iter().find(|n| n.id == root) {
        (root, r.position.x, r.position.y)
    } else {
        (root, 280.0, 0.0)
    }
}

/// 删除全部章节节点及其相关边（角色/剧情卡保留）。返回被删章节 id。
fn clear_all_chapter_nodes(tree: &mut NovelTree) -> Vec<String> {
    let old: Vec<String> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .map(|n| n.id.clone())
        .collect();
    if old.is_empty() {
        return old;
    }
    tree.nodes.retain(|n| !matches!(n.kind, NodeKind::Chapter));
    tree.edges.retain(|e| {
        e.kind != "chapter" && !old.iter().any(|id| e.source == *id || e.target == *id)
    });
    let alive: std::collections::HashSet<String> =
        tree.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut tree.nodes {
        n.linked_character_ids.retain(|id| alive.contains(id));
        n.linked_side_plot_ids.retain(|id| alive.contains(id));
        n.linked_knowledge_ids.retain(|id| alive.contains(id));
    }
    old
}

/// 清除章节正文文件 + SQLite/Lance 章节记忆（树节点已删时仍须调用）。
fn purge_chapter_artifacts(state: &State<'_, AppState>, novel_id: &str, node_ids: &[String]) {
    if node_ids.is_empty() {
        return;
    }
    for node_id in node_ids {
        let _ = fs::remove_file(chapter_path(novel_id, node_id));
        let _ = state.db.delete_chapter_memory(novel_id, node_id);
    }
    let nid = novel_id.to_string();
    let ids = node_ids.to_vec();
    tauri::async_runtime::spawn(async move {
        for id in ids {
            let _ = chapter_memory::delete_lance(&nid, &id).await;
        }
    });
}

fn clear_all_side_plot_nodes(tree: &mut NovelTree) {
    let old: Vec<String> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::SidePlot))
        .map(|n| n.id.clone())
        .collect();
    if old.is_empty() {
        return;
    }
    tree.nodes.retain(|n| !matches!(n.kind, NodeKind::SidePlot));
    tree.edges.retain(|e| {
        e.kind != "side_plot" && !old.iter().any(|id| e.source == *id || e.target == *id)
    });
    let alive: std::collections::HashSet<String> =
        tree.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut tree.nodes {
        n.linked_character_ids.retain(|id| alive.contains(id));
        n.linked_side_plot_ids.retain(|id| alive.contains(id));
        n.linked_knowledge_ids.retain(|id| alive.contains(id));
    }
}

fn push_chapter_node(
    tree: &mut NovelTree,
    prev: &mut String,
    x: f64,
    y: &mut f64,
    label: String,
    outline: String,
) {
    *y += 140.0;
    let cid = uuid::Uuid::new_v4().to_string();
    tree.nodes.push(TreeNode {
        id: cid.clone(),
        kind: NodeKind::Chapter,
        label,
        outline,
        character: None,
        knowledge: None,
        side_plot: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition { x, y: *y },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    tree.edges.push(TreeEdge {
        id: format!("e-{prev}-{cid}"),
        source: prev.clone(),
        target: cid.clone(),
        kind: "chapter".into(),
        source_handle: Some("bottom".into()),
        target_handle: Some("top".into()),
        label: String::new(),
    });
    *prev = cid;
}

/// 将大纲挂到树上：按模式追加/覆盖，不再默认整链替换。
fn apply_chapter_outlines(
    tree: &mut NovelTree,
    list: &[serde_json::Value],
    mode: ChapterResolveMode,
) {
    let existing = existing_chapter_nums(tree);
    let (mut prev, x, mut y) = last_chapter_anchor(tree);

    for (i, c) in list.iter().enumerate() {
        let n = c
            .get("n")
            .or_else(|| c.get("index"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32)
            .or_else(|| {
                c.get("label")
                    .or_else(|| c.get("title"))
                    .and_then(|x| x.as_str())
                    .and_then(chapter_number_from_label)
            })
            .unwrap_or((i + 1) as u32);
        let label = normalize_chapter_label(
            n,
            c.get("label")
                .or_else(|| c.get("title"))
                .and_then(|x| x.as_str())
                .unwrap_or(""),
        );
        let outline = c
            .get("outline")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        match mode {
            ChapterResolveMode::Skip if existing.contains_key(&n) => continue,
            ChapterResolveMode::Overwrite => {
                if let Some(id) = existing.get(&n) {
                    if let Some(node) = tree.nodes.iter_mut().find(|x| x.id == *id) {
                        node.label = label;
                        node.outline = outline;
                    }
                    continue;
                }
                push_chapter_node(tree, &mut prev, x, &mut y, label, outline);
            }
            ChapterResolveMode::Skip | ChapterResolveMode::ForceAppend => {
                push_chapter_node(tree, &mut prev, x, &mut y, label, outline);
            }
        }
    }
}

fn add_plot_to_chapter(
    tree: &mut NovelTree,
    chapter_num: u32,
    plot_text: &str,
) -> Result<String, String> {
    let existing = existing_chapter_nums(tree);
    let chapter_id = existing
        .get(&chapter_num)
        .cloned()
        .ok_or_else(|| format!("结构树上找不到第{chapter_num}章，请先生成章节卡。"))?;
    let plot_text = plot_text.trim();
    if plot_text.is_empty() {
        return Err("剧情内容为空。".into());
    }
    let label: String = plot_text.chars().take(24).collect();
    let label = if plot_text.chars().count() > 24 {
        format!("{label}…")
    } else {
        label
    };
    let plots = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::SidePlot))
        .count();
    let chapter_pos = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .map(|n| n.position.clone())
        .unwrap_or(NodePosition { x: 280.0, y: 140.0 });
    let pid = uuid::Uuid::new_v4().to_string();
    tree.nodes.push(TreeNode {
        id: pid.clone(),
        kind: NodeKind::SidePlot,
        label,
        outline: plot_text.to_string(),
        character: None,
        knowledge: None,
        side_plot: Some(SidePlotMeta::active()),
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition {
            x: chapter_pos.x + 280.0,
            y: chapter_pos.y + plots as f64 * 24.0,
        },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    if let Some(ch) = tree.nodes.iter_mut().find(|n| n.id == chapter_id) {
        ch.linked_side_plot_ids.push(pid.clone());
    }
    tree.edges.push(TreeEdge {
        id: format!("e-{chapter_id}-{pid}"),
        source: chapter_id,
        target: pid,
        kind: "side_plot".into(),
        source_handle: Some("right".into()),
        target_handle: Some("left".into()),
        label: String::new(),
    });
    Ok(format!(
        "已为第{chapter_num}章添加剧情卡，可继续追加更多剧情。"
    ))
}

fn parse_resolve_mode(text: &str) -> Option<ChapterResolveMode> {
    let lower = text.to_lowercase();
    if text.contains("覆盖") || text.contains("替换") || lower.contains("overwrite") {
        Some(ChapterResolveMode::Overwrite)
    } else if text.contains("跳过") || text.contains("只补") || lower.contains("skip") {
        Some(ChapterResolveMode::Skip)
    } else if text.contains("强制") || text.contains("仍要追加") || lower.contains("force") {
        Some(ChapterResolveMode::ForceAppend)
    } else {
        None
    }
}

fn extract_small_chapter_nums(text: &str) -> Vec<u32> {
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if (1..=3).contains(&cur.len()) {
                if let Ok(n) = cur.parse::<u32>() {
                    if (1..=500).contains(&n) {
                        nums.push(n);
                    }
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && (1..=3).contains(&cur.len()) {
        if let Ok(n) = cur.parse::<u32>() {
            if (1..=500).contains(&n) {
                nums.push(n);
            }
        }
    }
    nums
}

/// `/指令名 args` → 返回 args；按名称长度优先匹配。
fn slash_args<'a>(text: &'a str, names: &[&str]) -> Option<&'a str> {
    let t = text.trim();
    let rest = t.strip_prefix('/')?;
    let mut names: Vec<&str> = names.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        if let Some(after) = rest.strip_prefix(name) {
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                return Some(after.trim());
            }
        }
    }
    None
}

fn root_slash_help(zh: bool) -> String {
    if zh {
        "根节点指令（以 / 开头）：\n\
         · `/章节卡 1-10` — 生成章节卡（同号已存在则覆盖重写，从指定起点章开始）\n\
         · `/章节卡 覆盖 3-5` — 显式覆盖同号标题与大纲\n\
         · `/章节卡 跳过 1-10` / `/章节卡 强制追加 1-10` — 跳过已有 / 强制再追加节点\n\
         · `/刷新大纲 3` — 覆盖重写指定章标题与大纲（根 Chat 可改任意章；可换行写期望）\n\
         · `/添加剧情 3 剧情内容`（也支持 `/添加剧情 第一章 …`）\n\
         · `/剧情卡 3` 或 `/剧情卡 第一章` — 按该章大纲拆多张剧情卡并补人物\n\
         · `/清空章节` — 删除全部章节节点\n\
         · `/清空所有剧情` — 删除全部剧情卡（章节/角色保留）\n\
         · `/skill-name …` 或 `@skill-name …` — 启用 ~/.agents/skills 中的 skill（输入 / 可补全；也可自动匹配）"
            .into()
    } else {
        "Root slash commands:\n\
         · `/chapters 1-10` — generate cards (overwrite same numbers if they exist; starts at the range start)\n\
         · `/chapters overwrite 3-5` — explicit replace same-number titles/outlines\n\
         · `/chapters skip|force 1-10` — skip existing / force-append nodes\n\
         · `/refresh-outline 3` — overwrite that chapter’s title & outline (expectations below)\n\
         · `/add-plot 3 plot text` — add one plot card to chapter 3\n\
         · `/plots 3` — auto plot cards from chapter 3 outline + characters\n\
         · `/clear-chapters` — delete all chapter nodes\n\
         · `/clear-plots` — delete all plot cards (chapters/characters kept)\n\
         · `/skill-name …` or `@skill-name …` — skill from ~/.agents/skills (type / to list; or auto-match)"
            .into()
    }
}

fn parse_clear_all_chapters(text: &str) -> bool {
    slash_args(
        text,
        &[
            "清空章节",
            "清空所有章节",
            "清空所有章节节点",
            "clear-chapters",
            "clear_chapters",
        ],
    )
    .is_some()
}

fn parse_clear_all_plots(text: &str) -> bool {
    slash_args(
        text,
        &[
            "清空所有剧情",
            "清空剧情",
            "清空所有剧情卡",
            "清空剧情卡",
            "clear-plots",
            "clear_plots",
        ],
    )
    .is_some()
}

/// `/剧情卡 3` · `/剧情卡 第一章`
fn parse_gen_plots_for_chapter(text: &str) -> Option<u32> {
    let args = slash_args(text, &["生成剧情卡", "剧情卡", "plots", "gen-plots"])?;
    parse_chapter_ref(args)
}

/// 章节卡 Chat：`/完善剧情`（当前章）；可选换行后自定义剧情要求
fn parse_enrich_plots_command(text: &str) -> Option<String> {
    slash_args(
        text,
        &["完善剧情", "生成剧情", "enrich-plots", "enrich_plots"],
    )
    .map(|s| s.to_string())
}

/// 章节卡 Chat：`/下一章` · `/下一章 3` · `/下一章 3\n\n{材料与期望}`
fn parse_plan_next_command(text: &str) -> Option<(u32, String)> {
    let args = slash_args(
        text,
        &[
            "下一章",
            "生成下一章",
            "plan-next",
            "next-chapter",
            "next-chapters",
        ],
    )?;
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Some((1, String::new()));
    }
    if let Some((head, rest)) = trimmed.split_once('\n') {
        let n = head
            .trim()
            .parse::<u32>()
            .unwrap_or(1)
            .clamp(1, 12);
        return Some((n, rest.trim().to_string()));
    }
    if let Ok(n) = trimmed.parse::<u32>() {
        return Some((n.clamp(1, 12), String::new()));
    }
    Some((1, trimmed.to_string()))
}

/// 章节卡 Chat：`/预生成` · `/预生成\n\n{条件}`（默认不带历史记忆检索）
fn parse_generate_body_command(text: &str) -> Option<String> {
    slash_args(
        text,
        &[
            "预生成",
            "生成正文",
            "重新生成",
            "generate",
            "pre-generate",
            "regen",
            "regenerate",
        ],
    )
    .map(|s| s.to_string())
}

/// 章节卡 Chat：`/精修` · `/精修\n\n{条件}`（默认 memory + 前 10 章）
fn parse_refine_body_command(text: &str) -> Option<String> {
    slash_args(text, &["精修", "refine"]).map(|s| s.to_string())
}

/// 仅接受短章号 token（`3` / `三` / `第三章`），避免把期望散文里的「两年」「3天」误当成章号。
fn parse_outline_chapter_token(tok: &str) -> Option<u32> {
    let tok = tok.trim();
    if tok.is_empty() || tok.chars().count() > 12 {
        return None;
    }
    if let Some(n) = parse_chapter_num_token(tok) {
        return Some(n);
    }
    chapter_number_from_label(tok)
}

fn chapter_slash_help(zh: bool) -> String {
    if zh {
        "章节卡 Chat 只对当前章有效：\n\
         · `/完善剧情` — 按本章大纲补剧情卡\n\
         · `/刷新本章大纲` — 覆盖重写本章标题与大纲（可换行写期望；勿写章号）\n\
         · `/预生成` — 预生成本章正文\n\
         · `/精修` — 精修本章正文\n\
         改其他章 / 批量生成：请切到根节点 Chat（如 `/刷新大纲 3`、`/章节卡`）。"
            .into()
    } else {
        "Chapter-card Chat only affects this chapter:\n\
         · `/enrich-plots` — plot cards from this outline\n\
         · `/refresh-this-outline` — overwrite this title & outline (expectations below; no chapter number)\n\
         · `/generate` — pre-generate this body\n\
         · `/refine` — refine this body\n\
         Other chapters / batch: use root Chat (`/refresh-outline 3`, `/chapters`)."
            .into()
    }
}

/// 根 Chat：`/刷新大纲 3`；章节卡：`/刷新本章大纲`（兼容旧名 `/刷新大纲`）。
/// 返回 `(章号, 期望)`；章号 `None` = 未指定。
/// 章节卡禁止带章号（仅当前章）；根路径须带章号。
fn parse_regen_outline_command(text: &str) -> Option<(Option<u32>, String)> {
    let args = slash_args(
        text,
        &[
            "刷新本章大纲",
            "刷新大纲",
            "重写大纲",
            "refresh-this-outline",
            "refresh-outline",
            "regen-outline",
        ],
    )?;
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Some((None, String::new()));
    }
    if let Some((head, rest)) = trimmed.split_once('\n') {
        let head = head.trim();
        let brief = rest.trim().to_string();
        if head.is_empty() {
            return Some((None, brief));
        }
        if let Some(n) = parse_outline_chapter_token(head) {
            return Some((Some(n), brief));
        }
        // 首行不是章号 → 整段当期望（章节卡刷新当前章）
        return Some((None, trimmed.to_string()));
    }
    // 同行：仅当首个空白分隔 token 是短章号时才拆出章号，其余全文为期望
    let (first, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((a, b)) => (a, b.trim()),
        None => (trimmed, ""),
    };
    if let Some(n) = parse_outline_chapter_token(first) {
        return Some((Some(n), rest.to_string()));
    }
    Some((None, trimmed.to_string()))
}

/// 按章节大纲拆剧情卡并写入树；返回带落库说明的回复。
/// `outline_override` / `user_notes` 供左侧确认框传入；空则用树上大纲、无补充要求。
async fn enrich_plots_for_chapter_node(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel: &NovelProject,
    novel_id: &str,
    tree: &mut NovelTree,
    chapter_id: &str,
    chapter_num: Option<u32>,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
    outline_override: Option<&str>,
    user_notes: &str,
    use_chapter_progress: bool,
) -> Result<(String, usize, usize), String> {
    let chapter = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .cloned()
        .ok_or_else(|| "章节节点丢失".to_string())?;
    let outline = outline_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(chapter.outline.trim());
    if outline.is_empty() {
        let msg = if loc.is_zh() {
            format!(
                "本章「{}」尚无大纲，请先完善章节大纲再生成剧情卡。",
                chapter.label
            )
        } else {
            format!(
                "Chapter “{}” has no outline yet. Fill the outline first.",
                chapter.label
            )
        };
        return Err(msg);
    }
    let existing_chars: String = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| {
            let card = n.character.as_ref();
            format!(
                "- {} | {} | {}",
                n.label,
                card.map(|c| c.role.as_str()).unwrap_or(""),
                card.map(|c| c.personality.as_str()).unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "gen_plots", 2, 3);
    } else {
        emit_chat_progress(app, novel_id, "thinking");
    }
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (_def_label, def_role, def_align) = prompts::default_new_character(loc);
    let num = chapter_num.unwrap_or_else(|| chapter_number_from_label(&chapter.label).unwrap_or(0));
    let system =
        prompts::gen_chapter_plots_system(loc, &novel.title, &novel.synopsis, novel.chapter_count);
    let user = prompts::gen_chapter_plots_user(
        loc,
        num,
        &chapter.label,
        outline,
        &existing_chars,
        user_notes,
    );
    let (reply, _) = llm_complete(
        Some(app),
        state,
        settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        cancel,
    )
    .await?;
    let mut added_plots = 0usize;
    let mut added_chars = 0usize;
    // Models often pretty-print / fence JSON — don't require a single-line object.
    if let Some(list) = first_json_array_field(&reply, "plots") {
        if !list.is_empty() {
            if use_chapter_progress {
                emit_chapter_progress(app, novel_id, "saving", 3, 3);
            } else {
                emit_chat_progress(app, novel_id, "apply_card");
            }
            let (p, c) = apply_plots_and_characters_for_chapter(
                tree, chapter_id, &list, def_role, def_align,
            );
            added_plots = p;
            added_chars = c;
        }
    }
    if added_plots > 0 {
        let _ = save_tree(tree.clone());
    }
    if !use_chapter_progress {
        emit_chat_progress(app, novel_id, "saving");
    } else if added_plots == 0 {
        emit_chapter_progress(app, novel_id, "saving", 3, 3);
    }
    let note = if loc.is_zh() {
        if added_plots > 0 {
            format!(
                "\n\n（已为本章新增 {added_plots} 张剧情卡，补齐人物卡 {added_chars} 张，并已关联。）"
            )
        } else {
            "\n\n（未解析到 plots JSON，结构树未改动。请重试。）".into()
        }
    } else if added_plots > 0 {
        format!("\n\n(Added {added_plots} plot card(s) and {added_chars} new character(s).)")
    } else {
        "\n\n(No plots JSON found; tree unchanged.)".into()
    };
    Ok((format!("{reply}{note}"), added_plots, added_chars))
}

fn find_character_id_by_label(tree: &NovelTree, label: &str) -> Option<String> {
    let key = label.trim();
    if key.is_empty() {
        return None;
    }
    tree.nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .find(|n| {
            let l = n.label.trim();
            l == key || l.contains(key) || key.contains(l)
        })
        .map(|n| n.id.clone())
}

fn ensure_character_linked(tree: &mut NovelTree, host_id: &str, char_id: &str) {
    if let Some(host) = tree.nodes.iter_mut().find(|n| n.id == host_id) {
        if !host.linked_character_ids.contains(&char_id.to_string()) {
            host.linked_character_ids.push(char_id.to_string());
        }
    }
    let exists = tree.edges.iter().any(|e| {
        e.kind == "character"
            && ((e.source == host_id && e.target == char_id)
                || (e.source == char_id && e.target == host_id))
    });
    if !exists {
        tree.edges.push(TreeEdge {
            id: format!("e-{host_id}-{char_id}"),
            source: host_id.to_string(),
            target: char_id.to_string(),
            kind: "character".into(),
            source_handle: Some("left".into()),
            target_handle: Some("right".into()),
            label: String::new(),
        });
    }
}

/// 根据 JSON 为某章追加多张剧情卡，并补齐/关联人物卡。
fn apply_plots_and_characters_for_chapter(
    tree: &mut NovelTree,
    chapter_id: &str,
    plots: &[serde_json::Value],
    def_role: &str,
    def_align: &str,
) -> (usize, usize) {
    let chapter_pos = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .map(|n| n.position.clone())
        .unwrap_or(NodePosition { x: 280.0, y: 140.0 });
    let mut plot_count = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::SidePlot))
        .count();
    let mut char_y = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| n.position.y)
        .fold(80.0_f64, f64::max);
    let mut added_plots = 0usize;
    let mut added_chars = 0usize;

    for p in plots {
        let label = p
            .get("label")
            .or_else(|| p.get("title"))
            .and_then(|x| x.as_str())
            .unwrap_or("剧情")
            .to_string();
        let outline = p
            .get("outline")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if label.trim().is_empty() && outline.trim().is_empty() {
            continue;
        }
        let pid = uuid::Uuid::new_v4().to_string();
        tree.nodes.push(TreeNode {
            id: pid.clone(),
            kind: NodeKind::SidePlot,
            label: if label.trim().is_empty() {
                outline.chars().take(24).collect()
            } else {
                label
            },
            outline,
            character: None,
            knowledge: None,
            side_plot: Some(SidePlotMeta::active()),
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition {
                x: chapter_pos.x + 280.0,
                y: chapter_pos.y + plot_count as f64 * 36.0,
            },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        plot_count += 1;
        if let Some(ch) = tree.nodes.iter_mut().find(|n| n.id == chapter_id) {
            ch.linked_side_plot_ids.push(pid.clone());
        }
        tree.edges.push(TreeEdge {
            id: format!("e-{chapter_id}-{pid}"),
            source: chapter_id.to_string(),
            target: pid.clone(),
            kind: "side_plot".into(),
            source_handle: Some("right".into()),
            target_handle: Some("left".into()),
            label: String::new(),
        });
        added_plots += 1;

        let chars = p
            .get("characters")
            .or_else(|| p.get("roles"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for c in chars {
            let clabel = c
                .get("label")
                .or_else(|| c.get("name"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if clabel.is_empty() {
                continue;
            }
            let cid = if let Some(id) = find_character_id_by_label(tree, &clabel) {
                id
            } else {
                char_y += 120.0;
                let cid = uuid::Uuid::new_v4().to_string();
                tree.nodes.push(TreeNode {
                    id: cid.clone(),
                    kind: NodeKind::Character,
                    label: clabel,
                    outline: String::new(),
                    character: Some(CharacterCard {
                        role: c
                            .get("role")
                            .and_then(|x| x.as_str())
                            .unwrap_or(def_role)
                            .into(),
                        personality: c
                            .get("personality")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                        gender: c
                            .get("gender")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                        alignment: c
                            .get("alignment")
                            .and_then(|x| x.as_str())
                            .unwrap_or(def_align)
                            .into(),
                    }),
                    knowledge: None,
                    side_plot: None,
                    linked_character_ids: vec![],
                    linked_side_plot_ids: vec![],
                    linked_knowledge_ids: vec![],
                    position: NodePosition { x: 40.0, y: char_y },
                    word_count: 0,
                    word_count_min: 0,
                    word_count_max: 0,
                    chapter_count: 0,
                });
                added_chars += 1;
                cid
            };
            ensure_character_linked(tree, &pid, &cid);
            ensure_character_linked(tree, chapter_id, &cid);
        }
    }
    (added_plots, added_chars)
}

fn strip_resolve_prefix(args: &str) -> &str {
    let t = args.trim();
    for p in [
        "覆盖冲突",
        "覆盖已有",
        "覆盖",
        "跳过已有",
        "跳过冲突",
        "跳过",
        "强制追加",
        "强制",
        "只补缺",
    ] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim();
        }
    }
    let lower = t.to_lowercase();
    for p in ["overwrite", "skip", "force"] {
        if lower.starts_with(p) {
            return t[p.len()..].trim();
        }
    }
    t
}

/// 解析章号区间：第一到第十章 / 第1-10章 / 第 1-10 章 / 1-10 / 第一章-第十章
fn parse_chapter_range(args: &str) -> Option<(u32, u32)> {
    let raw = strip_resolve_prefix(args);
    // 去掉空白，便于匹配「第 1-10 章」
    let s: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return None;
    }

    let pair = |a: &str, b: &str| -> Option<(u32, u32)> {
        let x = parse_chapter_ref(a)?;
        let y = parse_chapter_ref(b)?;
        let (lo, hi) = if x <= y { (x, y) } else { (y, x) };
        if hi.saturating_sub(lo) > 200 {
            return None;
        }
        Some((lo, hi))
    };

    // 第一到第十章 / 第一至第十章
    for sep in ["到第", "至第"] {
        if let Some(i) = s.find(sep) {
            let left = &s[..i];
            let right = format!("第{}", &s[i + sep.len()..]);
            if let Some(r) = pair(left, &right) {
                return Some(r);
            }
        }
    }
    // 第一到十章 / 一到十
    for sep in ["到", "至"] {
        if s.contains("到第") || s.contains("至第") {
            break;
        }
        if let Some(i) = s.find(sep) {
            if let Some(r) = pair(&s[..i], &s[i + sep.len()..]) {
                return Some(r);
            }
        }
    }
    // 第1-10章 / 1-10 / 第一章-第十章
    for sep in ["-", "–", "—", "~"] {
        if let Some(i) = s.find(sep) {
            if let Some(r) = pair(&s[..i], &s[i + sep.len()..]) {
                return Some(r);
            }
        }
    }
    let lower = raw.to_lowercase();
    if let Some(i) = lower.find(" to ") {
        let left = raw[..i].trim();
        let right = raw[i + 4..].trim();
        if let Some(r) = pair(left, right) {
            return Some(r);
        }
    }
    None
}

/// LLM 生成章节卡大纲并写入结构树。返回 (模型原文, 是否已落盘应用)。
/// 根设定 + 编号小于 `before_n` 的章节大纲与记忆（再拼期望占位）。
fn assemble_outline_gen_brief(
    state: &State<'_, AppState>,
    novel: &NovelProject,
    tree: &NovelTree,
    novel_id: &str,
    before_n: u32,
    from: u32,
    to: u32,
    nums: &[u32],
    loc: PromptLocale,
    cur_body: Option<&str>,
) -> String {
    let mut root_ref = root_generate_reference(tree, &novel.synopsis, loc);
    root_ref.push_str(&root_linked_plots_and_knowledge(tree, loc));
    let query = format!(
        "{} {}\nchapters {from}-{to}",
        novel.title, novel.synopsis
    );
    let canon = build_canon_context(state, novel, tree, &query, loc);
    if !canon.trim().is_empty() {
        root_ref.push_str(&canon);
    }
    let chapters = sorted_chapter_nodes(tree);
    let prior: Vec<TreeNode> = chapters
        .into_iter()
        .filter(|c| {
            chapter_number_from_label(&c.label)
                .map(|n| n < before_n)
                .unwrap_or(false)
        })
        .collect();
    let prior_block = if prior.is_empty() {
        String::new()
    } else {
        let q = format!("{} {}", novel.title, novel.synopsis);
        build_refine_memory_context(state, novel_id, &prior, &q, loc)
    };
    prompts::outline_gen_brief(loc, &root_ref, &prior_block, cur_body, from, to, nums)
}

/// 剧情卡是否注入生成/大纲：进行中且未吸收；缺省 meta 视为可注入。
fn plot_is_injectable(n: &TreeNode) -> bool {
    if !matches!(n.kind, NodeKind::SidePlot) {
        return false;
    }
    match &n.side_plot {
        None => true,
        Some(m) => m.is_injectable(),
    }
}

/// 根节点链接的剧情卡 + 知识卡（供大纲生成 brief；正文预生成另有 chapter_context，避免重复改 root_generate_reference）。
fn root_linked_plots_and_knowledge(tree: &NovelTree, loc: PromptLocale) -> String {
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel));
    let root_id = root.map(|n| n.id.as_str()).unwrap_or("");
    let mut plot_ids: Vec<String> = root
        .map(|n| n.linked_side_plot_ids.clone())
        .unwrap_or_default();
    let mut knowledge_ids: Vec<String> = root
        .map(|n| n.linked_knowledge_ids.clone())
        .unwrap_or_default();
    if !root_id.is_empty() {
        for e in &tree.edges {
            if e.source != root_id && e.target != root_id {
                continue;
            }
            let other_id = if e.source == root_id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            match other.kind {
                NodeKind::SidePlot => {
                    if !plot_ids.iter().any(|id| id == other_id) {
                        plot_ids.push(other_id.to_string());
                    }
                }
                NodeKind::Knowledge => {
                    if !knowledge_ids.iter().any(|id| id == other_id) {
                        knowledge_ids.push(other_id.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    let mut plots = String::new();
    for id in &plot_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            if !plot_is_injectable(n) {
                continue;
            }
            let outline = if n.outline.trim().is_empty() {
                if loc.is_zh() {
                    "（大纲为空）"
                } else {
                    "(empty outline)"
                }
            } else {
                n.outline.as_str()
            };
            // ponytail: outline gen only needs beats, not full plot body
            if loc.is_zh() {
                plots.push_str(&format!("- 「{}」（全书剧情·进行中）\n  要点：{outline}\n", n.label));
            } else {
                plots.push_str(&format!(
                    "- “{}” (book-wide · active)\n  Beats: {outline}\n",
                    n.label
                ));
            }
        }
    }

    let mut kn_cards: Vec<(String, String)> = Vec::new();
    for id in &knowledge_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let k = n.knowledge.as_ref();
            let feat = k.map(|x| x.extracted.as_str()).unwrap_or("").trim();
            let req = k.map(|x| x.extract_prompt.as_str()).unwrap_or("").trim();
            let body = if !feat.is_empty() {
                feat.to_string()
            } else if !req.is_empty() {
                req.to_string()
            } else if !n.outline.trim().is_empty() {
                n.outline.clone()
            } else if loc.is_zh() {
                "（尚未提取特征）".into()
            } else {
                "(features not extracted)".into()
            };
            kn_cards.push((n.label.clone(), body));
        }
    }
    let knowledge = crate::kb_context::format_knowledge_cards_capped(
        &kn_cards,
        crate::kb_context::KNOWLEDGE_CARD_EXTRACT_CAP,
        crate::kb_context::ROOT_KNOWLEDGE_TOTAL_CAP,
        loc.is_zh(),
    );

    if plots.is_empty() && knowledge.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if loc.is_zh() {
        if !plots.is_empty() {
            out.push_str(&format!("【根节点关联剧情卡（全书须推进）】\n{plots}"));
        }
        if !knowledge.is_empty() {
            out.push_str(&format!("【根节点关联知识卡（设定/技法约束）】\n{knowledge}"));
        }
    } else {
        if !plots.is_empty() {
            out.push_str(&format!("[Root-linked plot cards (book-wide — advance)]\n{plots}"));
        }
        if !knowledge.is_empty() {
            out.push_str(&format!(
                "[Root-linked knowledge cards (setting/craft constraints)]\n{knowledge}"
            ));
        }
    }
    out
}

/// 单章刷新时模型常照抄 prompt 示例返回 n=1；强制改回目标章号。
fn coerce_outlines_to_single_num(
    list: Vec<serde_json::Value>,
    n: u32,
) -> Vec<serde_json::Value> {
    list.into_iter()
        .map(|mut v| {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("n".to_string(), serde_json::json!(n));
            }
            v
        })
        .collect()
}

async fn apply_llm_chapter_cards(
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel: &NovelProject,
    novel_id: &str,
    tree: &mut NovelTree,
    from: u32,
    to: u32,
    nums: &[u32],
    mode: ChapterResolveMode,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
    // user_brief: empty/short expectation → assemble + merge; legacy full brief used as-is
    user_brief: &str,
    // 追加到材料（如刷新大纲的本章链接卡）；在 merge 期望之前
    material_extra: Option<&str>,
) -> Result<(String, bool), String> {
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let replace = matches!(mode, ChapterResolveMode::Overwrite);
    let mut assembled = assemble_outline_gen_brief(
        state, novel, tree, novel_id, from, from, to, nums, loc, None,
    );
    if let Some(extra) = material_extra.map(str::trim).filter(|s| !s.is_empty()) {
        assembled.push_str(extra);
        if !assembled.ends_with('\n') {
            assembled.push('\n');
        }
    } else if nums.len() == 1 {
        // 单章覆盖/生成：自动注入该章链接人物+剧情（硬约束）及旧大纲
        if let Some(node) = tree.nodes.iter().find(|n| {
            matches!(n.kind, NodeKind::Chapter)
                && chapter_number_from_label(&n.label) == Some(nums[0])
        }) {
            let mut extras = String::new();
            append_regen_outline_extras(&mut extras, tree, node, loc);
            assembled.push_str(&extras);
        }
    }
    let consideration = crate::kb_context::merge_expectation_into_assembled(
        assembled,
        user_brief,
        loc.is_zh(),
    );
    let system = prompts::gen_chapter_cards_system(
        loc,
        &novel.title,
        &novel.synopsis,
        novel.chapter_count,
        from,
        to,
        nums,
        replace,
    );
    let user = prompts::gen_chapter_cards_user(loc, from, to, nums, &consideration);
    // 大纲任务强制 JSON 对象模式：Kimi 等模型否则易夹杂思维链/散文导致解析失败
    let (reply, _) = llm_complete_ex(
        None,
        state,
        settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        cancel.clone(),
        true,
    )
    .await?;
    let mut applied = false;
    let out_reply = match extract_outlines_list_or_repair(
        None,
        state,
        settings,
        &chat_model,
        novel_id,
        loc,
        cancel,
        &reply,
        nums,
    )
    .await
    {
        Ok((final_reply, list)) => {
            let list = if nums.len() == 1 {
                coerce_outlines_to_single_num(list, nums[0])
            } else {
                list
            };
            if !list.is_empty() {
                apply_chapter_outlines(tree, &list, mode);
                applied = true;
            }
            final_reply
        }
        Err(_) => reply,
    };
    if applied {
        let _ = save_tree(tree.clone());
    }
    Ok((out_reply, applied))
}

/// `/章节卡 1-10` · `/章节卡 第一到第十章` · `/章节卡 第 1-10 章`
fn parse_gen_chapter_cards(text: &str) -> Option<(u32, u32, Option<ChapterResolveMode>)> {
    let args = slash_args(text, &["生成章节卡", "章节卡", "chapters", "gen-chapters"])?;
    let resolve = parse_resolve_mode(args);
    if let Some((a, b)) = parse_chapter_range(args) {
        return Some((a, b, resolve));
    }
    // 单章：/章节卡 3 · /章节卡 第一章
    let single = strip_resolve_prefix(args);
    if let Some(n) = parse_chapter_ref(single) {
        return Some((n, n, resolve));
    }
    None
}

/// `/添加剧情 3 剧情正文` · `/添加剧情 第一章 剧情正文`
fn parse_add_plot_command(text: &str) -> Option<(u32, String)> {
    let args = slash_args(
        text,
        &["添加剧情", "增加剧情", "加剧情", "add-plot", "add_plot"],
    )?;
    let chapter = parse_chapter_ref(args)?;
    // 去掉开头的章号引用，剩下为剧情正文
    let rest = if let Some(i) = args.find('第') {
        if let Some(j) = args[i..].find('章') {
            let end = i + j + '章'.len_utf8();
            args[end..].trim()
        } else {
            ""
        }
    } else {
        // 阿拉伯数字或纯中文数字 token
        let token = args.split_whitespace().next().unwrap_or("");
        if token.is_empty() {
            ""
        } else if let Some(pos) = args.find(token) {
            args[pos + token.len()..]
                .trim()
                .trim_start_matches(['章', '节', '：', ':', ' ', '，', ',', '\n'])
        } else {
            ""
        }
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some((chapter, rest.to_string()))
}

async fn finish_chat_reply(
    state: &State<'_, AppState>,
    novel_id: &str,
    reply: String,
) -> Result<Vec<ChatMessage>, String> {
    let assistant = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.to_string(),
        node_id: String::new(),
        role: "assistant".into(),
        content: reply,
        created_at: Utc::now().to_rfc3339(),
    };
    state
        .db
        .insert_chat(&assistant)
        .map_err(|e| e.to_string())?;
    state.db.list_chat(novel_id).map_err(|e| e.to_string())
}

fn finish_card_chat_reply(
    state: &State<'_, AppState>,
    novel_id: &str,
    node_id: &str,
    reply: String,
    tree: NovelTree,
) -> Result<CardChatResult, String> {
    let assistant = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.to_string(),
        node_id: node_id.to_string(),
        role: "assistant".into(),
        content: reply.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    state
        .db
        .insert_chat(&assistant)
        .map_err(|e| e.to_string())?;
    Ok(CardChatResult { reply, tree })
}

#[tauri::command]
pub async fn create_novel_chat(
    state: State<'_, AppState>,
    input: NovelCreateChatInput,
) -> Result<NovelCreateChatResult, String> {
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.create_model, input.model.as_deref());
    let user_bits: Vec<&str> = input
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect();
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &user_bits);
    let transcript = input
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let system = prompts::create_novel_system(loc, input.force_create);

    let user = if transcript.is_empty() {
        prompts::create_novel_empty_user(loc).to_string()
    } else {
        transcript
    };
    let user = enrich_user_with_urls(&user).await;

    let model = task_model(&settings.create_model, &settings.default_model);

    let cancel = state.arm_chat_cancel(CREATE_CHAT_CANCEL_KEY);
    let _clear = ClearChatCancel {
        state: &*state,
        key: CREATE_CHAT_CANCEL_KEY.into(),
    };
    let (reply, used_mock) = llm_complete(
        None,
        &state,
        &settings,
        &system,
        &user,
        Some(&model),
        None,
        Some(cancel),
    )
    .await?;

    let mut novel = None;
    if let Some(draft) = extract_create_json(&reply) {
        let title = draft
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or(prompts::untitled_novel(loc))
            .to_string();
        let synopsis = draft
            .get("synopsis")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let strategy = draft
            .get("knowledge_strategy")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let characters = draft.get("characters").and_then(|x| x.as_array());
        let wmin = draft
            .get("word_count_min")
            .and_then(|x| x.as_u64())
            .unwrap_or(2000) as u32;
        let wmax = draft
            .get("word_count_max")
            .and_then(|x| x.as_u64())
            .unwrap_or(3000) as u32;
        let chapter_count = chapter_count_from_json(&draft)
            .or_else(|| {
                user_bits
                    .iter()
                    .rev()
                    .find_map(|t| parse_chapter_count_from_text(t))
            })
            .unwrap_or(20);
        novel = Some(create_novel_with_tree(
            &state,
            &title,
            &synopsis,
            &[],
            &strategy,
            wmin,
            wmax,
            chapter_count,
            None,
            characters.map(|v| v.as_slice()),
        )?);
    } else if input.force_create {
        // mock / 模型没吐 JSON 时兜底建一本
        let (fallback_label, fallback_syn, fallback_strat) = prompts::chat_created_novel(loc);
        let fallback_title = input
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| {
                let t: String = m.content.chars().take(24).collect();
                if t.is_empty() {
                    fallback_label.to_string()
                } else {
                    t
                }
            })
            .unwrap_or_else(|| fallback_label.to_string());
        let chapter_count = user_bits
            .iter()
            .rev()
            .find_map(|t| parse_chapter_count_from_text(t))
            .unwrap_or(20);
        novel = Some(create_novel_with_tree(
            &state,
            &fallback_title,
            fallback_syn,
            &[],
            fallback_strat,
            2000,
            3000,
            chapter_count,
            None,
            None,
        )?);
    }

    Ok(NovelCreateChatResult {
        reply,
        used_mock,
        novel,
    })
}

/// Shared LLM JSON recovery for all chats: fences, pretty-print, trailing commas.
fn normalize_llm_json_text(text: &str) -> String {
    let t = text.trim().trim_start_matches('\u{feff}');
    // Kimi 等常输出弯引号，serde_json 不认
    let t = t
        .replace(['\u{201c}', '\u{201d}', '\u{201e}', '\u{00ab}', '\u{00bb}'], "\"")
        .replace(['\u{2018}', '\u{2019}', '\u{201a}'], "'");
    if !t.contains("```") {
        return t;
    }
    let mut out = String::with_capacity(t.len());
    for line in t.lines() {
        if line.trim().starts_with("```") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Drop trailing commas before `}` / `]` (common model slip; serde_json rejects them).
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut in_str = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn find_matching_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0isize;
    let mut in_str = false;
    let mut escape = false;
    for (j, &c) in chars.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
                continue;
            }
            if c == '\\' {
                escape = true;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

fn try_parse_json_object(s: &str) -> Option<serde_json::Value> {
    let t = s.trim().trim_start_matches('\u{feff}');
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        if v.is_object() {
            return Some(v);
        }
    }
    let soft = strip_trailing_commas(t);
    if soft != t {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&soft) {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    None
}

/// JSON objects from model text: one-line, pretty-printed, fenced, or trailing-comma.
fn extract_json_objects(text: &str) -> Vec<serde_json::Value> {
    let s = normalize_llm_json_text(text);
    let mut out = Vec::new();
    if let Some(v) = try_parse_json_object(&s) {
        out.push(v);
        return out;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            i += 1;
            continue;
        }
        match find_matching_brace(&chars, i) {
            Some(end) => {
                let slice: String = chars[i..=end].iter().collect();
                if let Some(v) = try_parse_json_object(&slice) {
                    out.push(v);
                }
                i = end + 1;
            }
            None => i += 1,
        }
    }
    out
}

fn first_json_array_field(text: &str, key: &str) -> Option<Vec<serde_json::Value>> {
    for v in extract_json_objects(text) {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            return Some(arr.clone());
        }
    }
    None
}

fn looks_like_outline_item(v: &serde_json::Value) -> bool {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return false,
    };
    obj.contains_key("outline")
        || obj.contains_key("label")
        || obj.contains_key("title")
        || obj.contains_key("n")
        || obj.contains_key("index")
        || obj.contains_key("大纲")
        || obj.contains_key("标题")
}

fn coerce_json_u32(v: &serde_json::Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    if let Some(n) = v.as_i64() {
        if n > 0 {
            return u32::try_from(n).ok();
        }
    }
    if let Some(s) = v.as_str() {
        if let Ok(n) = s.trim().parse::<u32>() {
            return Some(n);
        }
        return parse_chapter_ref(s.trim());
    }
    None
}

fn outline_field_to_string(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        let t = s.trim();
        return if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        };
    }
    if let Some(arr) = v.as_array() {
        let lines: Vec<String> = arr
            .iter()
            .filter_map(|x| {
                x.as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        x.as_object().and_then(|o| {
                            o.get("text")
                                .or_else(|| o.get("beat"))
                                .or_else(|| o.get("content"))
                                .and_then(|t| t.as_str())
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                        })
                    })
            })
            .collect();
        if lines.is_empty() {
            return None;
        }
        return Some(lines.join("\n"));
    }
    None
}

fn normalize_outline_item(v: &serde_json::Value) -> serde_json::Value {
    let mut obj = match v.as_object() {
        Some(o) => o.clone(),
        None => return v.clone(),
    };
    if !obj.contains_key("label") {
        if let Some(t) = obj
            .get("title")
            .or_else(|| obj.get("标题"))
            .cloned()
        {
            obj.insert("label".into(), t);
        }
    }
    if let Some(raw) = obj
        .get("outline")
        .or_else(|| obj.get("大纲"))
        .or_else(|| obj.get("summary"))
        .or_else(|| obj.get("beats"))
        .cloned()
    {
        if let Some(s) = outline_field_to_string(&raw) {
            obj.insert("outline".into(), serde_json::Value::String(s));
        } else if !obj.contains_key("outline") {
            obj.insert("outline".into(), raw);
        }
    }
    if let Some(n) = obj
        .get("n")
        .or_else(|| obj.get("index"))
        .or_else(|| obj.get("章号"))
        .and_then(coerce_json_u32)
    {
        obj.insert("n".into(), serde_json::json!(n));
    }
    serde_json::Value::Object(obj)
}

fn usable_outline_list(list: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    list.into_iter()
        .map(|v| normalize_outline_item(&v))
        .filter(|v| {
            let has_outline = v
                .get("outline")
                .and_then(|x| x.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            let has_label = v
                .get("label")
                .and_then(|x| x.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            has_outline || has_label
        })
        .collect()
}

/// 从模型回复提取章节大纲列表（兼容裸数组、中文键、弯引号、单对象）。
fn extract_outlines_list(text: &str) -> Option<Vec<serde_json::Value>> {
    const KEYS: &[&str] = &["outlines", "chapters", "章节", "大纲列表", "items"];
    for v in extract_json_objects(text) {
        for key in KEYS {
            if let Some(arr) = v.get(*key).and_then(|x| x.as_array()) {
                let list = usable_outline_list(arr.clone());
                if !list.is_empty() {
                    return Some(list);
                }
            }
        }
        // 单对象：{"n":1,"label":"...","outline":"..."}
        if looks_like_outline_item(&v) && v.get("outlines").is_none() {
            let list = usable_outline_list(vec![v]);
            if !list.is_empty() {
                return Some(list);
            }
        }
    }
    // 裸数组：[{...}]
    let s = normalize_llm_json_text(text);
    let trimmed = s.trim();
    if trimmed.starts_with('[') {
        let soft = strip_trailing_commas(trimmed);
        if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&soft)
        {
            let list = usable_outline_list(arr);
            if !list.is_empty() {
                return Some(list);
            }
        }
    }
    first_json_array_field(text, "outlines").and_then(|arr| {
        let list = usable_outline_list(arr);
        if list.is_empty() {
            None
        } else {
            Some(list)
        }
    })
}

/// 解析 outlines；失败则再请求一次「仅输出标准 JSON」（统一各模型格式差异）。
async fn extract_outlines_list_or_repair(
    app: Option<&tauri::AppHandle>,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    model: &str,
    novel_id: &str,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
    reply: &str,
    nums: &[u32],
) -> Result<(String, Vec<serde_json::Value>), String> {
    if let Some(list) = extract_outlines_list(reply) {
        if !list.is_empty() {
            return Ok((reply.to_string(), list));
        }
    }
    let system = prompts::repair_outlines_json_system(loc);
    let user = prompts::repair_outlines_json_user(loc, nums, reply);
    let (fixed, _) = llm_complete_ex(
        app,
        state,
        settings,
        &system,
        &user,
        Some(model),
        Some(novel_id),
        cancel,
        true,
    )
    .await?;
    let list = extract_outlines_list(&fixed).filter(|l| !l.is_empty());
    match list {
        Some(list) => Ok((fixed, list)),
        None => {
            let snip: String = reply.chars().take(280).collect();
            Err(if loc.is_zh() {
                format!("未能解析 outlines JSON，结构树未改动。请重试。\n\n模型原文摘录：\n{snip}")
            } else {
                format!(
                    "Could not parse outlines JSON; tree unchanged. Please retry.\n\nModel snippet:\n{snip}"
                )
            })
        }
    }
}

fn first_json_with_any_key(text: &str, keys: &[&str]) -> Option<serde_json::Value> {
    extract_json_objects(text)
        .into_iter()
        .find(|v| keys.iter().any(|k| v.get(*k).is_some()))
}

fn extract_create_json(reply: &str) -> Option<serde_json::Value> {
    for v in extract_json_objects(reply) {
        if let Some(c) = v.get("create") {
            return Some(c.clone());
        }
        // Bare create payload (model omitted the {"create":...} wrapper).
        if v.get("title").and_then(|x| x.as_str()).is_some() {
            return Some(v);
        }
    }
    None
}

#[tauri::command]
pub fn get_tree(novel_id: String) -> Result<NovelTree, String> {
    let raw = fs::read_to_string(novel_tree_path(&novel_id)).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_tree(tree: NovelTree) -> Result<(), String> {
    fs::write(
        novel_tree_path(&tree.novel_id),
        serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 从磁盘树删除非根节点（章/人/剧情/知识），并清掉边与 linked_*。
#[tauri::command]
pub fn delete_tree_card(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<NovelTree, String> {
    let mut tree = get_tree(novel_id.clone())?;
    let kind = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.kind.clone())
        .ok_or_else(|| "节点不存在".to_string())?;
    if matches!(kind, NodeKind::Novel) {
        return Err("根节点不能删除".into());
    }

    // 删章节时把上下游剧情链接上，避免断链
    if matches!(kind, NodeKind::Chapter) {
        let preds: Vec<(String, Option<String>, Option<String>)> = tree
            .edges
            .iter()
            .filter(|e| e.kind == "chapter" && e.target == node_id)
            .map(|e| (e.source.clone(), e.source_handle.clone(), e.target_handle.clone()))
            .collect();
        let succs: Vec<(String, Option<String>, Option<String>)> = tree
            .edges
            .iter()
            .filter(|e| e.kind == "chapter" && e.source == node_id)
            .map(|e| (e.target.clone(), e.source_handle.clone(), e.target_handle.clone()))
            .collect();
        for (pred, sh, _) in &preds {
            for (succ, _, th) in &succs {
                if pred == succ {
                    continue;
                }
                let exists = tree.edges.iter().any(|e| {
                    e.kind == "chapter" && e.source == *pred && e.target == *succ
                });
                if exists {
                    continue;
                }
                tree.edges.push(TreeEdge {
                    id: format!("e-{pred}-{succ}"),
                    source: pred.clone(),
                    target: succ.clone(),
                    kind: "chapter".into(),
                    source_handle: sh.clone().or_else(|| Some("bottom".into())),
                    target_handle: th.clone().or_else(|| Some("top".into())),
                    label: String::new(),
                });
            }
        }
        let _ = fs::remove_file(chapter_path(&novel_id, &node_id));
        let _ = state.db.delete_chapter_memory(&novel_id, &node_id);
        let nid = novel_id.clone();
        let cid = node_id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = chapter_memory::delete_lance(&nid, &cid).await;
        });
    }

    tree.nodes.retain(|n| n.id != node_id);
    tree.edges
        .retain(|e| e.source != node_id && e.target != node_id);
    let alive: std::collections::HashSet<String> =
        tree.nodes.iter().map(|n| n.id.clone()).collect();
    tree.edges
        .retain(|e| alive.contains(&e.source) && alive.contains(&e.target));
    for n in &mut tree.nodes {
        n.linked_character_ids.retain(|id| alive.contains(id));
        n.linked_side_plot_ids.retain(|id| alive.contains(id));
        n.linked_knowledge_ids.retain(|id| alive.contains(id));
    }
    let _ = state.db.delete_chat_for_node(&novel_id, &node_id);
    save_tree(tree.clone())?;
    Ok(tree)
}

#[tauri::command]
pub fn get_chapter(novel_id: String, node_id: String) -> Result<String, String> {
    let p = chapter_path(&novel_id, &node_id);
    if p.exists() {
        fs::read_to_string(p).map_err(|e| e.to_string())
    } else {
        Ok(String::new())
    }
}

/// 手动保存章节/支线正文 Markdown；更新树上字数。空内容则清空文件。
#[tauri::command]
pub fn save_chapter(
    novel_id: String,
    node_id: String,
    content: String,
) -> Result<u32, String> {
    let mut tree = get_tree(novel_id.clone())?;
    let kind_ok = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| matches!(n.kind, NodeKind::Chapter | NodeKind::SidePlot))
        .unwrap_or(false);
    if !kind_ok {
        return Err("只能保存章节或支线剧情卡的正文".into());
    }
    let body = content.replace("\r\n", "\n");
    let words = count_words(body.trim());
    let p = chapter_path(&novel_id, &node_id);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if body.trim().is_empty() {
        if p.exists() {
            fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    } else {
        // 末尾保留一个换行，便于 diff / 再次编辑
        let mut out = body.trim_end().to_string();
        out.push('\n');
        fs::write(&p, out).map_err(|e| e.to_string())?;
    }
    set_node_word_count(&mut tree, &node_id, words);
    save_tree(tree)?;
    Ok(words)
}

/// 本章记忆条目（整体情节 + 要点），按写入顺序。
#[tauri::command]
pub fn get_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<String>, String> {
    let rows = state
        .db
        .list_chapter_memory_for_nodes(&novel_id, &[node_id])
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(_, c)| c).collect())
}

/// 全书章节记忆：按结构树章节顺序分组（仅含已有记忆的章）。
#[tauri::command]
pub fn list_all_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<Vec<ChapterMemoryGroup>, String> {
    let tree = get_tree(novel_id.clone())?;
    let chapters = sorted_chapter_nodes(&tree);
    let rows = state
        .db
        .list_chapter_memory(&novel_id)
        .map_err(|e| e.to_string())?;
    let mut by_node: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (nid, content) in rows {
        by_node.entry(nid).or_default().push(content);
    }
    let mut out = Vec::new();
    for ch in &chapters {
        let Some(items) = by_node.remove(&ch.id) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        out.push(ChapterMemoryGroup {
            node_id: ch.id.clone(),
            label: ch.label.clone(),
            items,
        });
    }
    // 树上已删但仍有记忆残留的节点
    for (nid, items) in by_node {
        if items.is_empty() {
            continue;
        }
        out.push(ChapterMemoryGroup {
            node_id: nid.clone(),
            label: nid,
            items,
        });
    }
    Ok(out)
}

/// 手动覆盖/清空某章记忆（`items` 为空则清除）。同步 SQLite + Lance。
#[tauri::command]
pub async fn set_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    items: Vec<String>,
) -> Result<(), String> {
    if items.is_empty() {
        state
            .db
            .delete_chapter_memory(&novel_id, &node_id)
            .map_err(|e| e.to_string())?;
        chapter_memory::delete_lance(&novel_id, &node_id)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        state
            .db
            .replace_chapter_memory(&novel_id, &node_id, &items)
            .map_err(|e| e.to_string())?;
        chapter_memory::upsert_lance(&novel_id, &node_id, &items)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 汇总本章大纲 + 树上链接的人物卡 / 剧情卡 / 知识卡（edges 与 linked_* 并集）+ 相关人物关系。
fn chapter_context(
    tree: &NovelTree,
    novel_id: &str,
    node_id: &str,
    loc: PromptLocale,
) -> (String, String) {
    let labels = prompts::chapter_context_labels(loc);
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let outline = node.map(|n| n.outline.clone()).unwrap_or_default();
    let label = node.map(|n| n.label.clone()).unwrap_or_default();

    let mut char_ids: Vec<String> = node
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    let mut plot_ids: Vec<String> = node
        .map(|n| n.linked_side_plot_ids.clone())
        .unwrap_or_default();
    let mut knowledge_ids: Vec<String> = node
        .map(|n| n.linked_knowledge_ids.clone())
        .unwrap_or_default();

    for e in &tree.edges {
        if e.source != node_id && e.target != node_id {
            continue;
        }
        let other_id = if e.source == node_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
            continue;
        };
        match other.kind {
            NodeKind::Character => {
                if !char_ids.iter().any(|id| id == other_id) {
                    char_ids.push(other_id.to_string());
                }
            }
            NodeKind::SidePlot => {
                if !plot_ids.iter().any(|id| id == other_id) {
                    plot_ids.push(other_id.to_string());
                }
            }
            NodeKind::Knowledge => {
                if !knowledge_ids.iter().any(|id| id == other_id) {
                    knowledge_ids.push(other_id.to_string());
                }
            }
            _ => {}
        }
    }

    // 根节点关联人物 / 剧情 / 知识卡：贯穿全书
    let mut root_char_ids: Vec<String> = Vec::new();
    let mut root_plot_ids: Vec<String> = Vec::new();
    if let Some(root) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        for cid in &root.linked_character_ids {
            if !root_char_ids.iter().any(|id| id == cid) {
                root_char_ids.push(cid.clone());
            }
            if !char_ids.iter().any(|id| id == cid) {
                char_ids.push(cid.clone());
            }
        }
        for pid in &root.linked_side_plot_ids {
            if !root_plot_ids.iter().any(|id| id == pid) {
                root_plot_ids.push(pid.clone());
            }
            if !plot_ids.iter().any(|id| id == pid) {
                plot_ids.push(pid.clone());
            }
        }
        for kid in &root.linked_knowledge_ids {
            if !knowledge_ids.iter().any(|id| id == kid) {
                knowledge_ids.push(kid.clone());
            }
        }
        for e in &tree.edges {
            if e.source != root.id && e.target != root.id {
                continue;
            }
            let other_id = if e.source == root.id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character) {
                if !root_char_ids.iter().any(|id| id == other_id) {
                    root_char_ids.push(other_id.to_string());
                }
                if !char_ids.iter().any(|id| id == other_id) {
                    char_ids.push(other_id.to_string());
                }
            }
            if matches!(other.kind, NodeKind::SidePlot) {
                if !root_plot_ids.iter().any(|id| id == other_id) {
                    root_plot_ids.push(other_id.to_string());
                }
                if !plot_ids.iter().any(|id| id == other_id) {
                    plot_ids.push(other_id.to_string());
                }
            }
            if matches!(other.kind, NodeKind::Knowledge)
                && !knowledge_ids.iter().any(|id| id == other_id)
            {
                knowledge_ids.push(other_id.to_string());
            }
        }
    }

    // 剧情卡上链接的人物也纳入本章上下文
    for pid in plot_ids.clone() {
        if let Some(plot) = tree.nodes.iter().find(|n| n.id == pid) {
            for cid in &plot.linked_character_ids {
                if !char_ids.iter().any(|id| id == cid) {
                    char_ids.push(cid.clone());
                }
            }
        }
        for e in &tree.edges {
            if e.source != pid && e.target != pid {
                continue;
            }
            let other_id = if e.source == pid {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character)
                && !char_ids.iter().any(|id| id == other_id)
            {
                char_ids.push(other_id.to_string());
            }
        }
    }

    // 本章人物若与另一人物有关系线，把对方一并纳入上下文
    let seed = char_ids.clone();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        let a_in = seed.iter().any(|id| id == &e.source);
        let b_in = seed.iter().any(|id| id == &e.target);
        if !a_in && !b_in {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) {
            char_ids.push(e.source.clone());
        }
        if !char_ids.iter().any(|id| id == &e.target) {
            char_ids.push(e.target.clone());
        }
    }

    let mut characters = String::new();
    for id in &char_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let bookwide = if root_char_ids.iter().any(|r| r == id) {
                if loc.is_zh() {
                    "｜全书人物"
                } else {
                    "｜book-wide"
                }
            } else {
                ""
            };
            if let Some(c) = &n.character {
                characters.push_str(&format!(
                    "- {}：{}{}｜{}：{}｜{}：{}｜{}：{}\n  {}：{}\n  {}：{}\n  {}：{}\n",
                    labels.name,
                    n.label,
                    bookwide,
                    labels.role,
                    c.role,
                    labels.gender,
                    c.gender,
                    labels.alignment,
                    c.alignment,
                    labels.personality,
                    c.personality,
                    labels.style,
                    c.style,
                    labels.motto,
                    c.motto
                ));
            } else {
                characters.push_str(&format!(
                    "- {}：{}{} {}\n",
                    labels.name, n.label, bookwide, labels.card_incomplete
                ));
            }
        }
    }
    if characters.is_empty() {
        characters.push_str(labels.no_characters);
    }

    let mut relations = String::new();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) || !char_ids.iter().any(|id| id == &e.target)
        {
            continue;
        }
        let rel = if e.label.trim().is_empty() {
            labels.relation_unset
        } else {
            e.label.as_str()
        };
        relations.push_str(&format!("- {} ↔ {}：{}\n", a.label, b.label, rel));
    }

    let mut plots = String::new();
    for id in &plot_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            if !plot_is_injectable(n) {
                continue;
            }
            let outline_text = if n.outline.trim().is_empty() {
                labels.empty_plot
            } else {
                n.outline.as_str()
            };
            let bookwide = if root_plot_ids.iter().any(|r| r == id) {
                if loc.is_zh() {
                    "｜全书剧情·进行中"
                } else {
                    "｜book-wide · active"
                }
            } else {
                ""
            };
            let mut cast: Vec<String> = n
                .linked_character_ids
                .iter()
                .filter_map(|cid| {
                    tree.nodes
                        .iter()
                        .find(|x| x.id == *cid)
                        .map(|x| x.label.clone())
                })
                .collect();
            for e in &tree.edges {
                if e.source != *id && e.target != *id {
                    continue;
                }
                let other_id = if e.source == *id {
                    e.target.as_str()
                } else {
                    e.source.as_str()
                };
                if let Some(other) = tree.nodes.iter().find(|x| x.id == other_id) {
                    if matches!(other.kind, NodeKind::Character)
                        && !cast.iter().any(|l| l == &other.label)
                    {
                        cast.push(other.label.clone());
                    }
                }
            }
            let cast_text = if cast.is_empty() {
                if loc.is_zh() {
                    "（未标）".to_string()
                } else {
                    "(none)".to_string()
                }
            } else {
                cast.join("、")
            };
            // ponytail: cap plot body so one long POV doesn't blow the prompt
            let body = get_chapter(novel_id.to_string(), id.clone()).unwrap_or_default();
            let body_snip: String = body.chars().take(2000).collect();
            if loc.is_zh() {
                plots.push_str(&format!(
                    "- 「{}」{bookwide}\n  剧情要点：{outline_text}\n  关联人物：{cast_text}\n",
                    n.label
                ));
                if !body_snip.trim().is_empty() {
                    plots.push_str(&format!("  剧情正文/补充：\n{body_snip}\n"));
                }
            } else {
                plots.push_str(&format!(
                    "- “{}”{bookwide}\n  Beats: {outline_text}\n  Cast: {cast_text}\n",
                    n.label
                ));
                if !body_snip.trim().is_empty() {
                    plots.push_str(&format!("  Plot body / notes:\n{body_snip}\n"));
                }
            }
        }
    }
    if plots.is_empty() {
        plots.push_str(labels.no_plots);
    }

    let mut kn_cards: Vec<(String, String)> = Vec::new();
    for id in &knowledge_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let k = n.knowledge.as_ref();
            let feat = k.map(|x| x.extracted.as_str()).unwrap_or("").trim();
            if !feat.is_empty() {
                kn_cards.push((n.label.clone(), feat.to_string()));
            } else {
                kn_cards.push((n.label.clone(), labels.empty_knowledge.to_string()));
            }
        }
    }
    let knowledge = crate::kb_context::format_knowledge_cards_capped(
        &kn_cards,
        crate::kb_context::KNOWLEDGE_CARD_EXTRACT_CAP,
        crate::kb_context::CHAPTER_KNOWLEDGE_TOTAL_CAP,
        loc.is_zh(),
    );
    let knowledge = if knowledge.is_empty() {
        labels.no_knowledge.to_string()
    } else {
        knowledge
    };

    let outline_text = if outline.trim().is_empty() {
        labels.empty_outline.to_string()
    } else {
        outline
    };

    let chapter_info = format!(
        "{}：{label}\n{}：\n{outline_text}",
        labels.chapter_title, labels.chapter_outline
    );
    let cards = if relations.is_empty() {
        format!(
            "{}\n{characters}\n{}\n{plots}\n{}\n{knowledge}",
            labels.chars_header, labels.plots_header, labels.knowledge_header
        )
    } else {
        format!(
            "{}\n{characters}\n{}\n{relations}\n{}\n{plots}\n{}\n{knowledge}",
            labels.chars_header,
            labels.relations_header,
            labels.plots_header,
            labels.knowledge_header
        )
    };
    (chapter_info, cards)
}

/// 预生成用：根节点故事简介 + 根节点关联人物卡。
fn root_generate_reference(tree: &NovelTree, synopsis: &str, loc: PromptLocale) -> String {
    let labels = prompts::chapter_context_labels(loc);
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel));
    let root_outline = root.map(|n| n.outline.as_str()).unwrap_or("");
    let root_id = root.map(|n| n.id.as_str()).unwrap_or("");

    let mut char_ids: Vec<String> = root
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    if !root_id.is_empty() {
        for e in &tree.edges {
            if e.source != root_id && e.target != root_id {
                continue;
            }
            let other_id = if e.source == root_id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character)
                && !char_ids.iter().any(|id| id == other_id)
            {
                char_ids.push(other_id.to_string());
            }
        }
    }

    let mut characters = String::new();
    for id in &char_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            if let Some(c) = &n.character {
                characters.push_str(&format!(
                    "- {}：{}｜{}：{}｜{}：{}｜{}：{}\n  {}：{}\n  {}：{}\n  {}：{}\n",
                    labels.name,
                    n.label,
                    labels.role,
                    c.role,
                    labels.gender,
                    c.gender,
                    labels.alignment,
                    c.alignment,
                    labels.personality,
                    c.personality,
                    labels.style,
                    c.style,
                    labels.motto,
                    c.motto
                ));
            } else {
                characters.push_str(&format!(
                    "- {}：{} {}\n",
                    labels.name, n.label, labels.card_incomplete
                ));
            }
        }
    }
    if characters.is_empty() {
        characters.push_str(if loc.is_zh() {
            "（根节点未关联人物卡）\n"
        } else {
            "(no characters linked to root)\n"
        });
    }

    let syn = if synopsis.trim().is_empty() {
        if loc.is_zh() {
            "（暂无简介）"
        } else {
            "(no synopsis)"
        }
    } else {
        synopsis
    };
    // Initial trees copy synopsis into root.outline — skip duplicate "notes".
    let extra = if root_outline.trim().is_empty() || root_outline.trim() == synopsis.trim()
    {
        String::new()
    } else if loc.is_zh() {
        format!("根节点补充说明：\n{root_outline}\n")
    } else {
        format!("Root notes:\n{root_outline}\n")
    };

    if loc.is_zh() {
        format!("【全书故事简介】\n{syn}\n{extra}\n【根节点关联人物卡（参考）】\n{characters}")
    } else {
        format!("[Novel synopsis]\n{syn}\n{extra}\n[Root-linked character cards (reference)]\n{characters}")
    }
}

/// Extract core chapter facts into this novel’s isolated memory store (SQLite + Lance).
async fn extract_and_store_chapter_memory(
    app: Option<&tauri::AppHandle>,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel_id: &str,
    node_id: &str,
    label: &str,
    outline: &str,
    body: &str,
    user_notes: &str,
    other_memory: &str,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<String>, String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(if loc.is_zh() {
            "本章尚无正文，无法抽取记忆".into()
        } else {
            "No chapter body to extract memory from".into()
        });
    }
    let model = task_model(&settings.knowledge_model, &settings.default_model);
    let system = prompts::extract_chapter_memory_system(loc);
    let user = prompts::extract_chapter_memory_user(
        loc,
        label,
        outline,
        body,
        user_notes,
        other_memory,
    );
    let (raw, used_mock) =
        llm_complete(app, state, settings, &system, &user, Some(&model), Some(novel_id), cancel)
            .await?;
    if used_mock {
        return Err(if loc.is_zh() {
            "当前为模拟模式，未写入记忆".into()
        } else {
            "Mock mode: memory was not written".into()
        });
    }
    let chunks = chapter_memory::split_memory_text(&raw);
    if chunks.is_empty() {
        return Err(if loc.is_zh() {
            "未能从正文中抽取出记忆".into()
        } else {
            "No memory chunks extracted from body".into()
        });
    }
    state
        .db
        .replace_chapter_memory(novel_id, node_id, &chunks)
        .map_err(|e| e.to_string())?;
    chapter_memory::upsert_lance(novel_id, node_id, &chunks)
        .await
        .map_err(|e| e.to_string())?;
    Ok(chunks)
}

/// 组装勾选章节的已有记忆（供抽取对照）。
fn build_extract_other_memory(
    state: &State<'_, AppState>,
    novel_id: &str,
    tree: &NovelTree,
    memory_node_ids: &[String],
    except_node_id: &str,
    loc: PromptLocale,
) -> String {
    let mut ids: Vec<String> = memory_node_ids
        .iter()
        .filter(|id| id.as_str() != except_node_id)
        .cloned()
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return String::new();
    }
    let mut by_node: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    if let Ok(rows) = state.db.list_chapter_memory_for_nodes(novel_id, &ids) {
        for (nid, content) in rows {
            by_node.entry(nid).or_default().push(content);
        }
    }
    let label_of = |id: &str| -> String {
        tree.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let outline_of = |id: &str| -> String {
        tree.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.outline.clone())
            .unwrap_or_default()
    };
    let mut out = String::new();
    for id in &ids {
        let facts = by_node
            .get(id)
            .map(|v| {
                v.iter()
                    .map(|f| format!("- {f}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if facts.trim().is_empty() {
            continue;
        }
        out.push_str(&prompts::prev_chapter_memory_block(
            loc,
            &label_of(id),
            &outline_of(id),
            &facts,
        ));
    }
    out
}

/// 手动按当前正文重新抽取并覆盖本章记忆。
/// `user_notes`：抽取注意事项（可空）；`memory_node_ids`：勾选的其他章，注入其记忆作对照。
#[tauri::command]
pub async fn regenerate_chapter_memory(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    user_notes: String,
    memory_node_ids: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 2;
    emit_chapter_progress(&app, &novel_id, "memory", 1, TOTAL);

    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节可重新生成记忆".into());
    }
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node.outline.as_str(),
            user_notes.as_str(),
        ],
    );
    let body = get_chapter(novel_id.clone(), node_id.clone())?;
    let ids = memory_node_ids.unwrap_or_default();
    let other_memory =
        build_extract_other_memory(&state, &novel_id, &tree, &ids, &node_id, loc);
    let chunks = extract_and_store_chapter_memory(
        Some(&app),
        &state,
        &settings,
        &novel_id,
        &node_id,
        &node.label,
        &node.outline,
        &body,
        &user_notes,
        &other_memory,
        loc,
        Some(cancel),
    )
    .await?;
    emit_chapter_progress(&app, &novel_id, "saving", 2, TOTAL);
    Ok(chunks)
}

/// 预生成硬验收：大纲节拍 + 本章/根节点剧情卡 + 本章焦点人物（不含仅挂在根上的路人）。
fn collect_land_beats(tree: &NovelTree, novel_id: &str, node_id: &str) -> Vec<LandBeat> {
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let mut beats = Vec::new();
    let outline = node.map(|n| n.outline.as_str()).unwrap_or("");
    for (i, text) in chapter_constraints::outline_beats(outline)
        .into_iter()
        .enumerate()
    {
        beats.push(LandBeat {
            kind: "outline",
            title: format!("§{}", i + 1),
            text,
        });
    }

    let mut plot_ids: Vec<String> = node
        .map(|n| n.linked_side_plot_ids.clone())
        .unwrap_or_default();
    let mut focus_chars: Vec<String> = node
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    for e in &tree.edges {
        if e.source != node_id && e.target != node_id {
            continue;
        }
        let other_id = if e.source == node_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
            continue;
        };
        match other.kind {
            NodeKind::SidePlot if !plot_ids.iter().any(|id| id == other_id) => {
                plot_ids.push(other_id.to_string());
            }
            NodeKind::Character if !focus_chars.iter().any(|id| id == other_id) => {
                focus_chars.push(other_id.to_string());
            }
            _ => {}
        }
    }
    if let Some(root) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        for pid in &root.linked_side_plot_ids {
            if !plot_ids.iter().any(|id| id == pid) {
                plot_ids.push(pid.clone());
            }
        }
        for e in &tree.edges {
            if e.source != root.id && e.target != root.id {
                continue;
            }
            let other_id = if e.source == root.id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            if let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) {
                if matches!(other.kind, NodeKind::SidePlot)
                    && !plot_ids.iter().any(|id| id == other_id)
                {
                    plot_ids.push(other_id.to_string());
                }
            }
        }
    }
    for pid in &plot_ids {
        if let Some(p) = tree.nodes.iter().find(|n| n.id == *pid) {
            if !plot_is_injectable(p) {
                continue;
            }
            for cid in &p.linked_character_ids {
                if !focus_chars.iter().any(|id| id == cid) {
                    focus_chars.push(cid.clone());
                }
            }
            let body = get_chapter(novel_id.to_string(), pid.clone()).unwrap_or_default();
            let body_snip: String = body.chars().take(400).collect();
            let text = if p.outline.trim().is_empty() {
                body_snip
            } else if body_snip.trim().is_empty() {
                p.outline.clone()
            } else {
                format!("{}\n{}", p.outline, body_snip)
            };
            if text.trim().chars().count() >= 4 {
                beats.push(LandBeat {
                    kind: "plot",
                    title: p.label.clone(),
                    text: text.chars().take(280).collect(),
                });
            }
        }
    }
    for cid in &focus_chars {
        if let Some(c) = tree.nodes.iter().find(|n| n.id == *cid) {
            let hint = c
                .character
                .as_ref()
                .map(|x| format!("{}｜{}", x.role, x.personality))
                .unwrap_or_default();
            beats.push(LandBeat {
                kind: "character",
                title: c.label.clone(),
                text: hint.chars().take(80).collect(),
            });
        }
    }
    beats
}

/// Sorted chapter nodes (by canvas y, then x).
fn sorted_chapter_nodes(tree: &NovelTree) -> Vec<TreeNode> {
    let mut chapters: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .cloned()
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.position
                    .x
                    .partial_cmp(&b.position.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    chapters
}

fn outline_mode_from_json(v: &serde_json::Value) -> Option<ChapterResolveMode> {
    let s = v
        .get("mode")
        .or_else(|| v.get("resolve"))
        .and_then(|x| x.as_str())?
        .trim()
        .to_ascii_lowercase();
    match s.as_str() {
        "overwrite" | "replace" | "覆盖" | "重写" | "重新生成" | "upsert" => {
            Some(ChapterResolveMode::Overwrite)
        }
        "append" | "force" | "forceappend" | "强制追加" | "追加" => {
            Some(ChapterResolveMode::ForceAppend)
        }
        "skip" | "跳过" => Some(ChapterResolveMode::Skip),
        _ => None,
    }
}

fn build_refine_memory_context(
    state: &State<'_, AppState>,
    novel_id: &str,
    prev_chapters: &[TreeNode],
    query: &str,
    loc: PromptLocale,
) -> String {
    // ponytail: top-k within picks; raise MEMORY_* when writers need denser recall
    let node_ids: Vec<String> = prev_chapters.iter().map(|c| c.id.clone()).collect();
    let mut facts: Vec<(String, String)> = Vec::new(); // (memory:node_id, content)
    if let Ok(rows) = state.db.list_chapter_memory_for_nodes(novel_id, &node_ids) {
        for (nid, content) in rows {
            facts.push((format!("memory:{nid}"), content));
        }
    }
    let ranked = chapter_memory::rank_memory(&facts, query, crate::kb_context::MEMORY_RETRIEVE_K);
    let mut fact_lines = Vec::new();
    let mut used = 0usize;
    for (sid, content) in ranked {
        let snip = crate::kb_context::truncate_chars(&content, crate::kb_context::MEMORY_FACT_CAP);
        let line = format!("- [{sid}] {snip}");
        let add = line.chars().count() + 1;
        if used + add > crate::kb_context::MEMORY_BLOCK_CAP {
            break;
        }
        used += add;
        fact_lines.push(line);
    }

    let mut out = String::new();
    // Compact outlines for selected chapters (facts come from top-k block below)
    for c in prev_chapters {
        let outline: String = c.outline.chars().take(200).collect();
        if loc.is_zh() {
            out.push_str(&format!("## {}\n大纲：{outline}\n\n", c.label));
        } else {
            out.push_str(&format!("## {}\nOutline: {outline}\n\n", c.label));
        }
    }
    if !fact_lines.is_empty() {
        let block = fact_lines.join("\n");
        if loc.is_zh() {
            out.push_str(&format!("## 相关章节记忆（检索 top-k）\n{block}\n\n"));
        } else {
            out.push_str(&format!("## Related chapter memory (retrieved top-k)\n{block}\n\n"));
        }
    }
    out
}

/// 预生成确认：当前章之前的章节列表（含是否已有记忆）。
#[tauri::command]
pub fn preview_generate_chapter(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<GenerateMemoryPick>, String> {
    let tree = get_tree(novel_id.clone())?;
    let chapters = sorted_chapter_nodes(&tree);
    let idx = chapters
        .iter()
        .position(|c| c.id == node_id)
        .ok_or_else(|| "当前节点不是章节卡".to_string())?;
    let prior = &chapters[..idx];
    let ids: Vec<String> = prior.iter().map(|c| c.id.clone()).collect();
    let mut has_mem: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !ids.is_empty() {
        if let Ok(rows) = state.db.list_chapter_memory_for_nodes(&novel_id, &ids) {
            for (nid, _) in rows {
                has_mem.insert(nid);
            }
        }
    }
    Ok(prior
        .iter()
        .map(|c| GenerateMemoryPick {
            node_id: c.id.clone(),
            label: c.label.clone(),
            has_memory: has_mem.contains(&c.id),
        })
        .collect())
}

#[tauri::command]
pub async fn generate_chapter(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    memory_node_ids: Vec<String>,
    user_brief: String,
    model: Option<String>,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 6;
    emit_chapter_progress(&app, &novel_id, "context", 1, TOTAL);

    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    // Chat 面板所选模型；非空时覆盖本轮 generate_model。
    apply_chat_model_override(&mut settings.generate_model, model.as_deref());
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let node_outline = node.map(|n| n.outline.as_str()).unwrap_or("");
    let node_label = node.map(|n| n.label.as_str()).unwrap_or("");
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node_outline,
            user_brief.as_str(),
        ],
    );
    let root_ref = root_generate_reference(&tree, &novel.synopsis, loc);
    let (chapter_info, cards) = chapter_context(&tree, &novel_id, &node_id, loc);
    let land_beats = collect_land_beats(&tree, &novel_id, &node_id);
    let contract = chapter_constraints::format_contract(loc.is_zh(), &land_beats);
    let chapters = sorted_chapter_nodes(&tree);
    // 预生成禁止参考本章已有正文：不读本章 md；记忆勾选亦排除本章（仅前序）。
    let selected: std::collections::HashSet<&str> = memory_node_ids
        .iter()
        .map(|s| s.as_str())
        .filter(|id| *id != node_id.as_str())
        .collect();
    let prior_hist = if let Some(i) = chapters.iter().position(|c| c.id == node_id) {
        let prev: Vec<TreeNode> = chapters[..i]
            .iter()
            .filter(|c| selected.contains(c.id.as_str()))
            .cloned()
            .collect();
        if prev.is_empty() {
            String::new()
        } else {
            let query = format!("{node_label}\n{node_outline}");
            build_refine_memory_context(&state, &novel_id, &prev, &query, loc)
        }
    } else {
        String::new()
    };
    let (wmin, wmax) = root_chapter_word_target(&tree, &novel);
    let query = format!("{node_label}\n{node_outline}\n{user_brief}");
    let canon_block = build_canon_context(&state, &novel, &tree, &query, loc);
    let system = prompts::generate_chapter_system(
        loc,
        &novel.title,
        &novel.synopsis,
        &novel.knowledge_strategy,
        wmin,
        wmax,
        novel.chapter_count,
        &novel.canon_mode,
    );
    let user = prompts::generate_chapter_user(
        loc,
        &root_ref,
        &prior_hist,
        &canon_block,
        &chapter_info,
        &cards,
        &contract,
        &user_brief,
        wmin,
        wmax,
    );
    let model = task_model(&settings.generate_model, &settings.default_model);
    emit_chapter_progress(&app, &novel_id, "writing", 2, TOTAL);
    let (mut content, used_mock) = llm_complete(
        Some(&app),
        &state,
        &settings,
        &system,
        &user,
        Some(&model),
        Some(&novel_id),
        Some(cancel.clone()),
    )
    .await?;

    // 落地校验拆成子步骤进度，避免长时间停在「校验落地节拍」不知在干嘛。
    // 字数只靠首轮 Prompt 交给 AI；落地/设定对齐轮不做字数验证。
    let mut repaired_n = 0usize;
    emit_chapter_progress(&app, &novel_id, "check_beats", 3, TOTAL);
    if !used_mock && !land_beats.is_empty() {
        let missing = chapter_constraints::missing_beats(&content, &land_beats);
        if !missing.is_empty() {
            repaired_n = missing.len();
            emit_chapter_progress(&app, &novel_id, "repair_land", 4, TOTAL);
            let miss_txt = chapter_constraints::format_missing(loc.is_zh(), &missing);
            let rsys = prompts::repair_chapter_system(loc);
            let ruser = prompts::repair_chapter_user(loc, &miss_txt, &content);
            match llm_complete(
                Some(&app),
                &state,
                &settings,
                &rsys,
                &ruser,
                Some(&model),
                Some(&novel_id),
                Some(cancel.clone()),
            )
            .await
            {
                Ok((fixed, mock2)) => {
                    if !mock2 && fixed.trim().chars().count() > content.trim().chars().count() / 2 {
                        content = fixed;
                    } else {
                        repaired_n = 0;
                    }
                }
                Err(e) if e == "cancelled" => return Err(e),
                Err(_) => repaired_n = 0,
            }
        }
    }

    // Phase 3: strict canon alignment when bound books present
    let mut canon_repaired = 0usize;
    if !used_mock && novel_canon_strict(&novel) && !canon_block.trim().is_empty() {
        emit_chapter_progress(&app, &novel_id, "check_canon", 5, TOTAL);
        let canon_beats = collect_canon_land_beats(&canon_block, node_outline);
        let missing = chapter_constraints::missing_beats(&content, &canon_beats);
        if !missing.is_empty() {
            canon_repaired = missing.len();
            emit_chapter_progress(&app, &novel_id, "repair_canon", 5, TOTAL);
            let miss_txt = chapter_constraints::format_missing(loc.is_zh(), &missing);
            // P3: only pass snippets that mention the missing terms (not full canon dump)
            let slim_canon = canon_snippets_for_missing(&canon_block, &missing);
            let rsys = prompts::repair_canon_system(loc);
            let ruser = prompts::repair_canon_user(loc, &slim_canon, &miss_txt, &content);
            match llm_complete(
                Some(&app),
                &state,
                &settings,
                &rsys,
                &ruser,
                Some(&model),
                Some(&novel_id),
                Some(cancel.clone()),
            )
            .await
            {
                Ok((fixed, mock2)) => {
                    if !mock2 && fixed.trim().chars().count() > content.trim().chars().count() / 2 {
                        content = fixed;
                    } else {
                        canon_repaired = 0;
                    }
                }
                Err(e) if e == "cancelled" => return Err(e),
                Err(_) => canon_repaired = 0,
            }
        }
    }

    emit_chapter_progress(&app, &novel_id, "saving", 6, TOTAL);
    let words = count_words(&content);
    content.push_str(&prompts::generate_footer(loc, &node_id, &model));
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    // 预生成不抽取章节记忆；记忆仅手动抽取。
    let mut message = prompts::generate_done_msg(loc, &model, words, used_mock, repaired_n);
    if canon_repaired > 0 {
        if loc.is_zh() {
            message.push_str(&format!(" 设定对齐补写 {canon_repaired} 项。"));
        } else {
            message.push_str(&format!(" Canon-aligned {canon_repaired} term(s)."));
        }
    }
    if !word_count_ok(words, wmin, wmax) {
        message.push_str(&prompts::word_count_off_note(loc, words, wmin, wmax));
    }
    Ok(GenerateResult {
        content,
        used_mock,
        message,
    })
}

/// `mode`: `memory` = 章节记忆+大纲；`full` = 前序章完整正文。
#[tauri::command]
pub async fn refine_chapter(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    prev_n: u32,
    mode: Option<String>,
    user_brief: Option<String>,
    model: Option<String>,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 2;
    emit_chapter_progress(&app, &novel_id, "context", 1, TOTAL);

    let full_body = matches!(
        mode.as_deref().unwrap_or("memory").to_ascii_lowercase().as_str(),
        "full" | "body" | "text"
    );
    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    // Chat 面板所选模型；非空时覆盖本轮 refine_model。
    apply_chat_model_override(&mut settings.refine_model, model.as_deref());
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let node_outline = node.map(|n| n.outline.as_str()).unwrap_or("");
    // 精修必须在当前已有正文上改；无正文则拒绝（勿当成预生成）。
    let current = get_chapter(novel_id.clone(), node_id.clone())?;
    if current.trim().is_empty() {
        return Err("当前章尚无正文，请先预生成再精修".into());
    }
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node_outline,
            current.as_str(),
        ],
    );
    let (chapter_info, cards) = chapter_context(&tree, &novel_id, &node_id, loc);

    // previous chapters along chapter chain order by y
    let mut chapters: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .cloned()
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let idx = chapters.iter().position(|c| c.id == node_id);
    let mut prev_text = String::new();
    let mut linked_n = 0u32;
    if let Some(i) = idx {
        let start = i.saturating_sub(prev_n as usize);
        linked_n = (i - start) as u32;
        let prev = &chapters[start..i];
        if full_body {
            for c in prev {
                let body = get_chapter(novel_id.clone(), c.id.clone()).unwrap_or_default();
                prev_text.push_str(&prompts::prev_chapter_block(
                    loc, &c.label, &c.outline, &body,
                ));
            }
        } else {
            let query =
                format!("{node_outline}\n{}", current.chars().take(1200).collect::<String>());
            prev_text = build_refine_memory_context(&state, &novel_id, prev, &query, loc);
        }
    }

    let (wmin, wmax) = root_chapter_word_target(&tree, &novel);
    let refine_model = task_model(&settings.refine_model, &settings.default_model);
    let system = prompts::refine_chapter_system(
        loc,
        &novel.title,
        linked_n,
        wmin,
        wmax,
        full_body,
    );
    let brief = user_brief.unwrap_or_default();
    let query = format!("{node_outline}\n{}", current.chars().take(800).collect::<String>());
    let canon_block = build_canon_context(&state, &novel, &tree, &query, loc);
    let strategy = if canon_block.trim().is_empty() {
        novel.knowledge_strategy.clone()
    } else {
        format!("{}\n\n{canon_block}", novel.knowledge_strategy)
    };
    let user = prompts::refine_chapter_user(
        loc,
        &strategy,
        linked_n,
        &prev_text,
        &chapter_info,
        &cards,
        &current,
        full_body,
        &brief,
        wmin,
        wmax,
    );
    emit_chapter_progress(&app, &novel_id, "refining", 2, TOTAL);
    // 字数只靠提示词约束，不再额外跑篇幅校准。
    let (content, used_mock) = llm_complete(
        Some(&app),
        &state,
        &settings,
        &system,
        &user,
        Some(&refine_model),
        Some(&novel_id),
        Some(cancel.clone()),
    )
    .await?;
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let words = count_words(&content);
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    // 精修不再自动抽取记忆；请在记忆面板手动提取。
    let mut message =
        prompts::refine_done_msg(loc, &refine_model, linked_n, words, used_mock, full_body);
    if !word_count_ok(words, wmin, wmax) {
        message.push_str(&prompts::word_count_off_note(loc, words, wmin, wmax));
    }
    Ok(GenerateResult {
        content,
        used_mock,
        message,
    })
}

/// 解析根节点「生成章节卡」的编号范围。
fn root_gen_range(novel: &NovelProject, count: u32) -> (u32, u32, Vec<u32>) {
    let mut to = count.clamp(1, 100);
    if novel.chapter_count > 0 {
        to = to.min(novel.chapter_count);
    }
    let from = 1u32;
    let nums: Vec<u32> = (from..=to).collect();
    (from, to, nums)
}

/// 解析「生成下一章」的编号范围；`through_n` = 当前章号（含）。
fn plan_next_range(
    novel: &NovelProject,
    cur_n: u32,
    count: u32,
    loc: PromptLocale,
) -> Result<(u32, u32, Vec<u32>), String> {
    let from = cur_n + 1;
    if novel.chapter_count > 0 && from > novel.chapter_count {
        return Err(if loc.is_zh() {
            "已是计划中的最后一章，无法再生成下一章大纲。".into()
        } else {
            "This is the last planned chapter; nothing left to outline.".into()
        });
    }
    let want = count.clamp(1, 12);
    let to = if novel.chapter_count > 0 {
        (from + want - 1).min(novel.chapter_count)
    } else {
        from + want - 1
    };
    let nums: Vec<u32> = (from..=to).collect();
    if nums.is_empty() {
        return Err(if loc.is_zh() {
            "没有可生成的下一章编号。".into()
        } else {
            "No next chapter numbers to generate.".into()
        });
    }
    Ok((from, to, nums))
}

/// 左侧确认框：预览须考虑的材料（根大纲 → 前序大纲+记忆）与期望占位。
/// `mode`: `root` | `plan_next` | `regen_outline`；后两者需要 `node_id`。
#[tauri::command]
pub fn preview_chapter_outline_brief(
    state: State<'_, AppState>,
    novel_id: String,
    mode: String,
    count: u32,
    node_id: Option<String>,
) -> Result<OutlineGenBrief, String> {
    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let root_outline = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.outline.as_str())
        .unwrap_or("");
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            root_outline,
        ],
    );

    match mode.as_str() {
        "root" => {
            let (from, to, nums) = root_gen_range(&novel, count);
            // from=1 → before_n=1：仅根；若日后支持中段生成则 before_n=from。
            let brief = assemble_outline_gen_brief(
                &state, &novel, &tree, &novel_id, from, from, to, &nums, loc, None,
            );
            Ok(OutlineGenBrief { brief, from, to })
        }
        "plan_next" => {
            let nid = node_id.ok_or_else(|| "缺少 node_id".to_string())?;
            let chapters = sorted_chapter_nodes(&tree);
            let idx = chapters
                .iter()
                .position(|c| c.id == nid)
                .ok_or_else(|| "当前节点不是章节卡".to_string())?;
            let cur = &chapters[idx];
            let cur_n = chapter_number_from_label(&cur.label).unwrap_or((idx + 1) as u32);
            let (from, to, nums) = plan_next_range(&novel, cur_n, count, loc)?;
            let body_raw = get_chapter(novel_id.clone(), nid)?;
            let cur_body: String = body_raw.chars().take(12_000).collect();
            // before_n = from：含当前章及之前的大纲+记忆。
            let brief = assemble_outline_gen_brief(
                &state,
                &novel,
                &tree,
                &novel_id,
                from,
                from,
                to,
                &nums,
                loc,
                if cur_body.trim().is_empty() {
                    None
                } else {
                    Some(cur_body.as_str())
                },
            );
            Ok(OutlineGenBrief { brief, from, to })
        }
        "regen_outline" => {
            let nid = node_id.ok_or_else(|| "缺少 node_id".to_string())?;
            let chapters = sorted_chapter_nodes(&tree);
            let idx = chapters
                .iter()
                .position(|c| c.id == nid)
                .ok_or_else(|| "当前节点不是章节卡".to_string())?;
            let cur = &chapters[idx];
            let cur_n = chapter_number_from_label(&cur.label).unwrap_or((idx + 1) as u32);
            let nums = vec![cur_n];
            // before_n = cur_n：仅前序章；本章旧大纲另附「将被覆盖」。
            let mut brief = assemble_outline_gen_brief(
                &state, &novel, &tree, &novel_id, cur_n, cur_n, cur_n, &nums, loc, None,
            );
            append_regen_outline_extras(&mut brief, &tree, cur, loc);
            Ok(OutlineGenBrief {
                brief,
                from: cur_n,
                to: cur_n,
            })
        }
        _ => Err("mode 须为 root、plan_next 或 regen_outline".into()),
    }
}

fn regen_outline_current_section(loc: PromptLocale, label: &str, outline: &str) -> String {
    let body = if outline.trim().is_empty() {
        if loc.is_zh() {
            "（当前大纲为空）".to_string()
        } else {
            "(current outline empty)".to_string()
        }
    } else {
        outline.to_string()
    };
    if loc.is_zh() {
        format!(
            "## 当前章大纲（将被覆盖重写，可参考或删改）\n\
             章节：{label}\n{body}\n\n"
        )
    } else {
        format!(
            "## Current chapter outline (will be overwritten; editable reference)\n\
             Chapter: {label}\n{body}\n\n"
        )
    }
}

/// 本章链接的全部人物卡/剧情卡设定（含边上的链接、剧情卡上的人物）；刷新大纲硬约束。
fn chapter_bound_cards_for_outline(tree: &NovelTree, node_id: &str, loc: PromptLocale) -> String {
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let mut char_ids: Vec<String> = node
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    let mut plot_ids: Vec<String> = node
        .map(|n| n.linked_side_plot_ids.clone())
        .unwrap_or_default();

    for e in &tree.edges {
        if e.source != node_id && e.target != node_id {
            continue;
        }
        let other_id = if e.source == node_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
            continue;
        };
        match other.kind {
            NodeKind::Character => {
                if !char_ids.iter().any(|id| id == other_id) {
                    char_ids.push(other_id.to_string());
                }
            }
            NodeKind::SidePlot => {
                if !plot_ids.iter().any(|id| id == other_id) {
                    plot_ids.push(other_id.to_string());
                }
            }
            _ => {}
        }
    }

    for pid in plot_ids.clone() {
        if let Some(plot) = tree.nodes.iter().find(|n| n.id == pid) {
            for cid in &plot.linked_character_ids {
                if !char_ids.iter().any(|id| id == cid) {
                    char_ids.push(cid.clone());
                }
            }
        }
        for e in &tree.edges {
            if e.source != pid && e.target != pid {
                continue;
            }
            let other_id = if e.source == pid {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character)
                && !char_ids.iter().any(|id| id == other_id)
            {
                char_ids.push(other_id.to_string());
            }
        }
    }

    // 本章人物之间的关系线
    let seed = char_ids.clone();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        let a_in = seed.iter().any(|id| id == &e.source);
        let b_in = seed.iter().any(|id| id == &e.target);
        if !a_in && !b_in {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) {
            char_ids.push(e.source.clone());
        }
        if !char_ids.iter().any(|id| id == &e.target) {
            char_ids.push(e.target.clone());
        }
    }

    let cap = |s: &str, n: usize| crate::kb_context::truncate_chars(s, n);
    let mut chars = String::new();
    for id in &char_ids {
        let Some(n) = tree.nodes.iter().find(|x| x.id == *id) else {
            continue;
        };
        if let Some(c) = &n.character {
            if loc.is_zh() {
                chars.push_str(&format!(
                    "- 「{}」\n  角色：{}\n  性别：{}｜阵营：{}\n  性格：{}\n  行事风格：{}\n  座右铭：{}\n",
                    n.label,
                    cap(&c.role, 200),
                    cap(&c.gender, 40),
                    cap(&c.alignment, 80),
                    cap(&c.personality, 400),
                    cap(&c.style, 400),
                    cap(&c.motto, 200)
                ));
            } else {
                chars.push_str(&format!(
                    "- “{}”\n  Role: {}\n  Gender: {}｜Alignment: {}\n  Personality: {}\n  Style: {}\n  Motto: {}\n",
                    n.label,
                    cap(&c.role, 200),
                    cap(&c.gender, 40),
                    cap(&c.alignment, 80),
                    cap(&c.personality, 400),
                    cap(&c.style, 400),
                    cap(&c.motto, 200)
                ));
            }
        } else if loc.is_zh() {
            chars.push_str(&format!("- 「{}」（人物卡字段未补全，仍须保留其人）\n", n.label));
        } else {
            chars.push_str(&format!(
                "- “{}” (card incomplete; still keep this character)\n",
                n.label
            ));
        }
    }

    let mut relations = String::new();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) || !char_ids.iter().any(|id| id == &e.target)
        {
            continue;
        }
        let rel = if e.label.trim().is_empty() {
            if loc.is_zh() {
                "（关系未标注）"
            } else {
                "(unlabeled)"
            }
        } else {
            e.label.as_str()
        };
        relations.push_str(&format!("- {} ↔ {}：{}\n", a.label, b.label, rel));
    }

    let plot_status_label = |n: &TreeNode, zh: bool| -> String {
        let meta = n.side_plot.as_ref();
        let absorbed = meta.map(|m| m.absorbed).unwrap_or(false);
        let status = meta.map(|m| m.status.trim()).unwrap_or("");
        if zh {
            if absorbed {
                "已吸收（作既定事实，勿推翻；不必再推进）".into()
            } else if status.is_empty() || status.eq_ignore_ascii_case("active") {
                "进行中（新大纲须安排推进）".into()
            } else if status.eq_ignore_ascii_case("resolved") {
                "已收束（作既定事实，勿推翻）".into()
            } else if status.eq_ignore_ascii_case("deferred") {
                "搁置（勿当本章主推进，亦勿矛盾）".into()
            } else {
                format!("状态：{status}")
            }
        } else if absorbed {
            "absorbed (established fact; do not overturn; need not advance)".into()
        } else if status.is_empty() || status.eq_ignore_ascii_case("active") {
            "active (must advance in new outline)".into()
        } else if status.eq_ignore_ascii_case("resolved") {
            "resolved (established fact; do not overturn)".into()
        } else if status.eq_ignore_ascii_case("deferred") {
            "deferred (do not make primary advance; do not contradict)".into()
        } else {
            format!("status: {status}")
        }
    };

    let mut plots = String::new();
    for id in &plot_ids {
        let Some(n) = tree.nodes.iter().find(|x| x.id == *id) else {
            continue;
        };
        let status = plot_status_label(n, loc.is_zh());
        let outline = if n.outline.trim().is_empty() {
            if loc.is_zh() {
                "（大纲为空）".to_string()
            } else {
                "(empty outline)".to_string()
            }
        } else {
            cap(&n.outline, 800)
        };
        if loc.is_zh() {
            plots.push_str(&format!("- 「{}」[{status}]\n  要点：{outline}\n", n.label));
        } else {
            plots.push_str(&format!("- “{}” [{status}]\n  Beats: {outline}\n", n.label));
        }
    }

    if chars.is_empty() && plots.is_empty() {
        return String::new();
    }
    if loc.is_zh() {
        let c = if chars.is_empty() {
            "（本章未链接人物卡）\n"
        } else {
            chars.as_str()
        };
        let r = if relations.is_empty() {
            String::new()
        } else {
            format!("### 人物关系\n{relations}\n")
        };
        let p = if plots.is_empty() {
            "（本章未链接剧情卡）\n"
        } else {
            plots.as_str()
        };
        format!(
            "## 本章已链接人物与剧情（硬约束：必须严格遵守）\n\
             **人物**：言行/动机/关系必须严格符合下列人设，不得改写核心性格或让未链接人物压过链接人物。\n\
             **剧情**：必须严格落实下列各卡要点——进行中须写入大纲并推进；已收束/已吸收/搁置仅作既定事实，不得推翻或无视。\n\
             禁止另起无关主线；禁止只点名不落地。\n\
             ### 人物\n{c}\n{r}### 剧情卡\n{p}\n"
        )
    } else {
        let c = if chars.is_empty() {
            "(no chapter-linked character cards)\n"
        } else {
            chars.as_str()
        };
        let r = if relations.is_empty() {
            String::new()
        } else {
            format!("### Character relations\n{relations}\n")
        };
        let p = if plots.is_empty() {
            "(no chapter-linked plot cards)\n"
        } else {
            plots.as_str()
        };
        format!(
            "## Chapter-linked characters & plots (hard constraint: obey strictly)\n\
             **Characters**: speech/motives/relations MUST match cards below; do not rewrite core personality or let unlinked cast overshadow linked ones.\n\
             **Plots**: land every beat by status—advance active in the outline; treat resolved/absorbed/deferred as facts (do not overturn/ignore).\n\
             No unrelated main line; no name-drops without landing.\n\
             ### Characters\n{c}\n{r}### Plot cards\n{p}\n"
        )
    }
}

fn append_regen_outline_extras(
    brief: &mut String,
    tree: &NovelTree,
    cur: &TreeNode,
    loc: PromptLocale,
) {
    brief.push_str(&regen_outline_current_section(loc, &cur.label, &cur.outline));
    brief.push_str(&chapter_bound_cards_for_outline(tree, &cur.id, loc));
}

/// 左侧「生成剧情卡」：按本章大纲（可覆盖）+ 用户补充拆剧情卡并关联人物。
#[tauri::command]
pub async fn generate_chapter_plots(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    outline: String,
    user_notes: String,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 3;
    emit_chapter_progress(&app, &novel_id, "context", 1, TOTAL);

    let mut tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let chapter = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id && matches!(n.kind, NodeKind::Chapter))
        .cloned()
        .ok_or_else(|| "当前节点不是章节卡".to_string())?;
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            chapter.outline.as_str(),
            outline.as_str(),
            user_notes.as_str(),
        ],
    );
    let outline_ref = if outline.trim().is_empty() {
        None
    } else {
        Some(outline.as_str())
    };
    let (_reply, added_plots, added_chars) = enrich_plots_for_chapter_node(
        &app,
        &state,
        &settings,
        &novel,
        &novel_id,
        &mut tree,
        &node_id,
        chapter_number_from_label(&chapter.label),
        loc,
        Some(cancel),
        outline_ref,
        &user_notes,
        true,
    )
    .await?;

    if added_plots == 0 {
        return Err(if loc.is_zh() {
            "未能解析剧情卡结果，结构树未改动。请重试。".into()
        } else {
            "Could not parse plot cards; tree unchanged. Please retry.".into()
        });
    }
    let message = if loc.is_zh() {
        format!("已为「{}」新增 {added_plots} 张剧情卡，补齐人物卡 {added_chars} 张。", chapter.label)
    } else {
        format!(
            "Added {added_plots} plot card(s) and {added_chars} character(s) for “{}”.",
            chapter.label
        )
    };
    Ok(GenerateResult {
        content: String::new(),
        used_mock: false,
        message,
    })
}

fn normalize_plot_status(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "resolved" | "done" | "closed" | "已收束" | "收束" => "resolved".into(),
        "deferred" | "paused" | "hold" | "搁置" => "deferred".into(),
        _ => "active".into(),
    }
}

/// 根节点「整理剧情」：近 N 章大纲+记忆 → 生成/更新根跨章剧情卡，并可归档章内卡。
#[tauri::command]
pub async fn consolidate_plot_cards(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    chapter_window: u32,
    user_notes: String,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 3;
    emit_chapter_progress(&app, &novel_id, "context", 1, TOTAL);

    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut tree = get_tree(novel_id.clone())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[novel.title.as_str(), novel.synopsis.as_str(), user_notes.as_str()],
    );

    let chapters = sorted_chapter_nodes(&tree);
    if chapters.is_empty() {
        return Err(if loc.is_zh() {
            "尚无章节卡，无法整理剧情".into()
        } else {
            "No chapter cards to consolidate".into()
        });
    }
    let window = chapter_window.clamp(1, 30) as usize;
    let start = chapters.len().saturating_sub(window);
    let recent: Vec<&TreeNode> = chapters[start..].iter().collect();
    let recent_ids: Vec<String> = recent.iter().map(|c| c.id.clone()).collect();

    let mem_map = {
        let mut m = std::collections::HashMap::<String, Vec<String>>::new();
        if let Ok(rows) = state
            .db
            .list_chapter_memory_for_nodes(&novel_id, &recent_ids)
        {
            for (nid, content) in rows {
                m.entry(nid).or_default().push(content);
            }
        }
        m
    };

    let mut chapters_block = String::new();
    for c in &recent {
        let mem = mem_map
            .get(&c.id)
            .map(|v| v.join("\n"))
            .unwrap_or_default();
        let mem_snip: String = mem.chars().take(1200).collect();
        let outline_snip: String = c.outline.chars().take(800).collect();
        if loc.is_zh() {
            chapters_block.push_str(&format!(
                "### {}\nid={}\n大纲：{}\n记忆：{}\n\n",
                c.label,
                c.id,
                if outline_snip.trim().is_empty() {
                    "（空）"
                } else {
                    outline_snip.as_str()
                },
                if mem_snip.trim().is_empty() {
                    "（无）"
                } else {
                    mem_snip.as_str()
                }
            ));
        } else {
            chapters_block.push_str(&format!(
                "### {}\nid={}\nOutline: {}\nMemory: {}\n\n",
                c.label,
                c.id,
                if outline_snip.trim().is_empty() {
                    "(empty)"
                } else {
                    outline_snip.as_str()
                },
                if mem_snip.trim().is_empty() {
                    "(none)"
                } else {
                    mem_snip.as_str()
                }
            ));
        }
    }

    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .cloned()
        .ok_or_else(|| "缺少根节点".to_string())?;
    let mut root_plot_ids: Vec<String> = root.linked_side_plot_ids.clone();
    for e in &tree.edges {
        if e.source != root.id && e.target != root.id {
            continue;
        }
        let other = if e.source == root.id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        if tree
            .nodes
            .iter()
            .any(|n| n.id == other && matches!(n.kind, NodeKind::SidePlot))
            && !root_plot_ids.iter().any(|id| id == other)
        {
            root_plot_ids.push(other.to_string());
        }
    }

    let mut root_plots_block = String::new();
    if root_plot_ids.is_empty() {
        root_plots_block.push_str(if loc.is_zh() { "（无）\n" } else { "(none)\n" });
    } else {
        for id in &root_plot_ids {
            if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
                let st = n
                    .side_plot
                    .as_ref()
                    .map(|m| {
                        if m.status.trim().is_empty() {
                            "active"
                        } else {
                            m.status.as_str()
                        }
                    })
                    .unwrap_or("active");
                let abs = n.side_plot.as_ref().map(|m| m.absorbed).unwrap_or(false);
                root_plots_block.push_str(&format!(
                    "- id={} | status={st} | absorbed={abs} | 「{}」\n  {}\n",
                    n.id, n.label, n.outline
                ));
            }
        }
    }

    let recent_set: std::collections::HashSet<&str> =
        recent_ids.iter().map(|s| s.as_str()).collect();
    let mut chapter_plots_block = String::new();
    let mut chapter_plot_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for n in &tree.nodes {
        if !matches!(n.kind, NodeKind::Chapter) || !recent_set.contains(n.id.as_str()) {
            continue;
        }
        for pid in &n.linked_side_plot_ids {
            chapter_plot_ids.insert(pid.clone());
        }
    }
    for e in &tree.edges {
        let (ch, pl) = if recent_set.contains(e.source.as_str()) {
            (e.source.as_str(), e.target.as_str())
        } else if recent_set.contains(e.target.as_str()) {
            (e.target.as_str(), e.source.as_str())
        } else {
            continue;
        };
        let _ = ch;
        if tree
            .nodes
            .iter()
            .any(|n| n.id == pl && matches!(n.kind, NodeKind::SidePlot))
        {
            chapter_plot_ids.insert(pl.to_string());
        }
    }
    // 排除已在根上的
    for id in &root_plot_ids {
        chapter_plot_ids.remove(id);
    }
    if chapter_plot_ids.is_empty() {
        chapter_plots_block.push_str(if loc.is_zh() { "（无）\n" } else { "(none)\n" });
    } else {
        for id in &chapter_plot_ids {
            if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
                let abs = n.side_plot.as_ref().map(|m| m.absorbed).unwrap_or(false);
                chapter_plots_block.push_str(&format!(
                    "- id={} | absorbed={abs} | 「{}」\n  {}\n",
                    n.id, n.label, n.outline
                ));
            }
        }
    }

    emit_chapter_progress(&app, &novel_id, "writing", 2, TOTAL);
    let model = task_model(&settings.chat_model, &settings.default_model);
    let sys = prompts::consolidate_plots_system(loc);
    let user = prompts::consolidate_plots_user(
        loc,
        &novel.title,
        &novel.synopsis,
        &chapters_block,
        &root_plots_block,
        &chapter_plots_block,
        &user_notes,
    );
    let (out, used_mock) = llm_complete(
        Some(&app),
        &state,
        &settings,
        sys,
        &user,
        Some(&model),
        Some(&novel_id),
        Some(cancel),
    )
    .await?;

    let mut updated = 0u32;
    let mut created = 0u32;
    let mut absorbed = 0u32;
    let mut parsed = false;
    for v in extract_json_objects(&out) {
        let Some(root_plots) = v.get("root_plots").and_then(|x| x.as_array()) else {
            continue;
        };
        parsed = true;
        let root_pos = tree
            .nodes
            .iter()
            .find(|n| n.id == root.id)
            .map(|n| n.position.clone())
            .unwrap_or(NodePosition { x: 280.0, y: 40.0 });
        let mut root_plot_count = tree
            .nodes
            .iter()
            .filter(|n| {
                matches!(n.kind, NodeKind::SidePlot)
                    && root_plot_ids.iter().any(|id| id == &n.id)
            })
            .count();

        for p in root_plots {
            let label = p
                .get("label")
                .or_else(|| p.get("title"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let outline = p
                .get("outline")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if label.is_empty() && outline.is_empty() {
                continue;
            }
            let status = normalize_plot_status(
                p.get("status").and_then(|x| x.as_str()).unwrap_or("active"),
            );
            let id_raw = p.get("id").and_then(|x| x.as_str()).unwrap_or("").trim();
            let existing = if !id_raw.is_empty() {
                tree.nodes
                    .iter()
                    .position(|n| n.id == id_raw && matches!(n.kind, NodeKind::SidePlot))
            } else {
                None
            };

            if let Some(idx) = existing {
                let n = &mut tree.nodes[idx];
                if !label.is_empty() {
                    n.label = label;
                }
                if !outline.is_empty() {
                    n.outline = outline;
                }
                let mut meta = n.side_plot.clone().unwrap_or_default();
                meta.status = status;
                // 更新根卡时清 absorbed，保证可注入
                meta.absorbed = false;
                n.side_plot = Some(meta);
                let pid = n.id.clone();
                // ensure linked to root
                if let Some(r) = tree.nodes.iter_mut().find(|n| n.id == root.id) {
                    if !r.linked_side_plot_ids.iter().any(|id| id == &pid) {
                        r.linked_side_plot_ids.push(pid.clone());
                    }
                }
                if !tree.edges.iter().any(|e| {
                    e.kind == "side_plot"
                        && ((e.source == root.id && e.target == pid)
                            || (e.target == root.id && e.source == pid))
                }) {
                    tree.edges.push(TreeEdge {
                        id: format!("e-{}-{pid}", root.id),
                        source: root.id.clone(),
                        target: pid,
                        kind: "side_plot".into(),
                        source_handle: Some("right".into()),
                        target_handle: Some("left".into()),
                        label: String::new(),
                    });
                }
                updated += 1;
            } else {
                let pid = uuid::Uuid::new_v4().to_string();
                root_plot_count += 1;
                tree.nodes.push(TreeNode {
                    id: pid.clone(),
                    kind: NodeKind::SidePlot,
                    label: if label.is_empty() {
                        outline.chars().take(24).collect()
                    } else {
                        label
                    },
                    outline,
                    character: None,
                    knowledge: None,
                    side_plot: Some(SidePlotMeta {
                        status,
                        absorbed: false,
                    }),
                    linked_character_ids: vec![],
                    linked_side_plot_ids: vec![],
                    linked_knowledge_ids: vec![],
                    position: NodePosition {
                        x: root_pos.x + 280.0,
                        y: root_pos.y + root_plot_count as f64 * 36.0,
                    },
                    word_count: 0,
                    word_count_min: 0,
                    word_count_max: 0,
                    chapter_count: 0,
                });
                if let Some(r) = tree.nodes.iter_mut().find(|n| n.id == root.id) {
                    r.linked_side_plot_ids.push(pid.clone());
                }
                tree.edges.push(TreeEdge {
                    id: format!("e-{}-{pid}", root.id),
                    source: root.id.clone(),
                    target: pid,
                    kind: "side_plot".into(),
                    source_handle: Some("right".into()),
                    target_handle: Some("left".into()),
                    label: String::new(),
                });
                created += 1;
            }
        }

        if let Some(ids) = v.get("absorb_plot_ids").and_then(|x| x.as_array()) {
            for idv in ids {
                let Some(id) = idv.as_str() else { continue };
                if !chapter_plot_ids.contains(id) {
                    continue;
                }
                if let Some(n) = tree
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == id && matches!(n.kind, NodeKind::SidePlot))
                {
                    let mut meta = n.side_plot.clone().unwrap_or_default();
                    if !meta.absorbed {
                        absorbed += 1;
                    }
                    meta.absorbed = true;
                    if meta.status.trim().is_empty() {
                        meta.status = "deferred".into();
                    }
                    n.side_plot = Some(meta);
                }
            }
        }
        break;
    }

    if !parsed || (updated + created + absorbed == 0) {
        return Err(if loc.is_zh() {
            "未能解析整理结果，结构树未改动。请重试。".into()
        } else {
            "Could not parse consolidation result; tree unchanged. Please retry.".into()
        });
    }

    emit_chapter_progress(&app, &novel_id, "saving", 3, TOTAL);
    save_tree(tree)?;
    let message = if loc.is_zh() {
        format!("整理完成：更新 {updated} · 新建 {created} · 归档章内卡 {absorbed}。")
    } else {
        format!("Consolidated: updated {updated}, created {created}, absorbed {absorbed}.")
    };
    Ok(GenerateResult {
        content: String::new(),
        used_mock,
        message,
    })
}

/// 覆盖重写单章标题与大纲（Chat `/刷新大纲` 与兼容命令共用）。
/// `user_brief` 若已是完整 assembled brief（含材料），则原样使用；否则按刷新大纲规则组装。
async fn run_regenerate_chapter_outline(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel_id: &str,
    node_id: &str,
    user_brief: &str,
    cancel: Arc<AtomicBool>,
    use_chapter_progress: bool,
) -> Result<String, String> {
    const TOTAL: u32 = 3;
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "context", 1, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "context");
    }

    let mut tree = get_tree(novel_id.to_string())?;
    let novel = state
        .db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let chapters = sorted_chapter_nodes(&tree);
    let idx = chapters
        .iter()
        .position(|c| c.id == node_id)
        .ok_or_else(|| "当前节点不是章节卡".to_string())?;
    let cur = chapters[idx].clone();
    let cur_n = chapter_number_from_label(&cur.label).unwrap_or((idx + 1) as u32);
    let nums = vec![cur_n];
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            cur.outline.as_str(),
            cur.label.as_str(),
            user_brief,
        ],
    );

    let mut extras = String::new();
    append_regen_outline_extras(&mut extras, &tree, &cur, loc);

    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "planning", 2, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "thinking");
    }
    let (reply, mut applied) = apply_llm_chapter_cards(
        state,
        settings,
        &novel,
        novel_id,
        &mut tree,
        cur_n,
        cur_n,
        &nums,
        ChapterResolveMode::Overwrite,
        loc,
        Some(cancel),
        user_brief,
        Some(&extras),
    )
    .await?;
    // 单章刷新：以触发节点为准写回（避免章号映射/误解析写到别的章）
    if let Some(list) = extract_outlines_list(&reply) {
        if let Some(c) = list.first() {
            let label = c
                .get("label")
                .or_else(|| c.get("title"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let outline = c
                .get("outline")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(node) = tree.nodes.iter_mut().find(|x| x.id == node_id) {
                if !label.is_empty() {
                    node.label = label;
                }
                node.outline = outline;
                let _ = save_tree(tree.clone());
                applied = true;
            }
        }
    }
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "saving", 3, TOTAL);
    } else if applied {
        emit_chat_progress(app, novel_id, "apply_outlines");
        emit_chat_progress(app, novel_id, "saving");
    } else {
        emit_chat_progress(app, novel_id, "saving");
    }

    let message = if applied {
        if loc.is_zh() {
            format!("已重写第{cur_n}章标题与大纲。")
        } else {
            format!("Regenerated chapter {cur_n} title & outline.")
        }
    } else {
        let snip: String = reply.chars().take(280).collect();
        if loc.is_zh() {
            format!("未能解析 outlines JSON，结构树未改动。请重试。\n\n模型原文摘录：\n{snip}")
        } else {
            format!(
                "Could not parse outlines JSON; tree unchanged. Please retry.\n\nModel snippet:\n{snip}"
            )
        }
    };

    if !applied {
        return Err(message);
    }
    Ok(message)
}

/// 兼容入口：按「生成章节卡」同源逻辑覆盖重写本章标题与大纲。
#[tauri::command]
pub async fn regenerate_chapter_outline(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    user_brief: String,
    model: Option<String>,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let message = run_regenerate_chapter_outline(
        &app,
        &state,
        &settings,
        &novel_id,
        &node_id,
        &user_brief,
        cancel,
        true,
    )
    .await?;
    Ok(GenerateResult {
        content: String::new(),
        used_mock: false,
        message,
    })
}

/// 从根节点生成第 1–count 章章节卡（同号覆盖标题与大纲）。
#[tauri::command]
pub async fn generate_chapter_cards(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    count: u32,
    user_brief: String,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    const TOTAL: u32 = 3;
    emit_chapter_progress(&app, &novel_id, "context", 1, TOTAL);

    let mut tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let root_outline = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.outline.as_str())
        .unwrap_or("");
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            root_outline,
        ],
    );

    let (from, to, nums) = root_gen_range(&novel, count);
    let mode = ChapterResolveMode::Overwrite;

    emit_chapter_progress(&app, &novel_id, "planning", 2, TOTAL);
    let (_reply, applied) = apply_llm_chapter_cards(
        &state,
        &settings,
        &novel,
        &novel_id,
        &mut tree,
        from,
        to,
        &nums,
        mode,
        loc,
        Some(cancel),
        &user_brief,
        None,
    )
    .await?;
    emit_chapter_progress(&app, &novel_id, "saving", 3, TOTAL);

    let message = if applied {
        if loc.is_zh() {
            format!("已生成第{from}–{to}章章节卡（同号覆盖标题与大纲）。")
        } else {
            format!("Generated chapter cards {from}–{to} (overwrite title & outline by number).")
        }
    } else if loc.is_zh() {
        "未能解析 outlines JSON，结构树未改动。请重试。".into()
    } else {
        "Could not parse outlines JSON; tree unchanged. Please retry.".into()
    };

    if !applied {
        return Err(message);
    }
    Ok(GenerateResult {
        content: String::new(),
        used_mock: false,
        message,
    })
}

/// 分析当前章 + 记忆 + 根大纲/人物，生成后续若干章的剧情大纲（写入结构树）。
async fn run_plan_next_chapters(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    novel_id: &str,
    node_id: &str,
    count: u32,
    user_brief: String,
    cancel: Arc<AtomicBool>,
    use_chapter_progress: bool,
    model: Option<&str>,
) -> Result<(String, bool), String> {
    const TOTAL: u32 = 3;
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "context", 1, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "context");
    }

    let tree = get_tree(novel_id.to_string())?;
    let novel = state
        .db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model);
    let chapters = sorted_chapter_nodes(&tree);
    let idx = chapters
        .iter()
        .position(|c| c.id == node_id)
        .ok_or_else(|| "当前节点不是章节卡".to_string())?;
    let cur = &chapters[idx];
    let cur_n = chapter_number_from_label(&cur.label).unwrap_or((idx + 1) as u32);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            cur.outline.as_str(),
            cur.label.as_str(),
            user_brief.as_str(),
        ],
    );
    let (from, to, nums) = plan_next_range(&novel, cur_n, count, loc)?;

    let body_raw = get_chapter(novel_id.to_string(), node_id.to_string())?;
    let cur_body: String = body_raw.chars().take(12_000).collect();
    let assembled = assemble_outline_gen_brief(
        state,
        &novel,
        &tree,
        novel_id,
        from,
        from,
        to,
        &nums,
        loc,
        if cur_body.trim().is_empty() {
            None
        } else {
            Some(cur_body.as_str())
        },
    );
    let consideration = crate::kb_context::merge_expectation_into_assembled(
        assembled,
        &user_brief,
        loc.is_zh(),
    );

    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let system = prompts::plan_next_chapters_system(
        loc,
        &novel.title,
        &novel.synopsis,
        novel.chapter_count,
        from,
        to,
        &nums,
    );
    let user = prompts::plan_next_chapters_user(
        loc,
        from,
        to,
        &nums,
        &consideration,
        &novel.knowledge_strategy,
    );
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "planning", 2, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "thinking");
    }
    let (reply, used_mock) = llm_complete_ex(
        Some(app),
        state,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        Some(cancel.clone()),
        true,
    )
    .await?;

    let (_fixed, list) = extract_outlines_list_or_repair(
        Some(app),
        state,
        &settings,
        &chat_model,
        novel_id,
        loc,
        Some(cancel),
        &reply,
        &nums,
    )
    .await?;
    if list.is_empty() {
        return Err(if loc.is_zh() {
            "模型返回的 outlines 为空。".into()
        } else {
            "Model returned empty outlines.".into()
        });
    }

    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "saving", 3, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "apply_outlines");
        emit_chat_progress(app, novel_id, "saving");
    }
    let mut tree = get_tree(novel_id.to_string())?;
    apply_chapter_outlines(&mut tree, &list, ChapterResolveMode::Overwrite);
    let _ = save_tree(tree);

    Ok((prompts::plan_next_done_msg(loc, from, to, used_mock), used_mock))
}

#[tauri::command]
pub async fn plan_next_chapters(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    count: u32,
    user_brief: String,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    let (message, used_mock) = run_plan_next_chapters(
        &app,
        &state,
        &novel_id,
        &node_id,
        count,
        user_brief,
        cancel,
        true,
        None,
    )
    .await?;
    Ok(GenerateResult {
        content: String::new(),
        used_mock,
        message,
    })
}

#[tauri::command]
pub fn list_chat_messages(
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state.db.list_chat(&novel_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_card_chat_messages(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state
        .db
        .list_chat_for_node(&novel_id, &node_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    content: String,
    model: Option<String>,
) -> Result<Vec<ChatMessage>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };

    let now = Utc::now().to_rfc3339();
    let user_msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.clone(),
        node_id: String::new(),
        role: "user".into(),
        content: content.clone(),
        created_at: now.clone(),
    };
    state.db.insert_chat(&user_msg).map_err(|e| e.to_string())?;

    emit_chat_progress(&app, &novel_id, "context");
    let mut tree = get_tree(novel_id.clone())?;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            content.as_str(),
            novel.title.as_str(),
            novel.synopsis.as_str(),
        ],
    );

    // —— 根 Chat 明确指令（确定性，优先于自由对话）——
    if parse_clear_all_chapters(&content) {
        emit_chat_progress(&app, &novel_id, "apply_outlines");
        let removed = clear_all_chapter_nodes(&mut tree);
        let n = removed.len();
        let _ = save_tree(tree);
        // 同步清掉正文文件与章节记忆，避免「节点没了记忆还在」
        purge_chapter_artifacts(&state, &novel_id, &removed);
        emit_chat_progress(&app, &novel_id, "saving");
        let reply = if loc.is_zh() {
            format!("已清空全部章节节点（共 {n} 个），并清除对应正文与章节记忆。角色卡与剧情卡仍保留。")
        } else {
            format!(
                "Cleared all chapter nodes ({n}), including bodies and chapter memory. Character/plot cards kept."
            )
        };
        return finish_chat_reply(&state, &novel_id, reply).await;
    }
    if parse_clear_all_plots(&content) {
        emit_chat_progress(&app, &novel_id, "apply_outlines");
        let n = tree
            .nodes
            .iter()
            .filter(|x| matches!(x.kind, NodeKind::SidePlot))
            .count();
        clear_all_side_plot_nodes(&mut tree);
        let _ = save_tree(tree);
        emit_chat_progress(&app, &novel_id, "saving");
        let reply = if loc.is_zh() {
            format!("已清空全部剧情卡（共 {n} 张）。章节与角色卡仍保留。")
        } else {
            format!("Cleared all plot cards ({n}). Chapters and characters kept.")
        };
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some((ch, plot)) = parse_add_plot_command(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        let reply = match add_plot_to_chapter(&mut tree, ch, &plot) {
            Ok(msg) => {
                let _ = save_tree(tree);
                msg
            }
            Err(e) => e,
        };
        emit_chat_progress(&app, &novel_id, "saving");
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some(chapter_num) = parse_gen_plots_for_chapter(&content) {
        let existing = existing_chapter_nums(&tree);
        let Some(chapter_id) = existing.get(&chapter_num).cloned() else {
            let reply = if loc.is_zh() {
                format!("结构树上找不到第{chapter_num}章，请先生成章节卡。")
            } else {
                format!("Chapter {chapter_num} not found. Generate the chapter card first.")
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        };
        let reply = match enrich_plots_for_chapter_node(
            &app,
            &state,
            &settings,
            &novel,
            &novel_id,
            &mut tree,
            &chapter_id,
            Some(chapter_num),
            loc,
            Some(cancel.clone()),
            None,
            "",
            false,
        )
        .await
        {
            Ok((r, _, _)) => r,
            Err(e) => e,
        };
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some((chapter_num, brief)) = parse_regen_outline_command(&content) {
        let Some(n) = chapter_num else {
            let reply = if loc.is_zh() {
                "请指定章节：`/刷新大纲 3` 或 `/刷新大纲 第三章`；可换行后写期望。".into()
            } else {
                "Specify a chapter: `/refresh-outline 3`; optional expectations on following lines."
                    .into()
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        };
        let existing = existing_chapter_nums(&tree);
        let Some(chapter_id) = existing.get(&n).cloned() else {
            let reply = if loc.is_zh() {
                format!("结构树上找不到第{n}章，请先生成章节卡。")
            } else {
                format!("Chapter {n} not found. Generate the chapter card first.")
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        };
        let reply = match run_regenerate_chapter_outline(
            &app,
            &state,
            &settings,
            &novel_id,
            &chapter_id,
            &brief,
            cancel.clone(),
            false,
        )
        .await
        {
            Ok(msg) => msg,
            Err(e) => e,
        };
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some((from, to, resolve)) = parse_gen_chapter_cards(&content) {
        let existing = existing_chapter_nums(&tree);
        // 未指定模式：无冲突 → 追加；有同号 → 覆盖（从 from 起重写，与左侧「生成章节卡」一致）
        let mode = resolve.unwrap_or(if (from..=to).any(|n| existing.contains_key(&n)) {
            ChapterResolveMode::Overwrite
        } else {
            ChapterResolveMode::ForceAppend
        });
        let nums: Vec<u32> = match mode {
            ChapterResolveMode::Skip => (from..=to).filter(|n| !existing.contains_key(n)).collect(),
            ChapterResolveMode::Overwrite | ChapterResolveMode::ForceAppend => {
                (from..=to).collect()
            }
        };
        if nums.is_empty() {
            let reply = if loc.is_zh() {
                "指定范围内没有需要新生成的章节（均已存在且选择了跳过）。".into()
            } else {
                "Nothing to generate in that range (all skipped).".into()
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        }
        emit_chat_progress(&app, &novel_id, "thinking");
        let (reply, applied) = apply_llm_chapter_cards(
            &state,
            &settings,
            &novel,
            &novel_id,
            &mut tree,
            from,
            to,
            &nums,
            mode,
            loc,
            Some(cancel.clone()),
            "",
            None,
        )
        .await?;
        if applied {
            emit_chat_progress(&app, &novel_id, "apply_outlines");
        }
        emit_chat_progress(&app, &novel_id, "saving");
        let mode_zh = match mode {
            ChapterResolveMode::Overwrite => "覆盖标题与大纲",
            ChapterResolveMode::Skip => "跳过已有",
            ChapterResolveMode::ForceAppend => "追加",
        };
        let note = if applied {
            if loc.is_zh() {
                format!("\n\n（已按「{mode_zh}」写入第{from}–{to}章到结构树。）")
            } else {
                format!("\n\n(Applied chapters {from}–{to}, mode={mode_zh}.)")
            }
        } else if loc.is_zh() {
            "\n\n（未解析到 outlines JSON，结构树未改动。请重试。）".into()
        } else {
            "\n\n(No outlines JSON found; tree unchanged.)".into()
        };
        return finish_chat_reply(&state, &novel_id, format!("{reply}{note}")).await;
    }

    // 以 / 开头但未识别 → 列出指令；`/skill-name` 除外（走 skill 注入）
    if content.trim().starts_with('/') && !crate::skills::is_slash_skill_invoke(&content) {
        emit_chat_progress(&app, &novel_id, "saving");
        return finish_chat_reply(&state, &novel_id, root_slash_help(loc.is_zh())).await;
    }

    let has_chapters = tree
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Chapter));
    let chapter_list = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .map(|n| format!("{}({})", n.label, n.id))
        .collect::<Vec<_>>()
        .join(", ");
    let root_outline = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.outline.as_str())
        .unwrap_or("");
    let system = prompts::workspace_chat_system(
        loc,
        &novel.title,
        &novel.synopsis,
        root_outline,
        has_chapters,
        novel.word_count_min,
        novel.word_count_max,
        novel.chapter_count,
        &chapter_list,
    );

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let mut llm_user = enrich_user_with_urls_progress(&app, &novel_id, &content).await;
    if let Some((_skill_name, injected)) = crate::skills::maybe_inject_skill(&llm_user) {
        emit_chat_progress(&app, &novel_id, "skill");
        llm_user = injected;
    }
    emit_chapter_tokens(
        &app,
        &novel_id,
        estimate_prompt_tokens(&system, &llm_user),
        0,
        false,
    );
    let tool_run = crate::chat_tools::run_with_tools(
        &settings,
        &state.db,
        &novel_id,
        &mut tree,
        &system,
        &llm_user,
        Some(&chat_model),
        Some(cancel.clone()),
        |name| {
            emit_chat_progress(&app, &novel_id, "tool");
            let _ = name;
        },
    )
    .await
    .map_err(|e| {
        let s = e.to_string();
        if s.contains("cancelled") {
            "cancelled".into()
        } else {
            s
        }
    })?;
    emit_chapter_tokens(
        &app,
        &novel_id,
        tool_run.completion.prompt_tokens,
        tool_run.completion.completion_tokens,
        true,
    );
    {
        let now = Local::now();
        let day = now.format("%Y-%m-%d").to_string();
        let hour = now.hour() as u8;
        let r = &tool_run.completion;
        let _ = state.db.add_token_usage(
            &day,
            hour,
            &r.model,
            &novel_id,
            r.prompt_tokens,
            r.completion_tokens,
            r.total_tokens,
        );
    }
    let reply = tool_run.completion.content;
    let (def_label, def_role, def_align) = prompts::default_new_character(loc);
    let mut tree_dirty = tool_run.tree_dirty;

    // 用户话里直接改字数 / 总章数 → 立刻落库（不依赖模型 JSON）
    if let Some((wmin, wmax)) = parse_word_target_from_text(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        apply_novel_word_target(&state, &mut novel, &mut tree, wmin, wmax)?;
        tree_dirty = true;
    }
    if let Some(n) = parse_chapter_count_from_text(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        apply_novel_chapter_count(&state, &mut novel, &mut tree, n)?;
        tree_dirty = true;
    }

    for v in extract_json_objects(&reply) {
        if let Some((wmin, wmax)) = word_target_from_json(&v) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            apply_novel_word_target(&state, &mut novel, &mut tree, wmin, wmax)?;
            tree_dirty = true;
        }
        if let Some(n) = chapter_count_from_json(&v) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            apply_novel_chapter_count(&state, &mut novel, &mut tree, n)?;
            tree_dirty = true;
        }
        if let Some(c) = v.get("character") {
            emit_chat_progress(&app, &novel_id, "apply_character");
            let cid = uuid::Uuid::new_v4().to_string();
            let label = c
                .get("label")
                .and_then(|x| x.as_str())
                .unwrap_or(def_label)
                .to_string();
            let y = tree
                .nodes
                .iter()
                .map(|n| n.position.y)
                .fold(0.0_f64, f64::max)
                + 120.0;
            tree.nodes.push(TreeNode {
                id: cid.clone(),
                kind: NodeKind::Character,
                label,
                outline: String::new(),
                character: Some(CharacterCard {
                    role: c
                        .get("role")
                        .and_then(|x| x.as_str())
                        .unwrap_or(def_role)
                        .into(),
                    personality: c
                        .get("personality")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                    gender: c
                        .get("gender")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                    alignment: c
                        .get("alignment")
                        .and_then(|x| x.as_str())
                        .unwrap_or(def_align)
                        .into(),
                }),
                knowledge: None,
                side_plot: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: 40.0, y },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            });
            let link = c
                .get("link_chapter_id")
                .and_then(|x| x.as_str())
                .filter(|id| tree.nodes.iter().any(|n| n.id == *id))
                .map(|s| s.to_string())
                .or_else(|| {
                    tree.nodes
                        .iter()
                        .find(|n| matches!(n.kind, NodeKind::Novel))
                        .map(|n| n.id.clone())
                });
            if let Some(ch) = link {
                if let Some(node) = tree.nodes.iter_mut().find(|n| n.id == ch) {
                    node.linked_character_ids.push(cid.clone());
                }
                tree.edges.push(TreeEdge {
                    id: format!("e-{ch}-{cid}"),
                    source: ch,
                    target: cid,
                    kind: "character".into(),
                    source_handle: Some("left".into()),
                    target_handle: Some("right".into()),
                    label: String::new(),
                });
            }
            tree_dirty = true;
        }
        if let Some(outline) = v.get("root_outline").and_then(|x| x.as_str()) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            if let Some(root) = tree
                .nodes
                .iter_mut()
                .find(|n| matches!(n.kind, NodeKind::Novel))
            {
                root.outline = outline.to_string();
                tree_dirty = true;
            }
        }
        if let Some(list) = v.get("outlines").and_then(|x| x.as_array()) {
            if !list.is_empty() {
                emit_chat_progress(&app, &novel_id, "apply_outlines");
                // Default: upsert by chapter number (regenerate replaces title+outline).
                let mode = outline_mode_from_json(&v).unwrap_or(ChapterResolveMode::Overwrite);
                apply_chapter_outlines(&mut tree, list, mode);
                tree_dirty = true;
            }
        }
    }
    if tree_dirty {
        let _ = save_tree(tree);
    }

    emit_chat_progress(&app, &novel_id, "saving");
    finish_chat_reply(&state, &novel_id, reply).await
}

/// 按知识卡上的书目与提取需求，用 AI 提炼特征写回卡片。
#[tauri::command]
pub async fn extract_knowledge_card(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<NovelTree, String> {
    let mut tree = get_tree(novel_id.clone())?;
    let idx = tree
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(tree.nodes[idx].kind, NodeKind::Knowledge) {
        return Err("不是知识卡".into());
    }
    let payload = tree.nodes[idx]
        .knowledge
        .clone()
        .unwrap_or_default();
    if payload.book_ids.is_empty() {
        return Err("请先选择至少一个知识库".into());
    }
    let extract_prompt = payload.extract_prompt.trim();
    if extract_prompt.is_empty() {
        return Err("请先填写提取需求".into());
    }

    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let corpus = gather_knowledge_corpus(&state, &payload.book_ids, 14_000)?;
    if corpus.trim().is_empty() {
        return Err("所选知识库没有可用分段".into());
    }
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[extract_prompt, corpus.as_str()],
    );
    let model = task_model(&settings.knowledge_model, &settings.default_model);
    let sys = prompts::knowledge_card_extract_system(loc);
    let user = prompts::knowledge_card_extract_user(loc, extract_prompt, &corpus);
    let (out, _) = llm_complete(
        None,
        &state,
        &settings,
        sys,
        &user,
        Some(&model),
        Some(&novel_id),
        None,
    )
    .await?;

    let n = &mut tree.nodes[idx];
    let mut k = n.knowledge.clone().unwrap_or_default();
    k.extracted = out.trim().to_string();
    // outline mirrors extracted for tree preview
    n.outline = k.extracted.chars().take(200).collect();
    n.knowledge = Some(k);
    save_tree(tree.clone())?;
    Ok(tree)
}

/// 从绑定知识库同步设定知识卡到根节点（覆盖同书 from_canon 卡）。
#[tauri::command]
pub async fn sync_canon_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<GenerateResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[novel.title.as_str(), novel.synopsis.as_str()],
    );
    if novel.knowledge_ids.is_empty() {
        return Err(if loc.is_zh() {
            "请先在根节点绑定至少一个公共知识库".into()
        } else {
            "Bind at least one knowledge book on the root first".into()
        });
    }

    let mut tree = get_tree(novel_id.clone())?;
    let root_id = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
        .ok_or_else(|| "缺少根节点".to_string())?;

    let total = (novel.knowledge_ids.len() as u32).max(1);
    let extract_prompt = prompts::canon_sync_extract_prompt(loc);
    let model = task_model(&settings.knowledge_model, &settings.default_model);
    let mut synced = 0usize;

    for (i, bid) in novel.knowledge_ids.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        emit_chapter_progress(&app, &novel_id, "context", (i as u32) + 1, total);
        let book = state
            .db
            .get_knowledge(bid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("知识库不存在: {bid}"))?;
        if book.archived {
            continue;
        }
        let corpus = gather_knowledge_corpus(&state, &[bid.clone()], 14_000)?;
        if corpus.trim().is_empty() {
            continue;
        }
        let sys = prompts::knowledge_card_extract_system(loc);
        let user = prompts::knowledge_card_extract_user(loc, extract_prompt, &corpus);
        let (out, _) = llm_complete(
            Some(&app),
            &state,
            &settings,
            sys,
            &user,
            Some(&model),
            Some(&novel_id),
            Some(cancel.clone()),
        )
        .await?;
        let extracted = out.trim().to_string();
        if extracted.is_empty() {
            continue;
        }

        let label = if loc.is_zh() {
            format!("设定 · {}", book.title)
        } else {
            format!("Setting · {}", book.title)
        };

        let existing = tree.nodes.iter().position(|n| {
            matches!(n.kind, NodeKind::Knowledge)
                && n.knowledge
                    .as_ref()
                    .map(|k| k.from_canon && k.book_ids.iter().any(|x| x == bid))
                    .unwrap_or(false)
        });

        let node_id = if let Some(idx) = existing {
            let n = &mut tree.nodes[idx];
            n.label = label.clone();
            n.outline = extracted.chars().take(200).collect();
            n.knowledge = Some(KnowledgeCardPayload {
                book_ids: vec![bid.clone()],
                extract_prompt: extract_prompt.to_string(),
                extracted: extracted.clone(),
                from_canon: true,
            });
            n.id.clone()
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let y = tree
                .nodes
                .iter()
                .map(|n| n.position.y)
                .fold(0.0_f64, f64::max)
                + 100.0;
            tree.nodes.push(TreeNode {
                id: id.clone(),
                kind: NodeKind::Knowledge,
                label: label.clone(),
                outline: extracted.chars().take(200).collect(),
                character: None,
                knowledge: Some(KnowledgeCardPayload {
                    book_ids: vec![bid.clone()],
                    extract_prompt: extract_prompt.to_string(),
                    extracted: extracted.clone(),
                    from_canon: true,
                }),
                side_plot: None,
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                linked_knowledge_ids: vec![],
                position: NodePosition { x: -220.0, y },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            });
            let edge_id = format!("e-canon-{id}");
            if !tree.edges.iter().any(|e| e.id == edge_id) {
                tree.edges.push(TreeEdge {
                    id: edge_id,
                    source: root_id.clone(),
                    target: id.clone(),
                    kind: "knowledge".into(),
                    source_handle: Some("left".into()),
                    target_handle: Some("right".into()),
                    label: String::new(),
                });
            }
            id
        };

        if let Some(root) = tree.nodes.iter_mut().find(|n| n.id == root_id) {
            if !root.linked_knowledge_ids.iter().any(|x| x == &node_id) {
                root.linked_knowledge_ids.push(node_id);
            }
        }
        synced += 1;
    }

    emit_chapter_progress(&app, &novel_id, "saving", total, total);
    save_tree(tree)?;
    if synced == 0 {
        return Err(if loc.is_zh() {
            "未能从绑定知识库提取设定，请确认书目有分段数据。".into()
        } else {
            "Could not extract settings — ensure bound books have chunks.".into()
        });
    }
    let message = if loc.is_zh() {
        format!("已同步 {synced} 张设定知识卡到根节点。")
    } else {
        format!("Synced {synced} setting knowledge card(s) to the root.")
    };
    Ok(GenerateResult {
        content: String::new(),
        used_mock: false,
        message,
    })
}

fn gather_knowledge_corpus(
    state: &State<'_, AppState>,
    book_ids: &[String],
    max_chars: usize,
) -> Result<String, String> {
    let mut out = String::new();
    for bid in book_ids {
        let book = state
            .db
            .get_knowledge(bid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("知识库不存在: {bid}"))?;
        if book.archived {
            continue;
        }
        let chunks = state
            .db
            .list_knowledge_chunks(bid)
            .map_err(|e| e.to_string())?;
        out.push_str(&format!("### 《{}》· {}\n", book.title, book.author));
        // Prefer analysis / toc chunks first
        let mut ordered = chunks;
        ordered.sort_by_key(|c| {
            let p = if c.content.contains("【知识库分析】") {
                0
            } else if c.content.contains("【目录】") {
                1
            } else {
                2
            };
            (p, c.idx)
        });
        for c in ordered.into_iter().take(24) {
            let piece = format!("{}\n\n", c.content);
            if out.chars().count() + piece.chars().count() > max_chars {
                return Ok(out);
            }
            out.push_str(&piece);
        }
    }
    Ok(out)
}

fn novel_canon_strict(novel: &NovelProject) -> bool {
    novel.canon_mode.eq_ignore_ascii_case("strict")
}

/// Keep only canon fragments that mention the missing setting terms.
fn canon_snippets_for_missing(canon_text: &str, missing: &[&LandBeat]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let terms: Vec<String> = missing
        .iter()
        .map(|b| b.title.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut kept = Vec::new();
    for part in canon_text.split("\n\n---\n\n") {
        let pl = part.to_lowercase();
        if terms.iter().any(|t| pl.contains(t)) {
            kept.push(part.trim());
        }
    }
    if kept.is_empty() {
        // fallback: short head of original block
        return crate::kb_context::truncate_chars(canon_text, 1200);
    }
    let joined = kept.join("\n\n---\n\n");
    crate::kb_context::truncate_chars(&joined, 2000)
}

/// 从绑定知识库按查询检索片段；并附根上 from_canon 知识卡摘要。
fn build_canon_context(
    state: &State<'_, AppState>,
    novel: &NovelProject,
    tree: &NovelTree,
    query: &str,
    loc: PromptLocale,
) -> String {
    if novel.knowledge_ids.is_empty() {
        return String::new();
    }
    let strict = novel_canon_strict(novel);
    let mut parts: Vec<String> = Vec::new();

    if let Some(root) = tree.nodes.iter().find(|n| matches!(n.kind, NodeKind::Novel)) {
        for kid in &root.linked_knowledge_ids {
            if let Some(n) = tree.nodes.iter().find(|x| x.id == *kid) {
                let Some(k) = n.knowledge.as_ref() else { continue };
                if !k.from_canon {
                    continue;
                }
                let feat = k.extracted.trim();
                if feat.is_empty() {
                    continue;
                }
                let snip = crate::kb_context::truncate_chars(feat, crate::kb_context::FROM_CANON_CARD_CAP);
                if loc.is_zh() {
                    parts.push(format!("【设定卡·{}】\n{snip}", n.label));
                } else {
                    parts.push(format!("[Setting card · {}]\n{snip}", n.label));
                }
            }
        }
    }

    let hits = crate::kb_context::retrieve_knowledge(
        &state.db,
        &novel.knowledge_ids,
        query,
        crate::kb_context::CANON_RETRIEVE_K,
        crate::kb_context::CANON_CHUNK_CAP,
    );
    for (sid, title, preview) in hits {
        if loc.is_zh() {
            parts.push(format!("【检索·《{title}》|{sid}】\n{preview}"));
        } else {
            parts.push(format!("[Retrieval · “{title}”|{sid}]\n{preview}"));
        }
    }

    if parts.is_empty() {
        return String::new();
    }
    let mut joined = parts.join("\n\n---\n\n");
    if joined.chars().count() > crate::kb_context::CANON_BLOCK_CAP {
        joined = crate::kb_context::truncate_chars(&joined, crate::kb_context::CANON_BLOCK_CAP);
    }
    prompts::format_canon_block(loc, strict, &joined)
}

/// 本章大纲涉及的设定关键词：若正文未覆盖则作为补写项（strict 用）。
fn collect_canon_land_beats(canon_text: &str, outline: &str) -> Vec<LandBeat> {
    let outline_l = outline.to_lowercase();
    let terms = chapter_memory::query_terms(canon_text);
    let mut beats = Vec::new();
    for t in terms.into_iter().take(40) {
        if t.chars().count() < 2 {
            continue;
        }
        let tl = t.to_lowercase();
        if !outline_l.contains(&tl) {
            continue;
        }
        beats.push(LandBeat {
            kind: "canon",
            title: t.clone(),
            text: t,
        });
        if beats.len() >= 12 {
            break;
        }
    }
    beats
}

#[tauri::command]
pub async fn card_chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    content: String,
    model: Option<String>,
) -> Result<CardChatResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };

    emit_chat_progress(&app, &novel_id, "context");
    let mut tree = get_tree(novel_id.clone())?;
    let idx = tree
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    let kind = tree.nodes[idx].kind.clone();
    if matches!(kind, NodeKind::Novel) {
        return Err("根节点请用创作 Chat".into());
    }
    if matches!(kind, NodeKind::Knowledge) {
        return Err("知识卡请用「提取知识」按钮".into());
    }

    let now = Utc::now().to_rfc3339();
    let user_msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.clone(),
        node_id: node_id.clone(),
        role: "user".into(),
        content: content.clone(),
        created_at: now,
    };
    state.db.insert_chat(&user_msg).map_err(|e| e.to_string())?;

    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let node_label = tree.nodes[idx].label.clone();
    let node_outline = tree.nodes[idx].outline.clone();
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            content.as_str(),
            novel.title.as_str(),
            node_label.as_str(),
            node_outline.as_str(),
        ],
    );
    if matches!(kind, NodeKind::Chapter) {
        // 硬约束：章节卡 Chat 只对当前章有效（含拒绝生成后续章）
        if parse_plan_next_command(&content).is_some()
            || parse_gen_chapter_cards(&content).is_some()
            || parse_clear_all_chapters(&content)
            || parse_clear_all_plots(&content)
            || parse_add_plot_command(&content).is_some()
            || parse_gen_plots_for_chapter(&content).is_some()
        {
            return finish_card_chat_reply(
                &state,
                &novel_id,
                &node_id,
                chapter_slash_help(loc.is_zh()),
                tree,
            );
        }

        if let Some(notes) = parse_enrich_plots_command(&content) {
            let reply = match enrich_plots_for_chapter_node(
                &app,
                &state,
                &settings,
                &novel,
                &novel_id,
                &mut tree,
                &node_id,
                chapter_number_from_label(&node_label),
                loc,
                Some(cancel.clone()),
                None,
                notes.trim(),
                false,
            )
            .await
            {
                Ok((r, _, _)) => r,
                Err(e) => e,
            };
            let tree = get_tree(novel_id.clone())?;
            return finish_card_chat_reply(&state, &novel_id, &node_id, reply, tree);
        }

        if let Some((chapter_num, brief)) = parse_regen_outline_command(&content) {
            // 章节卡：禁止带章号；一律刷新当前节点
            if let Some(n) = chapter_num {
                let reply = if loc.is_zh() {
                    format!(
                        "章节卡 Chat 只刷新本章大纲，不能指定第{n}章。\n\
                         · 刷新本章：`/刷新本章大纲`（可换行写期望）\n\
                         · 刷新其他章：切换到该章卡片，或在根节点 Chat 使用 `/刷新大纲 {n}`"
                    )
                } else {
                    format!(
                        "Chapter-card Chat only refreshes this chapter, not chapter {n}.\n\
                         · This chapter: `/refresh-this-outline` (expectations below)\n\
                         · Other chapters: open that card, or root Chat `/refresh-outline {n}`"
                    )
                };
                return finish_card_chat_reply(&state, &novel_id, &node_id, reply, tree);
            }
            let reply = match run_regenerate_chapter_outline(
                &app,
                &state,
                &settings,
                &novel_id,
                &node_id,
                &brief,
                cancel.clone(),
                false,
            )
            .await
            {
                Ok(msg) => msg,
                Err(e) => e,
            };
            let tree = get_tree(novel_id.clone())?;
            return finish_card_chat_reply(&state, &novel_id, &node_id, reply, tree);
        }

        if let Some(brief) = parse_generate_body_command(&content) {
            let r = generate_chapter(
                app.clone(),
                state.clone(),
                novel_id.clone(),
                node_id.clone(),
                vec![],
                brief,
                model.clone(),
            )
            .await?;
            let tree = get_tree(novel_id.clone())?;
            return finish_card_chat_reply(&state, &novel_id, &node_id, r.message, tree);
        }

        if let Some(brief) = parse_refine_body_command(&content) {
            let r = refine_chapter(
                app.clone(),
                state.clone(),
                novel_id.clone(),
                node_id.clone(),
                10,
                Some("memory".into()),
                Some(brief),
                model.clone(),
            )
            .await?;
            let tree = get_tree(novel_id.clone())?;
            return finish_card_chat_reply(&state, &novel_id, &node_id, r.message, tree);
        }

        if content.trim().starts_with('/') && !crate::skills::is_slash_skill_invoke(&content) {
            return finish_card_chat_reply(
                &state,
                &novel_id,
                &node_id,
                chapter_slash_help(loc.is_zh()),
                tree,
            );
        }
    }

    let kind_key = match kind {
        NodeKind::Chapter => "chapter",
        NodeKind::Character => "character",
        NodeKind::SidePlot => "side_plot",
        NodeKind::Novel | NodeKind::Knowledge => unreachable!(),
    };
    let system = prompts::card_chat_system(loc, kind_key, &novel.title, &node_label, &node_outline);

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let mut llm_user = enrich_user_with_urls_progress(&app, &novel_id, &content).await;
    if let Some((_skill_name, injected)) = crate::skills::maybe_inject_skill(&llm_user) {
        emit_chat_progress(&app, &novel_id, "skill");
        llm_user = injected;
    }
    let (reply, _) = llm_complete(
        Some(&app),
        &state,
        &settings,
        &system,
        &llm_user,
        Some(&chat_model),
        Some(&novel_id),
        Some(cancel),
    )
    .await?;

    if let Some(v) = first_json_with_any_key(&reply, &["label", "outline", "character"]) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        let n = &mut tree.nodes[idx];
        match kind {
            NodeKind::Chapter | NodeKind::SidePlot => {
                if let Some(label) = v.get("label").and_then(|x| x.as_str()) {
                    if !label.is_empty() {
                        n.label = label.to_string();
                    }
                }
                if let Some(outline) = v.get("outline").and_then(|x| x.as_str()) {
                    n.outline = outline.to_string();
                }
            }
            NodeKind::Character => {
                if let Some(c) = v.get("character") {
                    if let Some(label) = c.get("label").and_then(|x| x.as_str()) {
                        if !label.is_empty() {
                            n.label = label.to_string();
                        }
                    }
                    let mut card = n.character.clone().unwrap_or(CharacterCard {
                        role: String::new(),
                        personality: String::new(),
                        motto: String::new(),
                        gender: String::new(),
                        style: String::new(),
                        alignment: String::new(),
                    });
                    if let Some(s) = c.get("role").and_then(|x| x.as_str()) {
                        card.role = s.into();
                    }
                    if let Some(s) = c.get("personality").and_then(|x| x.as_str()) {
                        card.personality = s.into();
                    }
                    if let Some(s) = c.get("motto").and_then(|x| x.as_str()) {
                        card.motto = s.into();
                    }
                    if let Some(s) = c.get("gender").and_then(|x| x.as_str()) {
                        card.gender = s.into();
                    }
                    if let Some(s) = c.get("style").and_then(|x| x.as_str()) {
                        card.style = s.into();
                    }
                    if let Some(s) = c.get("alignment").and_then(|x| x.as_str()) {
                        card.alignment = s.into();
                    }
                    n.character = Some(card);
                }
            }
            NodeKind::Novel | NodeKind::Knowledge => {}
        }
    }

    emit_chat_progress(&app, &novel_id, "saving");
    save_tree(tree.clone())?;
    finish_card_chat_reply(&state, &novel_id, &node_id, reply, tree)
}

/// 根据书名 / 简介 / 根大纲 / 挂载人物，生成英文文生图封面提示词。
#[tauri::command]
pub async fn generate_cover_prompt(
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<String, String> {
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let tree = get_tree(novel_id.clone())?;
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .ok_or_else(|| "缺少根节点".to_string())?;

    let mut char_lines = Vec::new();
    for cid in &root.linked_character_ids {
        let Some(n) = tree.nodes.iter().find(|n| n.id == *cid) else {
            continue;
        };
        if !matches!(n.kind, NodeKind::Character) {
            continue;
        }
        let c = n.character.as_ref();
        char_lines.push(format!(
            "- {} | role: {} | gender: {} | style: {} | personality: {}",
            n.label,
            c.map(|x| x.role.as_str()).unwrap_or(""),
            c.map(|x| x.gender.as_str()).unwrap_or(""),
            c.map(|x| x.style.as_str()).unwrap_or(""),
            c.map(|x| x.personality.as_str()).unwrap_or(""),
        ));
        if char_lines.len() >= 8 {
            break;
        }
    }
    let characters = if char_lines.is_empty() {
        "(none linked on root)".to_string()
    } else {
        char_lines.join("\n")
    };

    let root_outline = {
        let o = root.outline.trim();
        if o.is_empty() {
            "(empty)".to_string()
        } else {
            o.chars().take(2500).collect()
        }
    };
    let synopsis = {
        let s = novel.synopsis.trim();
        if s.is_empty() {
            "(empty)".to_string()
        } else {
            s.chars().take(2000).collect()
        }
    };

    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let model = task_model(&settings.create_model, &settings.default_model);
    let sys = prompts::cover_t2i_prompt_system();
    let user = prompts::cover_t2i_prompt_user(&novel.title, &synopsis, &root_outline, &characters);
    let (out, _) = llm_complete(
        None,
        &state,
        &settings,
        sys,
        &user,
        Some(&model),
        Some(&novel_id),
        None,
    )
    .await?;
    let prompt = out.trim().trim_matches('"').trim().to_string();
    if prompt.is_empty() {
        return Err("模型未返回提示词".into());
    }
    Ok(prompt)
}

#[tauri::command]
pub async fn pick_cover(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
        .blocking_pick_file();
    Ok(file.and_then(|f| f.as_path().map(|p| p.to_string_lossy().to_string())))
}

#[tauri::command]
pub fn set_cover(
    state: State<'_, AppState>,
    novel_id: String,
    source_path: String,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let ext = Path::new(&source_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let dest = novel_dir(&novel_id).join(format!("cover.{ext}"));
    fs::copy(&source_path, &dest).map_err(|e| e.to_string())?;
    novel.cover_path = Some(dest.to_string_lossy().to_string());
    novel.updated_at = Utc::now().to_rfc3339();
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    fs::write(
        novel_meta_path(&novel_id),
        serde_json::to_string_pretty(&novel).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(novel)
}

#[tauri::command]
pub async fn pick_text_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Text", &["txt"])
        .add_filter("All", &["txt", "epub", "pdf"])
        .blocking_pick_file();
    Ok(file.and_then(|f| f.as_path().map(|p| p.to_string_lossy().to_string())))
}

#[tauri::command]
pub fn app_data_root() -> String {
    app_root().to_string_lossy().to_string()
}

#[tauri::command]
pub fn list_chat_skills() -> crate::skills::SkillsPreview {
    crate::skills::list_previews()
}

#[tauri::command]
pub fn preview_chat_skill_match(query: String) -> crate::skills::SkillMatchPreview {
    crate::skills::preview_match(&query)
}

#[tauri::command]
pub fn get_token_usage(state: State<'_, AppState>, date: String) -> Result<TokenUsageDay, String> {
    let day = date.trim();
    let day = if day.is_empty() {
        Local::now().format("%Y-%m-%d").to_string()
    } else {
        day.to_string()
    };
    let rows = state
        .db
        .list_token_usage_day(&day)
        .map_err(|e| e.to_string())?;
    let novels = state
        .db
        .list_token_usage_by_novel(&day)
        .map_err(|e| e.to_string())?;
    let mut models: Vec<String> = rows.iter().map(|r| r.model.clone()).collect();
    models.sort();
    models.dedup();
    let prompt_tokens = rows.iter().map(|r| r.prompt_tokens).sum();
    let completion_tokens = rows.iter().map(|r| r.completion_tokens).sum();
    let total_tokens = rows.iter().map(|r| r.total_tokens).sum();
    Ok(TokenUsageDay {
        date: day,
        rows,
        models,
        novels,
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

#[tauri::command]
pub fn get_token_usage_month(
    state: State<'_, AppState>,
    month: String,
) -> Result<TokenUsageMonth, String> {
    let m = month.trim();
    let m = if m.is_empty() {
        Local::now().format("%Y-%m").to_string()
    } else {
        m.to_string()
    };
    let models = state
        .db
        .list_token_usage_month(&m)
        .map_err(|e| e.to_string())?;
    let prompt_tokens = models.iter().map(|r| r.prompt_tokens).sum();
    let completion_tokens = models.iter().map(|r| r.completion_tokens).sum();
    let total_tokens = models.iter().map(|r| r.total_tokens).sum();
    Ok(TokenUsageMonth {
        month: m,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        models,
    })
}

pub fn init_state() -> Result<AppState, String> {
    let db = Db::open().map_err(|e| e.to_string())?;
    crate::sample::ensure_sample(&db).map_err(|e| e.to_string())?;
    Ok(AppState {
        db: Arc::new(db),
        chat_cancel: Default::default(),
    })
}

#[tauri::command]
pub fn chat_cancel(state: State<'_, AppState>, novel_id: String) -> Result<(), String> {
    let key = {
        let t = novel_id.trim();
        if t.is_empty() {
            CREATE_CHAT_CANCEL_KEY
        } else {
            t
        }
    };
    state.request_chat_cancel(key);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::PromptLocale;

    #[test]
    fn parse_single_chapter_word_target() {
        assert_eq!(
            parse_word_target_from_text("请把每章目标改成4500字"),
            Some((4500, 4500))
        );
        assert_eq!(
            parse_word_target_from_text("set per chapter target to 4500 words"),
            Some((4500, 4500))
        );
    }

    #[test]
    fn parse_range_chapter_word_target() {
        assert_eq!(
            parse_word_target_from_text("每章 4000-4500 字"),
            Some((4000, 4500))
        );
    }

    #[test]
    fn word_count_tolerance_60() {
        assert!(word_count_ok(1940, 2000, 3000)); // 2000-60
        assert!(word_count_ok(3060, 2000, 3000)); // 3000+60
        assert!(!word_count_ok(1939, 2000, 3000));
        assert!(!word_count_ok(3061, 2000, 3000));
        assert!(word_count_ok(4440, 4500, 4500));
        assert!(!word_count_ok(4439, 4500, 4500));
        assert_eq!(word_tolerance_bounds(2000, 3000), (1940, 3060));
    }

    #[test]
    fn word_target_json() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"word_count":{"min":4500,"max":4500}}"#).unwrap();
        assert_eq!(word_target_from_json(&v), Some((4500, 4500)));
    }

    #[test]
    fn parse_planned_chapter_count() {
        assert_eq!(parse_chapter_count_from_text("这本一共20章"), Some(20));
        assert_eq!(parse_chapter_count_from_text("计划总共 30 章"), Some(30));
        assert_eq!(
            parse_chapter_count_from_text("plan about 25 chapters total"),
            Some(25)
        );
        let v: serde_json::Value = serde_json::from_str(r#"{"chapter_count":40}"#).unwrap();
        assert_eq!(chapter_count_from_json(&v), Some(40));
    }

    #[test]
    fn detect_zh_over_english_ui() {
        let loc = PromptLocale::for_interaction("en", &["帮我写一个腹黑女主，每章4500字"]);
        assert!(loc.is_zh());
    }

    #[test]
    fn detect_en_input() {
        let loc = PromptLocale::for_interaction(
            "zh-CN",
            &["Write a calm heroine and set 4500 words per chapter"],
        );
        assert_eq!(loc, PromptLocale::En);
    }

    #[test]
    fn extract_pretty_plots_json() {
        let reply = r#"好的，拆解如下：
```json
{
  "plots": [
    {"label": "密信", "outline": "主角发现密信", "characters": [{"label": "林深"}]}
  ]
}
```
已完成。"#;
        let plots = first_json_array_field(reply, "plots").expect("plots");
        assert_eq!(plots.len(), 1);
        assert_eq!(plots[0]["label"], "密信");
    }

    #[test]
    fn outline_mode_json() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"mode":"overwrite","outlines":[]}"#).unwrap();
        assert_eq!(outline_mode_from_json(&v), Some(ChapterResolveMode::Overwrite));
        let v2: serde_json::Value = serde_json::from_str(r#"{"mode":"append"}"#).unwrap();
        assert_eq!(outline_mode_from_json(&v2), Some(ChapterResolveMode::ForceAppend));
    }

    #[test]
    fn extract_chat_json_variants() {
        let outlines = extract_outlines_list(
            "说明\n{\n  \"outlines\": [{\"n\":1,\"label\":\"第一章\",\"outline\":\"a\"}]\n}\n完",
        )
        .unwrap();
        assert_eq!(outlines[0]["n"], 1);
        let curly = extract_outlines_list(
            "好的\n{\n  “outlines”: [{\"n\":2,\"title\":\"第二章 · x\",\"大纲\":\"要点\"}]\n}\n",
        )
        .unwrap();
        assert_eq!(curly[0]["n"], 2);
        assert_eq!(curly[0]["label"], "第二章 · x");
        assert_eq!(curly[0]["outline"], "要点");
        let bare = extract_outlines_list(
            "[{\"n\":1,\"label\":\"第一章\",\"outline\":\"a\"}]",
        )
        .unwrap();
        assert_eq!(bare[0]["n"], 1);
        let soft = extract_outlines_list(
            r#"[{"n":"2","title":"第二章","beats":["1. [本章定位] a","2. [开篇钩子] b","3. c"]}]"#,
        )
        .expect("string n + beats array");
        assert_eq!(soft[0]["n"], 2);
        assert_eq!(soft[0]["label"], "第二章");
        assert!(soft[0]["outline"].as_str().unwrap().contains("[本章定位]"));
        assert!(soft[0]["outline"].as_str().unwrap().contains("[开篇钩子]"));

        let create = extract_create_json(
            "```json\n{\"create\":{\"title\":\"雾港\",\"synopsis\":\"s\",\"word_count_min\":2000,\"word_count_max\":3000,\"chapter_count\":20,\"characters\":[]}}\n```",
        )
        .unwrap();
        assert_eq!(create["title"], "雾港");

        let bare = extract_create_json("{\"title\":\"裸对象\",\"synopsis\":\"x\"}").unwrap();
        assert_eq!(bare["title"], "裸对象");

        let trailing = first_json_with_any_key(
            "{\n  \"label\": \"新标题\",\n  \"outline\": \"要点\",\n}",
            &["label", "outline"],
        )
        .unwrap();
        assert_eq!(trailing["label"], "新标题");

        let multi = extract_json_objects(
            "{\"word_count\":{\"min\":4000,\"max\":4500}}\n好的\n{\"root_outline\":\"全书主线\"}",
        );
        assert_eq!(multi.len(), 2);
        assert!(word_target_from_json(&multi[0]).is_some());
        assert_eq!(multi[1]["root_outline"], "全书主线");
    }

    #[test]
    fn parse_root_chat_commands() {
        assert!(parse_clear_all_chapters("/清空章节"));
        assert!(!parse_clear_all_chapters("清空所有章节节点"));
        assert!(parse_clear_all_plots("/清空所有剧情"));
        assert!(parse_clear_all_plots("/clear-plots"));
        assert!(!parse_clear_all_plots("清空所有剧情"));
        assert_eq!(parse_gen_chapter_cards("/章节卡 1-10"), Some((1, 10, None)));
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第 1-10 章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第1-10章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第一到第十章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第一章到第十二章"),
            Some((1, 12, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 覆盖 3-5"),
            Some((3, 5, Some(ChapterResolveMode::Overwrite)))
        );
        assert_eq!(
            parse_add_plot_command("/添加剧情 3 主角发现密信"),
            Some((3, "主角发现密信".into()))
        );
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 3"), Some(3));
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 第一章"), Some(1));
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 第十二章"), Some(12));
        assert!(parse_enrich_plots_command("/完善剧情").is_some());
        assert!(parse_enrich_plots_command("/enrich-plots").is_some());
        assert!(parse_enrich_plots_command("/剧情卡 3").is_none());
        assert_eq!(
            parse_enrich_plots_command("/完善剧情\n\n要有反转").as_deref(),
            Some("要有反转")
        );
        assert!(parse_generate_body_command("/预生成").is_some());
        assert_eq!(
            parse_generate_body_command("/generate\n\n偏对话").as_deref(),
            Some("偏对话")
        );
        assert!(parse_refine_body_command("/精修").is_some());
        assert!(parse_refine_body_command("/refine").is_some());
        assert_eq!(
            parse_regen_outline_command("/刷新本章大纲"),
            Some((None, String::new()))
        );
        assert_eq!(
            parse_regen_outline_command("/刷新大纲"),
            Some((None, String::new()))
        );
        assert_eq!(
            parse_regen_outline_command("/刷新大纲 3"),
            Some((Some(3), String::new()))
        );
        assert_eq!(
            parse_regen_outline_command("/refresh-outline 第三章\n\n加快节奏"),
            Some((Some(3), "加快节奏".into()))
        );
        assert_eq!(
            parse_regen_outline_command("/刷新本章大纲\n\n勿剧透"),
            Some((None, "勿剧透".into()))
        );
        // 期望里含「两年」「3天」不得误解析为章号
        let prose = "/刷新本章大纲 故事开始已经是游戏入侵两年以后，昏迷了3天";
        let (n, brief) = parse_regen_outline_command(prose).expect("regen prose");
        assert_eq!(n, None);
        assert!(brief.contains("两年"));
        assert!(brief.contains("3天"));
        assert_eq!(
            parse_regen_outline_command("/刷新大纲 3 加快节奏"),
            Some((Some(3), "加快节奏".into()))
        );
        let coerced = coerce_outlines_to_single_num(
            vec![serde_json::json!({"n": 1, "label": "第一章 · x", "outline": "a"})],
            5,
        );
        assert_eq!(coerced[0].get("n").and_then(|x| x.as_u64()), Some(5));
        assert_eq!(parse_gen_chapter_cards("/剧情卡 3"), None);
        assert_eq!(parse_gen_chapter_cards("生成 1-10 章的章节卡"), None);
        assert_eq!(chapter_number_from_label("第一章 · 夜色"), Some(1));
        assert_eq!(chapter_number_from_label("第十二章"), Some(12));
        assert_eq!(chapter_number_from_label("第12章 · 夜色"), Some(12));
        assert_eq!(normalize_chapter_label(1, "初入异界"), "第1章 · 初入异界");
        assert_eq!(normalize_chapter_label(1, "第一章 · 夜"), "第一章 · 夜");
        assert_eq!(normalize_chapter_label(3, ""), "第3章");
        // 无「第N章」标题时，按顺序把空缺编号补上，避免漏掉第 1 章
        let tree = NovelTree {
            novel_id: "n1".into(),
            nodes: vec![
                TreeNode {
                    id: "c1".into(),
                    kind: NodeKind::Chapter,
                    label: "初入异界".into(),
                    outline: String::new(),
                    character: None,
                    knowledge: None,
                    side_plot: None,
                    linked_character_ids: vec![],
                    linked_side_plot_ids: vec![],
                    linked_knowledge_ids: vec![],
                    position: NodePosition { x: 0.0, y: 10.0 },
                    word_count: 0,
                    word_count_min: 0,
                    word_count_max: 0,
                    chapter_count: 0,
                },
                TreeNode {
                    id: "c2".into(),
                    kind: NodeKind::Chapter,
                    label: "第二章 · 冲突".into(),
                    outline: String::new(),
                    character: None,
                    knowledge: None,
                    side_plot: None,
                    linked_character_ids: vec![],
                    linked_side_plot_ids: vec![],
                    linked_knowledge_ids: vec![],
                    position: NodePosition { x: 0.0, y: 20.0 },
                    word_count: 0,
                    word_count_min: 0,
                    word_count_max: 0,
                    chapter_count: 0,
                },
            ],
            edges: vec![],
        };
        let map = existing_chapter_nums(&tree);
        assert_eq!(map.get(&1).map(String::as_str), Some("c1"));
        assert_eq!(map.get(&2).map(String::as_str), Some("c2"));
        assert_eq!(parse_chinese_numeral("二十一"), Some(21));
        assert_eq!(
            parse_add_plot_command("/添加剧情 第一章 主角发现密信"),
            Some((1, "主角发现密信".into()))
        );
    }
}

}
