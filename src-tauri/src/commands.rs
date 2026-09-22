use crate::chapter_constraints::{self, LandBeat};
use crate::chapter_memory;
use crate::db::Db;
use crate::knowledge::{
    build_knowledge_chunks, fetch_urls_context, load_source, load_source_from_url,
    with_fetched_url_context,
};
use crate::llm;
use crate::models::*;
use crate::paths::*;
use crate::prompts::{self, PromptLocale};
use crate::AppState;
use serde::{Deserialize, Serialize};
use chrono::{Local, Timelike, Utc};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tauri_plugin_dialog::DialogExt;

const CREATE_CHAT_CANCEL_KEY: &str = "__create__";

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
/// `detail`：可选补充（如确认模型时的模型名）。
fn emit_chapter_progress(
    app: &tauri::AppHandle,
    novel_id: &str,
    step: &str,
    index: u32,
    total: u32,
) {
    emit_chapter_progress_detail(app, novel_id, step, index, total, None);
}

fn emit_chapter_progress_detail(
    app: &tauri::AppHandle,
    novel_id: &str,
    step: &str,
    index: u32,
    total: u32,
    detail: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "novelId": novel_id,
        "step": step,
        "index": index,
        "total": total,
    });
    if let Some(d) = detail.map(str::trim).filter(|s| !s.is_empty()) {
        payload["detail"] = serde_json::Value::String(d.to_string());
    }
    let _ = app.emit("chapter-progress", payload);
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
        app, &state.db, settings, system, user, model, novel_id, cancel, false,
    )
    .await
}

async fn llm_complete_ex(
    app: Option<&tauri::AppHandle>,
    db: &Db,
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
    let _ = db.add_token_usage(
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

/// 全应用共用：Chat 面板所选模型（`chat_model`），空则 `default_model`。
fn llm_model(settings: &AppSettings) -> String {
    task_model(&settings.chat_model, &settings.default_model)
}

/// 文生图提示词：`image_model`，空则跟 Chat。
fn image_prompt_model(settings: &AppSettings) -> String {
    task_model(&settings.image_model, &llm_model(settings))
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
    // 无对应 Key 时不暴露目录里的陈旧模型名（避免删光配置后 Chat 仍有选项）
    catalog.compat = s.all_compat_model_ids();
    catalog.deepseek.clear();
    catalog.grok.clear();
    catalog.kimi.clear();
    catalog.gemini.clear();
    catalog.claude.clear();
    Ok(SettingsView {
        compat_providers: s
            .compat_providers
            .iter()
            .map(|p| CompatProviderView {
                id: p.id.clone(),
                label: p.label.clone(),
                protocol: p.chat_protocol().to_string(),
                base_url: p.base_url.clone(),
                api_key_masked: mask_key(&p.api_key),
                api_key_configured: p.is_ready(),
                models: p.models.clone(),
            })
            .collect(),
        default_model: s.default_model,
        create_model: s.create_model,
        generate_model: s.generate_model,
        chat_model: s.chat_model,
        refine_model: s.refine_model,
        knowledge_model: s.knowledge_model,
        image_model: s.image_model,
        model_catalog: catalog,
        ui_locale: s.ui_locale,
        mcp_port: if s.mcp_port == 0 {
            crate::mcp::DEFAULT_MCP_PORT
        } else {
            s.mcp_port
        },
        mcp_enabled: s.mcp_enabled,
        mcp_lan: s.mcp_lan,
        ai_interaction_log: s.ai_interaction_log,
        ai_log_dir: crate::ai_log::dir().to_string_lossy().into_owned(),
        comfyui_url: if s.comfyui_url.trim().is_empty() {
            "http://127.0.0.1:8188".into()
        } else {
            s.comfyui_url
        },
        comfyui_workflow: s.comfyui_workflow,
        comfyui_prompt_node: s.comfyui_prompt_node,
        comfyui_image_workflow: s.comfyui_image_workflow,
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
            .map(|p| (p.id.clone(), (p.api_key.clone(), p.label.clone())))
            .collect();
        let mut next = Vec::with_capacity(providers.len());
        for p in providers {
            let id = if p.id.trim().is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                p.id.trim().to_string()
            };
            let (mut api_key, old_label) = prev.get(&id).cloned().unwrap_or_default();
            apply_optional_key(&mut api_key, p.api_key);
            let label = if old_label.trim().is_empty() {
                p.label.trim().to_string()
            } else {
                old_label
            };
            next.push(CompatProvider {
                id,
                label,
                protocol: {
                    crate::models::infer_compat_protocol(&p.protocol, &p.base_url).to_string()
                },
                base_url: p.base_url.trim().to_string(),
                api_key,
                models: p.models,
            });
        }
        s.compat_providers = next;
        // 用户已通过列表管理兼容提供商：清掉旧单项 Key，避免空列表被再次迁移灌回
        s.deepseek_api_key.clear();
        s.chatgpt_api_key.clear();
        s.grok_api_key.clear();
        s.kimi_api_key.clear();
        s.gemini_api_key.clear();
        s.claude_api_key.clear();
    }
    s.gemini_api_key.clear();
    s.claude_api_key.clear();
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
    // 空字符串表示跟随 Chat，与其它槽位「跳过空值」不同。
    if let Some(m) = input.image_model {
        s.image_model = m.trim().to_string();
    }
    if let Some(loc) = input.ui_locale {
        let t = loc.trim();
        if !t.is_empty() {
            s.ui_locale = t.to_string();
        }
    }
    let mut mcp_changed = false;
    if let Some(p) = input.mcp_port {
        let p = if (1024..=65535).contains(&p) {
            p
        } else {
            crate::mcp::DEFAULT_MCP_PORT
        };
        if s.mcp_port != p {
            s.mcp_port = p;
            mcp_changed = true;
        }
    }
    if let Some(en) = input.mcp_enabled {
        if s.mcp_enabled != en {
            s.mcp_enabled = en;
            mcp_changed = true;
        }
    }
    if let Some(lan) = input.mcp_lan {
        if s.mcp_lan != lan {
            s.mcp_lan = lan;
            mcp_changed = true;
        }
    }
    if let Some(on) = input.ai_interaction_log {
        s.ai_interaction_log = on;
    }
    if let Some(u) = input.comfyui_url {
        let u = u.trim().to_string();
        s.comfyui_url = if u.is_empty() {
            "http://127.0.0.1:8188".into()
        } else {
            u
        };
    }
    if let Some(w) = input.comfyui_workflow {
        s.comfyui_workflow = w;
    }
    if let Some(n) = input.comfyui_prompt_node {
        s.comfyui_prompt_node = n.trim().to_string();
    }
    if let Some(w) = input.comfyui_image_workflow {
        s.comfyui_image_workflow = w;
    }
    state.db.save_settings(&s).map_err(|e| e.to_string())?;
    crate::ai_log::set_enabled(s.ai_interaction_log);
    if mcp_changed {
        restart_mcp_from_state(&state);
    }
    get_settings(state)
}

#[tauri::command]
pub fn open_ai_log_dir() -> Result<String, String> {
    crate::ai_log::open_dir().map(|p| p.to_string_lossy().into_owned())
}

fn restart_mcp_from_state(state: &AppState) {
    let s = state.db.get_settings().unwrap_or_default();
    let ctx = crate::mcp::McpCtx {
        db: state.db.clone(),
        app: state.app_handle.clone(),
        selection: state.mcp.selection.clone(),
    };
    crate::mcp::restart(
        ctx,
        state.mcp.clone(),
        s.mcp_enabled,
        s.mcp_port,
        s.mcp_lan,
    );
}

#[tauri::command]
pub fn set_workspace_selection(
    state: State<'_, AppState>,
    novel_id: Option<String>,
    node_id: Option<String>,
) {
    let n = novel_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let id = node_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match (n, id) {
        (Some(novel_id), Some(node_id)) => state.mcp.set_workspace_selection(novel_id, node_id),
        // Leaving one workspace: only clear if selection still belongs to that novel
        // (param change / remount race must not wipe the newly opened book).
        (Some(novel_id), None) => state.mcp.clear_workspace_selection_if(novel_id),
        _ => state.mcp.clear_workspace_selection(),
    }
}

#[tauri::command]
pub fn get_mcp_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let s = state.db.get_settings().map_err(|e| e.to_string())?;
    Ok(state.mcp.status_json(s.mcp_lan))
}

#[tauri::command]
pub fn restart_mcp_server(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    restart_mcp_from_state(&state);
    let s = state.db.get_settings().map_err(|e| e.to_string())?;
    Ok(state.mcp.status_json(s.mcp_lan))
}

#[tauri::command]
pub async fn refresh_model_catalog(state: State<'_, AppState>) -> Result<ModelCatalog, String> {
    crate::catalog::refresh(&state.db)
        .await
        .map_err(|e| e.to_string())
}

/// Probe provider `{base}/models` (or protocol-specific list) with a raw key.
#[tauri::command]
pub async fn fetch_compat_models(
    base_url: String,
    api_key: String,
    protocol: Option<String>,
) -> Result<Vec<String>, String> {
    let proto = crate::models::infer_compat_protocol(
        protocol.as_deref().unwrap_or(""),
        base_url.trim(),
    );
    let key = api_key.trim();
    if key.contains('*') {
        return Err("api key required".into());
    }
    if key.is_empty() && !crate::models::protocol_allows_empty_key(proto) {
        return Err("api key required".into());
    }
    if base_url.trim().is_empty() {
        return Err("base url required".into());
    }
    crate::catalog::fetch_provider_models(proto, base_url.trim(), key)
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
    let _ = crate::knowledge_vec::delete_book(&id).await;
    state.db.delete_knowledge(&id).map_err(|e| e.to_string())?;
    let _ = state.db.unlink_knowledge_from_novels(&id);
    Ok(())
}

#[tauri::command]
pub async fn import_knowledge_text(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let src = load_source(&path).map_err(|e| e.to_string())?;
    ingest_knowledge_source(Some(&app), &state.db, src, path, genres, None).await
}

#[tauri::command]
pub async fn import_knowledge_url(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let src = load_source_from_url(&url).await.map_err(|e| e.to_string())?;
    ingest_knowledge_source(Some(&app), &state.db, src, url, genres, None).await
}

fn merge_knowledge_genres(user: Vec<String>, from_src: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for g in user.into_iter().chain(from_src) {
        let g = g.trim().to_string();
        if g.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x == &g) {
            out.push(g);
        }
    }
    out
}

/// 无 AI：解析元数据 + 1000/20 分块。
fn prepare_knowledge_ingest(
    src: &crate::knowledge::ParsedSource,
    genres: Vec<String>,
) -> (String, String, Vec<String>, Vec<String>) {
    let title = src.title.trim().to_string();
    let author = src.author.trim().to_string();
    let genres = merge_knowledge_genres(genres, src.subjects.clone());
    let chunks = build_knowledge_chunks(src);
    (title, author, genres, chunks)
}

pub(crate) async fn ingest_knowledge_source(
    app: Option<&tauri::AppHandle>,
    db: &crate::db::Db,
    src: crate::knowledge::ParsedSource,
    source_path: String,
    genres: Vec<String>,
    title_override: Option<String>,
) -> Result<KnowledgeBook, String> {
    let (title, author, genres, chunks) = prepare_knowledge_ingest(&src, genres);
    if chunks.is_empty() {
        return Err("未能从源文件拆出内容分段".into());
    }
    let title = title_override
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(title);
    let book = KnowledgeBook {
        id: uuid::Uuid::new_v4().to_string(),
        title: if title.is_empty() {
            "未命名".into()
        } else {
            title
        },
        author: if author.is_empty() {
            "未知作者".into()
        } else {
            author
        },
        genres,
        source_path,
        extract_prompt: String::new(),
        created_at: Utc::now().to_rfc3339(),
        chunk_count: chunks.len() as i64,
        archived: false,
    };
    db.insert_knowledge(&book, &chunks)
        .map_err(|e| e.to_string())?;
    maybe_index_book_embeddings(app, db, &book.id).await?;
    Ok(book)
}

async fn maybe_index_book_embeddings(
    app: Option<&tauri::AppHandle>,
    db: &crate::db::Db,
    book_id: &str,
) -> Result<(), String> {
    let _ = crate::knowledge_vec::delete_book(book_id).await;
    crate::kb_context::index_book_embeddings(db, book_id, app.cloned())
        .await
        .map_err(|e| format!("本地向量索引失败: {e}"))?;
    Ok(())
}

fn is_http_source(path: &str) -> bool {
    let p = path.trim().to_ascii_lowercase();
    p.starts_with("http://") || p.starts_with("https://")
}

/// Re-read source (file or URL) and replace stored knowledge chunks for an existing book.
#[tauri::command]
pub async fn reextract_knowledge(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: Option<String>,
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
    let src = if from_url {
        load_source_from_url(&source_path)
            .await
            .map_err(|e| e.to_string())?
    } else {
        if !Path::new(&source_path).is_file() {
            return Err("源文件不存在或已移动，请重新选择文件".into());
        }
        load_source(&source_path).map_err(|e| e.to_string())?
    };
    let (title, author, genres, chunks) = prepare_knowledge_ingest(&src, book.genres.clone());
    if chunks.is_empty() {
        return Err("未能从源文件拆出内容分段".into());
    }
    let _ = crate::knowledge_vec::delete_book(&id).await;
    state
        .db
        .replace_knowledge_content(
            &id,
            &title,
            &author,
            &genres,
            &source_path,
            book.extract_prompt.as_str(),
            &chunks,
        )
        .map_err(|e| e.to_string())?;
    maybe_index_book_embeddings(Some(&app), &state.db, &id).await?;
    maybe_index_book_embeddings(Some(&app), &state.db, &id).await?;
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
pub async fn knowledge_index_status(
    state: State<'_, AppState>,
    book_id: String,
) -> Result<KnowledgeIndexStatus, String> {
    let book = state
        .db
        .get_knowledge(&book_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    let embedding_count = crate::knowledge_vec::count_book(&book_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(KnowledgeIndexStatus {
        book_id,
        chunk_count: book.chunk_count,
        embedding_count,
    })
}

#[tauri::command]
pub async fn rebuild_knowledge_index(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    book_id: String,
) -> Result<KnowledgeIndexStatus, String> {
    let book = state
        .db
        .get_knowledge(&book_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "知识库不存在".to_string())?;
    let _ = crate::knowledge_vec::delete_book(&book_id).await;
    let n = crate::kb_context::index_book_embeddings(&state.db, &book_id, Some(app))
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
        &state.db,
        &input.title,
        &input.synopsis,
        &input.knowledge_ids,
        &input.knowledge_strategy,
        2000,
        3000,
        20,
        None,
        None,
        input.features,
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

/// 细纲条数 + 每条扩写约字数，使合计贴近目标中位。
/// 旧规则一律 5–12 条：2000 字章被拆成十几场就会首轮严重超字。
pub(crate) fn detailed_outline_pace(wmin: u32, wmax: u32) -> (u32, u32, u32) {
    let (wmin, wmax) = normalize_word_range(wmin, wmax);
    let mid = wmin.saturating_add(wmax) / 2;
    // ponytail: ~320 字/场面；升级可按题材改 PER
    const PER: u32 = 320;
    let n = (mid / PER).clamp(4, 10);
    let lo = n.saturating_sub(1).max(4);
    let hi = (n + 1).min(10);
    let per = (mid / n).max(180);
    (lo, hi, per)
}

pub(crate) fn chapter_length_feedback(words: u32, wmin: u32, wmax: u32) -> serde_json::Value {
    let (wmin, wmax) = normalize_word_range(wmin, wmax);
    let in_band = word_count_ok(words, wmin, wmax);
    let note = if in_band {
        "字数已在允许区间，禁止再为篇幅改正文。"
    } else if words > wmax.saturating_add(WORD_COUNT_TOLERANCE) {
        "偏长：只删冗一次后交卷，禁止整章重写。"
    } else {
        "偏短：只补关键场面一次后交卷，禁止整章重写。"
    };
    serde_json::json!({
        "ok": true,
        "word_count": words,
        "word_count_min": wmin,
        "word_count_max": wmax,
        "in_band": in_band,
        "ai_guidance": note,
    })
}

#[cfg(test)]
mod outline_pace_tests {
    use super::{chapter_length_feedback, detailed_outline_pace};

    #[test]
    fn pace_scales_with_chapter_target() {
        let (lo, hi, per) = detailed_outline_pace(2000, 3000);
        assert_eq!((lo, hi), (6, 8));
        assert!(per >= 300 && per <= 420, "{per}");
        let (lo4, hi4, _) = detailed_outline_pace(4000, 5000);
        assert!(hi4 >= hi, "{lo4}-{hi4} vs {lo}-{hi}");
        assert!(lo4 >= 8, "{lo4}");
    }

    #[test]
    fn length_feedback_stops_in_band_rewrite() {
        let v = chapter_length_feedback(2500, 2000, 3000);
        assert_eq!(v["in_band"], true);
        assert!(v["ai_guidance"].as_str().unwrap().contains("禁止再为篇幅"));
        let over = chapter_length_feedback(5000, 2000, 3000);
        assert_eq!(over["in_band"], false);
        assert!(over["ai_guidance"].as_str().unwrap().contains("删冗一次"));
    }
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

#[tauri::command]
pub fn update_novel_features(
    state: State<'_, AppState>,
    novel_id: String,
    features: crate::models::NovelFeatures,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    novel.features = features;
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

pub(crate) fn create_novel_with_tree(
    db: &crate::db::Db,
    title: &str,
    synopsis: &str,
    knowledge_ids: &[String],
    knowledge_strategy: &str,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
    _chapters: Option<&[serde_json::Value]>,
    characters: Option<&[serde_json::Value]>,
    features: crate::models::NovelFeatures,
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
        knowledge_strategy: knowledge_strategy.to_string(),
        canon_mode: "reference".into(),
        archived: false,
        word_count_min: wmin,
        word_count_max: wmax,
        chapter_count: chapters_n,
        features,
        created_at: now.clone(),
        updated_at: now,
    };
    db.upsert_novel(&novel).map_err(|e| e.to_string())?;
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
        detailed_outline: vec![],
        character: None,
        knowledge: None,
        side_plot: None,
        volume: None,
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
                detailed_outline: vec![],
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
                    age: c
                        .get("age")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    constraints: c
                        .get("constraints")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    ..Default::default()
                }),
                knowledge: None,
                side_plot: None,
                volume: None,
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

    let mut tree = NovelTree {
        novel_id: novel_id.into(),
        nodes,
        edges,
    };
    crate::write_prompts::ensure_write_prompts_card(&mut tree);
    crate::tree_layout::apply_auto_layout(&mut tree);
    tree
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

pub(crate) fn last_chapter_anchor(tree: &NovelTree) -> (String, f64, f64) {
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
        detailed_outline: vec![],
        character: None,
        knowledge: None,
        side_plot: None,
        volume: None,
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
    let chat_model = llm_model(&settings);
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
        crate::tree_layout::apply_auto_layout(tree);
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
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: Some(SidePlotMeta::active()),
            volume: None,
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
                    detailed_outline: vec![],
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
                        age: c
                            .get("age")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        constraints: c
                            .get("constraints")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        ..Default::default()
                    }),
                    knowledge: None,
                    side_plot: None,
                    volume: None,
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
    knowledge_ids.retain(|id| !crate::write_prompts::is_write_prompt_excluded_id(tree, id));

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

    let mut kn_rows: Vec<(u8, String, String, usize)> = Vec::new();
    let mut inject_bumps = 0usize;
    for id in &knowledge_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let body = crate::core_laws_fmt::knowledge_card_inject_body(tree, n);
            let body = if body.trim().is_empty() {
                if loc.is_zh() {
                    "（尚未提取特征）".into()
                } else {
                    "(features not extracted)".into()
                }
            } else {
                body
            };
            let per = crate::core_laws_fmt::knowledge_card_inject_cap(n);
            if per > crate::kb_context::KNOWLEDGE_CARD_EXTRACT_CAP {
                inject_bumps += 1;
            }
            let pri = crate::core_laws_fmt::knowledge_card_inject_priority(n);
            kn_rows.push((pri, n.label.clone(), body, per));
        }
    }
    kn_rows.sort_by_key(|r| r.0);
    let kn_cards: Vec<(String, String, usize)> = kn_rows
        .into_iter()
        .map(|(_, l, b, p)| (l, b, p))
        .collect();
    let mut total = crate::kb_context::ROOT_KNOWLEDGE_TOTAL_CAP;
    for _ in 0..inject_bumps {
        total = crate::core_laws_fmt::knowledge_inject_total(total);
    }
    let knowledge =
        crate::kb_context::format_knowledge_cards_capped_var(&kn_cards, total, loc.is_zh());

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
    let chat_model = llm_model(&settings);
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
        &state.db,
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
                let n0 = tree.nodes.len();
                apply_chapter_outlines(tree, &list, mode);
                if tree.nodes.len() > n0 {
                    crate::tree_layout::apply_auto_layout(tree);
                }
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

#[tauri::command]
pub async fn create_novel_chat(
    state: State<'_, AppState>,
    input: NovelCreateChatInput,
) -> Result<NovelCreateChatResult, String> {
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, input.model.as_deref());
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

    let model = llm_model(&settings);

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
            &state.db,
            &title,
            &synopsis,
            &[],
            &strategy,
            wmin,
            wmax,
            chapter_count,
            None,
            characters.map(|v| v.as_slice()),
            Default::default(),
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
            &state.db,
            &fallback_title,
            fallback_syn,
            &[],
            fallback_strat,
            2000,
            3000,
            chapter_count,
            None,
            None,
            Default::default(),
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

fn find_matching_pair(chars: &[char], start: usize, open: char, close: char) -> Option<usize> {
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
        if c == '"' {
            in_str = true;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

fn parse_json_string_array(s: &str) -> Option<Vec<String>> {
    let t = normalize_llm_json_text(s);
    let soft = strip_trailing_commas(t.trim());
    let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&soft) else {
        return None;
    };
    let items: Vec<String> = arr
        .iter()
        .filter_map(|v| {
            v.as_str()
                .map(|s| s.trim().to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn json_value_to_beat(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = ["who", "does", "what", "result", "turn", "谁", "做", "结果"]
                .iter()
                .filter_map(|k| {
                    map.get(*k)
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .collect();
            if parts.len() >= 2 {
                return Some(parts.join("；"));
            }
            const KEYS: &[&str] = &[
                "item",
                "beat",
                "text",
                "scene",
                "content",
                "outline",
                "细纲",
                "内容",
                "要点",
                "what",
                "desc",
                "description",
                "summary",
            ];
            for k in KEYS {
                if let Some(s) = map
                    .get(*k)
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    return Some(s.to_string());
                }
            }
            if parts.len() == 1 {
                return Some(parts[0].clone());
            }
            let strs: Vec<String> = map
                .values()
                .filter_map(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() >= 4)
                .map(|s| s.to_string())
                .collect();
            if strs.is_empty() {
                None
            } else {
                Some(strs.join("；"))
            }
        }
        _ => None,
    }
}

fn beats_from_json_map(slice: &str) -> Option<Vec<String>> {
    let v = serde_json::from_str::<serde_json::Value>(&strip_trailing_commas(slice)).ok()?;
    let obj = v.as_object()?;
    let mut numbered: Vec<(u32, String)> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for (k, val) in obj {
        let Some(beat) = json_value_to_beat(val) else {
            continue;
        };
        if let Ok(n) = k.parse::<u32>() {
            numbered.push((n, beat));
        } else {
            rest.push(beat);
        }
    }
    numbered.sort_by_key(|p| p.0);
    let mut items: Vec<String> = numbered.into_iter().map(|p| p.1).collect();
    items.extend(rest);
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn collect_beats_from_array_body(rest: &str) -> Vec<String> {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    if chars.first() == Some(&'[') {
        i = 1;
    }
    let mut out = Vec::new();
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= chars.len() || chars[i] == ']' {
            break;
        }
        if chars[i] == '"' {
            let mut j = i + 1;
            let mut escape = false;
            let mut closed = false;
            while j < chars.len() {
                if escape {
                    escape = false;
                    j += 1;
                    continue;
                }
                if chars[j] == '\\' {
                    escape = true;
                    j += 1;
                    continue;
                }
                if chars[j] == '"' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                break;
            }
            let slice: String = chars[i..=j].iter().collect();
            if let Ok(s) = serde_json::from_str::<String>(&slice) {
                let t = s.trim().to_string();
                if !t.is_empty() {
                    out.push(t);
                }
            }
            i = j + 1;
            continue;
        }
        if chars[i] == '{' {
            match find_matching_pair(&chars, i, '{', '}') {
                Some(end) => {
                    let slice: String = chars[i..=end].iter().collect();
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&slice) {
                        if let Some(b) = json_value_to_beat(&v) {
                            out.push(b);
                        }
                    }
                    i = end + 1;
                }
                None => break,
            }
            continue;
        }
        i += 1;
    }
    out
}

/// GLM etc. often emit `"detailed_outline": [ … ]` without wrapping `{…}`.
fn extract_named_string_array(text: &str, keys: &[&str]) -> Option<Vec<String>> {
    let s = normalize_llm_json_text(text);
    for key in keys {
        let needle = format!("\"{key}\"");
        let Some(i) = s.find(&needle) else { continue };
        let rest = s[i + needle.len()..]
            .trim_start()
            .trim_start_matches(':')
            .trim_start();
        let chars: Vec<char> = rest.chars().collect();
        if chars.first() == Some(&'{') {
            if let Some(end) = find_matching_pair(&chars, 0, '{', '}') {
                let slice: String = chars[..=end].iter().collect();
                if let Some(items) = beats_from_json_map(&slice) {
                    return Some(items);
                }
            }
            continue;
        }
        if chars.first() != Some(&'[') {
            continue;
        }
        if let Some(end) = find_matching_pair(&chars, 0, '[', ']') {
            let slice: String = chars[..=end].iter().collect();
            if let Some(items) = parse_json_string_array(&slice) {
                return Some(items);
            }
            let soft = strip_trailing_commas(slice.trim());
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&soft) {
                let items: Vec<String> = arr.iter().filter_map(json_value_to_beat).collect();
                if !items.is_empty() {
                    return Some(items);
                }
            }
        }
        let items = collect_beats_from_array_body(rest);
        if items.len() >= 2 {
            return Some(items);
        }
    }
    None
}

fn outline_beat_from_line(l: &str) -> Option<String> {
    let raw = l.trim();
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw.trim_end_matches(',').trim();
    if matches!(trimmed, "{" | "}" | "[" | "]" | "}," | "]," | ",") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("detailed_outline") || lower.contains("\"items\"") || lower.contains("\"细纲\"")
    {
        return None;
    }
    if trimmed.starts_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(trimmed) {
            let t = s.trim().to_string();
            if t.chars().count() >= 4 {
                return Some(t);
            }
        }
        return None;
    }
    let t = trimmed
        .trim_start_matches(|c: char| {
            c.is_ascii_digit()
                || c == '.'
                || c == ')'
                || c == '、'
                || c == '．'
                || c == '）'
                || c == '-'
                || c == '•'
                || c == '*'
        })
        .trim();
    if t.chars().count() >= 4 {
        Some(t.to_string())
    } else {
        None
    }
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
        match find_matching_pair(&chars, i, '{', '}') {
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
    if settings.resolve_compat(model).is_some_and(|ep| ep.is_ready()) {
        if let Ok(ex) = crate::llm_extract::extract::<crate::llm_extract::OutlinesExtract>(
            settings,
            model,
            &prompts::repair_outlines_json_system(loc),
            reply,
        )
        .await
        {
            let arr: Vec<serde_json::Value> = ex
                .outlines
                .into_iter()
                .map(|i| {
                    let mut v = serde_json::json!({ "outline": i.outline });
                    if let Some(n) = i.n {
                        v["n"] = serde_json::json!(n);
                    }
                    if let Some(label) = i.label {
                        v["label"] = serde_json::json!(label);
                    }
                    v
                })
                .collect();
            let list = usable_outline_list(arr);
            if !list.is_empty() {
                return Ok((reply.to_string(), list));
            }
        }
    }
    let system = prompts::repair_outlines_json_system(loc);
    let user = prompts::repair_outlines_json_user(loc, nums, reply);
    let (fixed, _) = llm_complete_ex(
        app,
        &state.db,
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

/// 把 Agent 传入的 node_id 收成树上真实 id。
/// 接受：已有 id、`root`/`novel`、唯一标题、「第N章」/章号（`3` / `三`）。
pub(crate) fn resolve_node_ref(tree: &NovelTree, spec: &str) -> Result<String, String> {
    let t = spec.trim();
    if tree.nodes.iter().any(|n| n.id == t) {
        return Ok(t.to_string());
    }
    if t.is_empty() || t.eq_ignore_ascii_case("root") || t.eq_ignore_ascii_case("novel") {
        return tree
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Novel))
            .map(|n| n.id.clone())
            .ok_or_else(|| "找不到小说根节点".into());
    }
    let exact: Vec<&TreeNode> = tree.nodes.iter().filter(|n| n.label.trim() == t).collect();
    if exact.len() == 1 {
        return Ok(exact[0].id.clone());
    }
    if exact.len() > 1 {
        let ids: Vec<&str> = exact.iter().map(|n| n.id.as_str()).collect();
        return Err(format!("标题「{t}」对应多个节点：{}", ids.join("、")));
    }
    let lower = t.to_lowercase();
    let ci: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| n.label.trim().to_lowercase() == lower)
        .collect();
    if ci.len() == 1 {
        return Ok(ci[0].id.clone());
    }
    // 不要用 extract_small_chapter_nums：失败的 UUID 里常带数字，会误命中章号
    if let Some(num) = chapter_number_from_label(t).or_else(|| parse_chapter_num_token(t)) {
        if let Some(id) = existing_chapter_nums(tree).get(&num) {
            return Ok(id.clone());
        }
    }
    Err(format!(
        "无法找到节点「{t}」。node_id 须为树上 id，章节也可传「第N章」或唯一标题。{}",
        chapter_id_hint(tree)
    ))
}

fn chapter_id_hint(tree: &NovelTree) -> String {
    let chs = sorted_chapter_nodes(tree);
    if chs.is_empty() {
        return "树上暂无章节。".into();
    }
    let n = chs.len();
    let shown: Vec<String> = chs
        .into_iter()
        .take(12)
        .map(|c| format!("{} ({})", c.label, c.id))
        .collect();
    let extra = if n > 12 {
        format!(" 等共{n}章")
    } else {
        String::new()
    };
    format!("可用章节：{}{extra}", shown.join("；"))
}

/// `link_to`：已有节点 id 原样返回；`root` / `novel` / 空串解析为小说根（根 id 不是字面 `"root"`）。
/// 也接受唯一标题与「第N章」。
pub(crate) fn resolve_link_host(tree: &NovelTree, spec: &str) -> Result<String, String> {
    resolve_node_ref(tree, spec)
}

/// 公共知识卡挂到哪：章/根用自身；人物/剧情/知识卡用其宿主；否则小说根。
pub(crate) fn knowledge_attach_host(tree: &NovelTree, selected_id: Option<&str>) -> Result<String, String> {
    let root_id = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
        .ok_or_else(|| "找不到小说根节点".to_string())?;
    let Some(id) = selected_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(root_id);
    };
    if id.eq_ignore_ascii_case("root") || id.eq_ignore_ascii_case("novel") {
        return Ok(root_id);
    }
    let Some(n) = tree.nodes.iter().find(|n| n.id == id) else {
        return Err(format!("节点不存在: {id}"));
    };
    let host = match n.kind {
        NodeKind::Novel | NodeKind::Volume | NodeKind::Chapter => n.id.clone(),
        NodeKind::SidePlot => tree
            .nodes
            .iter()
            .find(|h| {
                matches!(h.kind, NodeKind::Chapter | NodeKind::Novel | NodeKind::Volume)
                    && h.linked_side_plot_ids.iter().any(|x| x == id)
            })
            .map(|h| h.id.clone())
            .unwrap_or_else(|| root_id.clone()),
        NodeKind::Knowledge => tree
            .nodes
            .iter()
            .find(|h| h.linked_knowledge_ids.iter().any(|x| x == id))
            .map(|h| h.id.clone())
            .unwrap_or(root_id),
        NodeKind::Character => tree
            .nodes
            .iter()
            .find(|h| h.linked_character_ids.iter().any(|x| x == id))
            .map(|h| h.id.clone())
            .unwrap_or(root_id),
    };
    Ok(host)
}

pub fn upsert_public_knowledge_card_inner(
    db: &Db,
    id: Option<String>,
    title: String,
    book_ids: Vec<String>,
    extract_prompt: String,
    extracted: String,
) -> Result<PublicKnowledgeCard, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    let now = Utc::now().to_rfc3339();
    let existing = id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|id| db.get_public_knowledge_card(id).ok().flatten());
    let card = PublicKnowledgeCard {
        id: existing
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        title,
        book_ids,
        extract_prompt,
        extracted,
        created_at: existing
            .as_ref()
            .map(|c| c.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        archived: existing.as_ref().map(|c| c.archived).unwrap_or(false),
    };
    db.upsert_public_knowledge_card(&card)
        .map_err(|e| e.to_string())?;
    Ok(card)
}

pub fn add_public_knowledge_card_inner(
    db: &Db,
    novel_id: &str,
    public_id: &str,
    host_spec: Option<&str>,
) -> Result<(NovelTree, String), String> {
    let card = db
        .get_public_knowledge_card(public_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "公共知识卡不存在".to_string())?;
    if card.archived {
        return Err("该公共知识卡已归档".into());
    }
    let mut tree = get_tree(novel_id.to_string())?;
    let host_id = knowledge_attach_host(&tree, host_spec)?;
    let payload = KnowledgeCardPayload {
        book_ids: card.book_ids,
        extract_prompt: card.extract_prompt,
        extracted: card.extracted.clone(),
        from_canon: false,
        slot: String::new(),
        core_laws: None,
        spatiotemporal: None,
        world_axiom: None,
        key_location: None,
        social_power: None,
        world_race: None,
        major_faction: None,
        existence: None,
        info_flow: None,
        history_culture: None,
        world_religion: None,
        major_event: None,
        surface_setting: None,
        story_engine: None,
        fulfillment_system: None,
        constraint_redlines: None,
    };
    let kid = format!("kb-{}", uuid::Uuid::new_v4());
    let kn = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Knowledge))
        .count();
    tree.nodes.push(TreeNode {
        id: kid.clone(),
        kind: NodeKind::Knowledge,
        label: card.title,
        outline: payload.extracted.chars().take(200).collect(),
        detailed_outline: vec![],
        character: None,
        knowledge: Some(payload),
        side_plot: None,
        volume: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition {
            x: 40.0,
            y: 80.0 + kn as f64 * 140.0,
        },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    let exists = tree.edges.iter().any(|e| {
        e.source == host_id && e.target == kid || e.source == kid && e.target == host_id
    });
    if !exists {
        tree.edges.push(TreeEdge {
            id: format!("e-{host_id}-{kid}"),
            source: host_id.clone(),
            target: kid.clone(),
            kind: "knowledge".into(),
            source_handle: Some("left".into()),
            target_handle: Some("right".into()),
            label: String::new(),
        });
    }
    if let Some(host) = tree.nodes.iter_mut().find(|n| n.id == host_id) {
        if !host.linked_knowledge_ids.iter().any(|x| x == &kid) {
            host.linked_knowledge_ids.push(kid.clone());
        }
    }
    crate::tree_layout::apply_auto_layout(&mut tree);
    save_tree(tree.clone())?;
    Ok((tree, kid))
}

/// 把指向不存在节点 `"root"` / `"novel"` 的边改挂到真正的小说根，并补 `linked_*`。
pub(crate) fn repair_root_alias_links(tree: &mut NovelTree) -> bool {
    let Some(root_id) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
    else {
        return false;
    };
    let node_ids: Vec<String> = tree.nodes.iter().map(|n| n.id.clone()).collect();
    let mut changed = false;
    for e in &mut tree.edges {
        for end in [&mut e.source, &mut e.target] {
            if *end == root_id {
                continue;
            }
            let missing = !node_ids.iter().any(|id| id == end);
            if missing && (end.eq_ignore_ascii_case("root") || end.eq_ignore_ascii_case("novel")) {
                *end = root_id.clone();
                changed = true;
            }
        }
    }
    if !changed {
        return false;
    }
    let extras: Vec<(String, String)> = tree
        .edges
        .iter()
        .filter_map(|e| {
            let other = if e.source == root_id {
                e.target.as_str()
            } else if e.target == root_id {
                e.source.as_str()
            } else {
                return None;
            };
            Some((e.kind.clone(), other.to_string()))
        })
        .collect();
    if let Some(root) = tree.nodes.iter_mut().find(|n| n.id == root_id) {
        for (kind, card) in extras {
            let list = match kind.as_str() {
                "knowledge" => &mut root.linked_knowledge_ids,
                "character" => &mut root.linked_character_ids,
                "side_plot" => &mut root.linked_side_plot_ids,
                _ => continue,
            };
            if !list.iter().any(|id| id == &card) {
                list.push(card);
            }
        }
    }
    true
}

#[tauri::command]
pub fn get_tree(novel_id: String) -> Result<NovelTree, String> {
    let raw = fs::read_to_string(novel_tree_path(&novel_id)).map_err(|e| e.to_string())?;
    let mut tree: NovelTree = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut changed = false;
    if repair_root_alias_links(&mut tree) {
        changed = true;
    }
    if repair_volume_payload(&mut tree) {
        changed = true;
    }
    if crate::write_prompts::ensure_write_prompts_card(&mut tree) {
        changed = true;
    }
    if changed {
        let _ = save_tree(tree.clone());
    }
    Ok(tree)
}

/// 分卷数据仅存 `volume` 字段；旧数据若只有 outline 则一次性迁入 volume 并清空 outline。
fn repair_volume_payload(tree: &mut NovelTree) -> bool {
    let mut changed = false;
    for n in &mut tree.nodes {
        if !matches!(n.kind, NodeKind::Volume) {
            continue;
        }
        let payload = n.volume.clone().unwrap_or_default();
        if !crate::volume_fmt::volume_payload_nonempty(&payload) && !n.outline.trim().is_empty() {
            let migrated = crate::volume_fmt::parse_volume_from_formatted(&n.outline);
            if crate::volume_fmt::volume_payload_nonempty(&migrated) {
                n.volume = Some(migrated);
                n.outline = String::new();
                changed = true;
                continue;
            }
        }
        if n.volume.is_none() {
            n.volume = Some(crate::models::VolumePayload::default());
            changed = true;
        }
    }
    changed
}

#[tauri::command]
pub fn save_tree(tree: NovelTree) -> Result<(), String> {
    fs::write(
        novel_tree_path(&tree.novel_id),
        serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 前端 JSON 字符串落盘（世界观 Chat 等大批量改树时更稳）。
#[tauri::command]
pub fn save_tree_json(novel_id: String, tree_json: String) -> Result<NovelTree, String> {
    let mut tree: NovelTree =
        serde_json::from_str(tree_json.trim()).map_err(|e| format!("树 JSON 无效：{e}"))?;
    if tree.novel_id.trim().is_empty() {
        tree.novel_id = novel_id.clone();
    } else if tree.novel_id != novel_id {
        return Err("树 novel_id 与当前小说不一致".into());
    }
    if !tree.nodes.iter().any(|n| matches!(n.kind, NodeKind::Novel)) {
        return Err("树缺少根节点".into());
    }
    save_tree(tree.clone())?;
    Ok(tree)
}

/// 从磁盘树删除非根节点（章/人/剧情/知识），并清掉边与 linked_*。
#[tauri::command]
pub fn delete_tree_card(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<NovelTree, String> {
    delete_tree_card_inner(&state.db, novel_id, node_id)
}

pub(crate) fn delete_tree_card_inner(
    db: &crate::db::Db,
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
    if matches!(kind, NodeKind::Knowledge)
        && tree.nodes.iter().any(|n| {
            n.id == node_id
                && crate::write_prompts::is_write_prompts_slot(crate::write_prompts::knowledge_slot(
                    n,
                ))
        })
    {
        return Err("生成/精修卡不能删除".into());
    }

    // 删分卷：卷下首章改挂到根（或分卷上游），不删章节正文
    if matches!(kind, NodeKind::Volume) {
        let root_id = tree
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Novel))
            .map(|n| n.id.clone())
            .ok_or_else(|| "根节点不存在".to_string())?;
        let preds: Vec<(String, Option<String>, Option<String>)> = tree
            .edges
            .iter()
            .filter(|e| {
                (e.kind == "volume" || e.kind == "chapter") && e.target == node_id
            })
            .map(|e| (e.source.clone(), e.source_handle.clone(), e.target_handle.clone()))
            .collect();
        let attach_to = preds
            .first()
            .map(|(s, _, _)| s.clone())
            .unwrap_or(root_id);
        let attach_sh = preds
            .first()
            .and_then(|(_, sh, _)| sh.clone())
            .or_else(|| Some("bottom".into()));
        let children: Vec<(String, Option<String>)> = tree
            .edges
            .iter()
            .filter(|e| e.kind == "chapter" && e.source == node_id)
            .map(|e| (e.target.clone(), e.target_handle.clone()))
            .collect();
        for (child, th) in &children {
            let exists = tree.edges.iter().any(|e| {
                (e.kind == "chapter" || e.kind == "volume")
                    && e.source == attach_to
                    && e.target == *child
            });
            if exists {
                continue;
            }
            tree.edges.push(TreeEdge {
                id: format!("e-{attach_to}-{child}"),
                source: attach_to.clone(),
                target: child.clone(),
                kind: "chapter".into(),
                source_handle: attach_sh.clone(),
                target_handle: th.clone().or_else(|| Some("top".into())),
                label: String::new(),
            });
        }
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
        let _ = db.delete_chapter_memory(&novel_id, &node_id);
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
    let _ = db.delete_chat_for_node(&novel_id, &node_id);
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

#[tauri::command]
pub async fn play_chapter_tts(
    app: tauri::AppHandle,
    text: String,
    speed: Option<i32>,
) -> Result<(), String> {
    crate::chapter_tts::play_text(&app, &text, speed.unwrap_or(1)).await
}

#[tauri::command]
pub fn stop_chapter_tts() {
    crate::chapter_tts::stop();
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
pub fn get_chapter_memory_inner(
    db: &Db,
    novel_id: &str,
    node_id: &str,
) -> Result<Vec<String>, String> {
    let rows = db
        .list_chapter_memory_for_nodes(novel_id, &[node_id.to_string()])
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(_, c)| c).collect())
}

#[tauri::command]
pub fn get_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<String>, String> {
    get_chapter_memory_inner(&state.db, &novel_id, &node_id)
}

/// 全书章节记忆：按结构树章节顺序分组（仅含已有记忆的章）。
pub fn list_all_chapter_memory_inner(
    db: &Db,
    novel_id: &str,
) -> Result<Vec<ChapterMemoryGroup>, String> {
    let tree = get_tree(novel_id.to_string())?;
    let chapters = sorted_chapter_nodes(&tree);
    let rows = db.list_chapter_memory(novel_id).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn list_all_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<Vec<ChapterMemoryGroup>, String> {
    list_all_chapter_memory_inner(&state.db, &novel_id)
}

/// 手动覆盖/清空某章记忆（`items` 为空则清除）。同步 SQLite 列表 + Lance 向量。
pub async fn set_chapter_memory_inner(
    db: &Db,
    novel_id: &str,
    node_id: &str,
    items: Vec<String>,
) -> Result<(), String> {
    if items.is_empty() {
        db.delete_chapter_memory(novel_id, node_id)
            .map_err(|e| e.to_string())?;
        chapter_memory::delete_lance(novel_id, node_id)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        db.replace_chapter_memory(novel_id, node_id, &items)
            .map_err(|e| e.to_string())?;
        chapter_memory::upsert_lance(novel_id, node_id, &items)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_chapter_memory(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    items: Vec<String>,
) -> Result<(), String> {
    set_chapter_memory_inner(&state.db, &novel_id, &node_id, items).await
}

/// 汇总本章大纲 + 树上链接的人物卡 / 剧情卡 / 知识卡（edges 与 linked_* 并集）+ 相关人物关系。
fn chapter_context(
    tree: &NovelTree,
    novel_id: &str,
    node_id: &str,
    loc: PromptLocale,
    slim: bool,
) -> (String, String) {
    let labels = prompts::chapter_context_labels(loc);
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let outline = node.map(|n| n.outline.clone()).unwrap_or_default();
    let detailed = node
        .map(|n| n.detailed_outline.clone())
        .unwrap_or_default();
    let label = node.map(|n| n.label.clone()).unwrap_or_default();

    let mut char_ids: Vec<String> = node
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    let mut plot_ids = crate::tree_links::chapter_local_plot_ids(tree, node_id);
    let inherited_plots = crate::tree_links::chapter_inherited_plot_ids(tree, node_id);
    let root_plot_ids = crate::tree_links::root_plot_ids(tree);
    for pid in &inherited_plots {
        if !plot_ids.iter().any(|id| id == pid) {
            plot_ids.push(pid.clone());
        }
    }
    let knowledge_ids = crate::tree_links::chapter_write_knowledge_ids(tree, node_id);

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
        if matches!(other.kind, NodeKind::Character)
            && !char_ids.iter().any(|id| id == other_id)
        {
            char_ids.push(other_id.to_string());
        }
    }

    // 根 + 分卷关联人物
    let mut root_char_ids: Vec<String> = crate::tree_links::root_character_ids(tree);
    for cid in &root_char_ids {
        if !char_ids.iter().any(|id| id == cid) {
            char_ids.push(cid.clone());
        }
    }
    if let Some(vid) = crate::tree_links::chapter_parent_volume_id(tree, node_id) {
        for cid in crate::tree_links::volume_character_ids(tree, &vid) {
            if !char_ids.iter().any(|id| id == &cid) {
                char_ids.push(cid);
            }
        }
    }
    if let Some(root) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
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
                let cap = if slim {
                    1200
                } else {
                    crate::character_fmt::CHARACTER_INJECT_CAP
                };
                let body = crate::kb_context::truncate_chars(
                    &crate::character_fmt::format_character_full(&n.label, c),
                    cap,
                );
                characters.push_str(&format!(
                    "- {}：{}{}\n{}\n",
                    labels.name, n.label, bookwide, body
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
            } else if inherited_plots.iter().any(|r| r == id) {
                if loc.is_zh() {
                    "｜分卷剧情·进行中"
                } else {
                    "｜volume · active"
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
            if slim {
                if loc.is_zh() {
                    plots.push_str(&format!(
                        "- 「{}」{bookwide}\n  剧情要点：{outline_text}\n  关联人物：{cast_text}\n",
                        n.label
                    ));
                } else {
                    plots.push_str(&format!(
                        "- “{}”{bookwide}\n  Beats: {outline_text}\n  Cast: {cast_text}\n",
                        n.label
                    ));
                }
            } else {
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
    }
    if plots.is_empty() {
        plots.push_str(labels.no_plots);
    }

    let mut kn_rows: Vec<(u8, String, String, usize)> = Vec::new();
    let mut inject_bumps = 0usize;
    for id in &knowledge_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let body = crate::core_laws_fmt::knowledge_card_inject_body(tree, n);
            let body = if body.trim().is_empty() {
                let k = n.knowledge.as_ref();
                let prompt = k.map(|x| x.extract_prompt.as_str()).unwrap_or("").trim();
                let outline = n.outline.trim();
                if !prompt.is_empty() {
                    format!("{}：{prompt}", labels.empty_knowledge)
                } else if !outline.is_empty() {
                    outline.to_string()
                } else {
                    labels.empty_knowledge.to_string()
                }
            } else {
                body
            };
            // 细纲 slim 只砍人物/剧情正文，勿把世界观/故事规则压回 500
            let per = crate::core_laws_fmt::knowledge_card_inject_cap(n);
            if per > crate::kb_context::KNOWLEDGE_CARD_EXTRACT_CAP {
                inject_bumps += 1;
            }
            let pri = crate::core_laws_fmt::knowledge_card_inject_priority(n);
            kn_rows.push((pri, n.label.clone(), body, per));
        }
    }
    kn_rows.sort_by_key(|r| r.0);
    let kn_cards: Vec<(String, String, usize)> = kn_rows
        .into_iter()
        .map(|(_, l, b, p)| (l, b, p))
        .collect();
    let mut total = crate::kb_context::CHAPTER_KNOWLEDGE_TOTAL_CAP;
    for _ in 0..inject_bumps {
        total = crate::core_laws_fmt::knowledge_inject_total(total);
    }
    let knowledge =
        crate::kb_context::format_knowledge_cards_capped_var(&kn_cards, total, loc.is_zh());
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
    let detailed_text = if detailed.iter().any(|s| !s.trim().is_empty()) {
        detailed
            .iter()
            .filter(|s| !s.trim().is_empty())
            .enumerate()
            .map(|(i, s)| format!("{}. {}", i + 1, s.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        labels.empty_detailed_outline.to_string()
    };

    let chapter_info = format!(
        "{}：{label}\n{}：\n{outline_text}\n{}：\n{detailed_text}",
        labels.chapter_title, labels.chapter_outline, labels.chapter_detailed_outline
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
                let body = crate::kb_context::truncate_chars(
                    &crate::character_fmt::format_character_full(&n.label, c),
                    crate::character_fmt::CHARACTER_INJECT_CAP,
                );
                characters.push_str(&format!(
                    "- {}：{}\n{}\n",
                    labels.name, n.label, body
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

/// 细纲：简介 + 功能选项 + 总纲；人物在 chapter_context。
fn root_brief_for_outline(tree: &NovelTree, novel: &NovelProject, loc: PromptLocale) -> String {
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel));
    let root_outline = root.map(|n| n.outline.as_str()).unwrap_or("");
    let synopsis = novel.synopsis.as_str();
    let syn = if synopsis.trim().is_empty() {
        if loc.is_zh() {
            "（暂无简介）"
        } else {
            "(no synopsis)"
        }
    } else {
        synopsis
    };
    let feat = crate::novel_features::format_novel_features_block(&novel.features);
    let extra = if root_outline.trim().is_empty() || root_outline.trim() == synopsis.trim()
    {
        String::new()
    } else if loc.is_zh() {
        format!("根节点补充说明：\n{root_outline}\n")
    } else {
        format!("Root notes:\n{root_outline}\n")
    };
    if loc.is_zh() {
        format!("【全书故事简介】\n{syn}\n{feat}\n{extra}")
    } else {
        format!("[Novel synopsis]\n{syn}\n{feat}\n{extra}")
    }
}

/// Extract core chapter facts into this novel’s isolated memory store (SQLite + Lance).
async fn extract_and_store_chapter_memory(
    app: Option<&tauri::AppHandle>,
    db: &Db,
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
    let model = llm_model(&settings);
    let system = prompts::extract_chapter_memory_system(loc);
    let user = prompts::extract_chapter_memory_user(
        loc,
        label,
        outline,
        body,
        user_notes,
        other_memory,
    );
    let extracted = if settings.resolve_compat(&model).is_some_and(|ep| ep.is_ready()) {
        crate::llm_extract::extract::<crate::llm_extract::ChapterMemoryExtract>(
            settings, &model, &system, &user,
        )
        .await
        .ok()
        .map(|e| e.into_chunks())
        .filter(|c| !c.is_empty())
    } else {
        None
    };
    let (chunks, used_mock) = if let Some(chunks) = extracted {
        (chunks, false)
    } else {
        let (raw, used_mock) = llm_complete_ex(
            app,
            db,
            settings,
            &system,
            &user,
            Some(&model),
            Some(novel_id),
            cancel,
            false,
        )
        .await?;
        (chapter_memory::split_memory_text(&raw), used_mock)
    };
    if used_mock {
        return Err(if loc.is_zh() {
            "当前为模拟模式，未写入记忆".into()
        } else {
            "Mock mode: memory was not written".into()
        });
    }
    if chunks.is_empty() {
        return Err(if loc.is_zh() {
            "未能从正文中抽取出记忆".into()
        } else {
            "No memory chunks extracted from body".into()
        });
    }
    db.replace_chapter_memory(novel_id, node_id, &chunks)
        .map_err(|e| e.to_string())?;
    chapter_memory::upsert_lance(novel_id, node_id, &chunks)
        .await
        .map_err(|e| e.to_string())?;
    Ok(chunks)
}

/// 组装勾选章节的已有记忆（供抽取对照）。
fn build_extract_other_memory(
    db: &Db,
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
    if let Ok(rows) = db.list_chapter_memory_for_nodes(novel_id, &ids) {
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
pub async fn regenerate_chapter_memory_inner(
    db: &Db,
    app: Option<&tauri::AppHandle>,
    novel_id: String,
    node_id: String,
    user_notes: String,
    memory_node_ids: Option<Vec<String>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<String>, String> {
    const TOTAL: u32 = 2;
    if let Some(app) = app {
        emit_chapter_progress(app, &novel_id, "memory", 1, TOTAL);
    }

    let tree = get_tree(novel_id.clone())?;
    let novel = db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = db.get_settings().map_err(|e| e.to_string())?;
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
    let other_memory = build_extract_other_memory(db, &novel_id, &tree, &ids, &node_id, loc);
    let chunks = extract_and_store_chapter_memory(
        app,
        db,
        &settings,
        &novel_id,
        &node_id,
        &node.label,
        &node.outline,
        &body,
        &user_notes,
        &other_memory,
        loc,
        cancel,
    )
    .await?;
    if let Some(app) = app {
        emit_chapter_progress(app, &novel_id, "saving", 2, TOTAL);
    }
    Ok(chunks)
}

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
    regenerate_chapter_memory_inner(
        &state.db,
        Some(&app),
        novel_id,
        node_id,
        user_notes,
        memory_node_ids,
        Some(cancel),
    )
    .await
}

pub(crate) fn shot_character_looks(tree: &NovelTree, node_id: &str) -> String {
    let Some(node) = tree.nodes.iter().find(|n| n.id == node_id) else {
        return String::new();
    };
    let mut ids = node.linked_character_ids.clone();
    for e in &tree.edges {
        if e.source != node_id && e.target != node_id {
            continue;
        }
        let other = if e.source == node_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        if tree
            .nodes
            .iter()
            .any(|n| n.id == other && matches!(n.kind, NodeKind::Character))
            && !ids.iter().any(|id| id == other)
        {
            ids.push(other.to_string());
        }
    }
    let mut out = String::new();
    for id in ids {
        let Some(n) = tree.nodes.iter().find(|x| x.id == id) else {
            continue;
        };
        let Some(c) = &n.character else { continue };
        let body = crate::kb_context::truncate_chars(
            &crate::character_fmt::format_character_full(&n.label, c),
            600,
        );
        out.push_str(&format!("- {}\n{}\n", n.label, body));
    }
    out
}

#[tauri::command]
pub fn get_chapter_shots(novel_id: String, node_id: String) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    crate::chapter_shots::load_shots(&novel_id, &node_id)
}

#[tauri::command]
pub fn set_chapter_shots(
    novel_id: String,
    node_id: String,
    shots: Vec<crate::chapter_shots::ChapterShot>,
) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    crate::chapter_shots::save_shots(&novel_id, &node_id, shots)
}

pub async fn split_chapter_shots_inner(
    db: &Db,
    app: Option<&tauri::AppHandle>,
    novel_id: String,
    node_id: String,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节可拆分镜头".into());
    }
    let body = get_chapter(novel_id.clone(), node_id.clone())?;
    if body.trim().is_empty() {
        return Err("请先写好本章正文".into());
    }
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &[node.label.as_str(), body.as_str()]);
    let chars = shot_character_looks(&tree, &node_id);
    let (reply, _) = llm_complete_ex(
        app,
        db,
        &settings,
        &prompts::split_chapter_shots_system(loc),
        &prompts::split_chapter_shots_user(loc, &node.label, &body, &chars),
        None,
        Some(&novel_id),
        cancel,
        true,
    )
    .await?;
    let shots = crate::chapter_shots::parse_shots_reply(&reply)?;
    crate::chapter_shots::save_shots(&novel_id, &node_id, shots)
}

#[tauri::command]
pub async fn split_chapter_shots(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    split_chapter_shots_inner(&state.db, Some(&app), novel_id, node_id, Some(cancel)).await
}

pub async fn generate_shot_comfy_prompts_inner(
    db: &Db,
    app: Option<&tauri::AppHandle>,
    novel_id: String,
    node_id: String,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    let mut shots = crate::chapter_shots::load_shots(&novel_id, &node_id)?;
    if shots.is_empty() {
        return Err("请先拆分镜头".into());
    }
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &[node.label.as_str()]);
    let chars = shot_character_looks(&tree, &node_id);
    let shots_json = serde_json::to_string_pretty(&shots).unwrap_or_default();
    let (reply, _) = llm_complete_ex(
        app,
        db,
        &settings,
        &prompts::shot_comfy_prompts_system(loc),
        &prompts::shot_comfy_prompts_user(loc, &node.label, &chars, &shots_json),
        None,
        Some(&novel_id),
        cancel,
        true,
    )
    .await?;
    shots = crate::chapter_shots::apply_prompt_reply(&shots, &reply)?;
    crate::chapter_shots::save_shots(&novel_id, &node_id, shots)
}

#[tauri::command]
pub async fn generate_shot_comfy_prompts(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<Vec<crate::chapter_shots::ChapterShot>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    generate_shot_comfy_prompts_inner(&state.db, Some(&app), novel_id, node_id, Some(cancel)).await
}

pub async fn submit_chapter_shots_comfyui_inner(
    db: &Db,
    novel_id: &str,
    node_id: &str,
) -> Result<crate::chapter_shots::ComfySubmitResult, String> {
    let shots = crate::chapter_shots::load_shots(novel_id, node_id)?;
    let settings = db.get_settings().map_err(|e| e.to_string())?;
    let wf: serde_json::Value = serde_json::from_str(settings.comfyui_workflow.trim())
        .map_err(|_| "请先在设置粘贴 ComfyUI「导出（API）」工作流 JSON".to_string())?;
    crate::chapter_shots::submit_shots(
        &settings.comfyui_url,
        &wf,
        &settings.comfyui_prompt_node,
        &shots,
    )
    .await
}

#[tauri::command]
pub async fn submit_chapter_shots_comfyui(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<crate::chapter_shots::ComfySubmitResult, String> {
    submit_chapter_shots_comfyui_inner(&state.db, &novel_id, &node_id).await
}

/// 预生成硬验收：大纲节拍 + 本章/根/分卷剧情卡 + 本章焦点人物（不含仅挂在根上的路人）。
fn collect_land_beats(tree: &NovelTree, novel_id: &str, node_id: &str) -> Vec<LandBeat> {
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let mut beats = Vec::new();
    let detailed = node
        .map(|n| n.detailed_outline.clone())
        .unwrap_or_default();
    let detailed_items: Vec<String> = detailed
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= 4)
        .take(12)
        .collect();
    if !detailed_items.is_empty() {
        for (i, text) in detailed_items.into_iter().enumerate() {
            beats.push(LandBeat {
                kind: "outline",
                title: format!("细纲§{}", i + 1),
                text,
            });
        }
    } else {
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
    }

    // 本机 + 根∪分卷继承（与 chapter_context / 左栏一致；继承项不参与本章排序）
    let mut plot_ids = crate::tree_links::chapter_local_plot_ids(tree, node_id);
    for pid in crate::tree_links::chapter_inherited_plot_ids(tree, node_id) {
        if !plot_ids.iter().any(|id| id == &pid) {
            plot_ids.push(pid);
        }
    }
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
        if matches!(other.kind, NodeKind::Character) && !focus_chars.iter().any(|id| id == other_id)
        {
            focus_chars.push(other_id.to_string());
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
    let mut facts: Vec<(String, String)> = Vec::new();
    if let Ok(rows) = state.db.list_chapter_memory_for_nodes(novel_id, &node_ids) {
        facts.extend(rows);
    }
    let ranked = chapter_memory::retrieve_memory(
        novel_id,
        &facts,
        query,
        crate::kb_context::MEMORY_RETRIEVE_K,
        Some(&node_ids),
    );
    let mut fact_lines = Vec::new();
    let mut used = 0usize;
    for (nid, content) in ranked {
        let sid = format!("memory:{nid}");
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
pub async fn generate_detailed_outline(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    user_notes: String,
    model: Option<String>,
) -> Result<Vec<String>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    generate_detailed_outline_inner(
        Some(&app),
        &state.db,
        &novel_id,
        &node_id,
        &user_notes,
        model.as_deref(),
        Some(cancel),
        true,
    )
    .await
}

/// 从简纲进化细纲并写回树。`force` 为 false 且已有细纲时直接返回现有。
pub(crate) async fn generate_detailed_outline_inner(
    app: Option<&tauri::AppHandle>,
    db: &Db,
    novel_id: &str,
    node_id: &str,
    user_notes: &str,
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
    force: bool,
) -> Result<Vec<String>, String> {
    let tree = get_tree(novel_id.to_string())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节卡可生成细纲".into());
    }
    if !force {
        let existing: Vec<String> = node
            .detailed_outline
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    if node.outline.trim().is_empty() {
        return Err("请先填写本章简纲".into());
    }
    let novel = db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node.outline.as_str(),
            user_notes,
        ],
    );
    let root_ref = root_brief_for_outline(&tree, &novel, loc);
    let (chapter_info, cards) = chapter_context(&tree, novel_id, node_id, loc, true);
    let (wmin, wmax) = root_chapter_word_target(&tree, &novel);
    let (beats_lo, beats_hi, words_per) = detailed_outline_pace(wmin, wmax);
    let system = prompts::generate_detailed_outline_system(
        loc, wmin, wmax, beats_lo, beats_hi, words_per,
    );
    let user = prompts::generate_detailed_outline_user(
        loc,
        &root_ref,
        &chapter_info,
        &cards,
        user_notes,
        wmin,
        wmax,
        beats_lo,
        beats_hi,
        words_per,
    );
    let model_name = llm_model(&settings);
    let (reply, _mock) = llm_complete_ex(
        app,
        db,
        &settings,
        &system,
        &user,
        Some(&model_name),
        Some(novel_id),
        cancel.clone(),
        true,
    )
    .await?;
    let items = parse_detailed_outline_list_or_repair(
        app,
        db,
        &settings,
        &model_name,
        novel_id,
        loc,
        cancel,
        &reply,
    )
    .await?;
    if items.is_empty() {
        return Err(if loc.is_zh() {
            "细纲为空".into()
        } else {
            "Detailed outline empty".into()
        });
    }
    let mut tree = get_tree(novel_id.to_string())?;
    if let Some(n) = tree.nodes.iter_mut().find(|n| n.id == node_id) {
        n.detailed_outline = items.clone();
    }
    save_tree(tree)?;
    Ok(items)
}

/// 重写细纲单条并写回树。`index` 为 0-based。
#[tauri::command]
pub async fn regenerate_detailed_outline_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    index: u32,
    user_notes: String,
    model: Option<String>,
) -> Result<String, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    regenerate_detailed_outline_item_inner(
        Some(&app),
        &state.db,
        &novel_id,
        &node_id,
        index as usize,
        &user_notes,
        model.as_deref(),
        Some(cancel),
    )
    .await
}

pub(crate) async fn regenerate_detailed_outline_item_inner(
    app: Option<&tauri::AppHandle>,
    db: &Db,
    novel_id: &str,
    node_id: &str,
    index: usize,
    user_notes: &str,
    model: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let tree = get_tree(novel_id.to_string())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节卡可重写细纲".into());
    }
    if node.outline.trim().is_empty() {
        return Err("请先填写本章简纲".into());
    }
    let items = node.detailed_outline.clone();
    if index >= items.len() {
        return Err(format!("细纲下标越界：{index}（共 {} 条）", items.len()));
    }
    let novel = db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node.outline.as_str(),
            user_notes,
            items.get(index).map(|s| s.as_str()).unwrap_or(""),
        ],
    );
    let root_ref = root_brief_for_outline(&tree, &novel, loc);
    let (chapter_info, cards) = chapter_context(&tree, novel_id, node_id, loc, true);
    let neighbors = items
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mark = if i == index {
                if loc.is_zh() {
                    " ←重写"
                } else {
                    " ←rewrite"
                }
            } else {
                ""
            };
            format!("{}. {}{mark}", i + 1, s.trim())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let current = items[index].trim().to_string();
    let system = prompts::regenerate_detailed_outline_item_system(loc);
    let user = prompts::regenerate_detailed_outline_item_user(
        loc,
        &root_ref,
        &chapter_info,
        &cards,
        index,
        &current,
        &neighbors,
        user_notes,
    );
    let model_name = llm_model(&settings);
    let (reply, _mock) = llm_complete_ex(
        app,
        db,
        &settings,
        &system,
        &user,
        Some(&model_name),
        Some(novel_id),
        cancel,
        true,
    )
    .await?;
    let item = parse_detailed_outline_item(&reply).ok_or_else(|| {
        if loc.is_zh() {
            "细纲单条解析失败，请重试".to_string()
        } else {
            "Failed to parse detailed outline item".to_string()
        }
    })?;
    let mut tree = get_tree(novel_id.to_string())?;
    if let Some(n) = tree.nodes.iter_mut().find(|n| n.id == node_id) {
        if index >= n.detailed_outline.len() {
            return Err(format!(
                "细纲下标越界：{index}（共 {} 条）",
                n.detailed_outline.len()
            ));
        }
        n.detailed_outline[index] = item.clone();
    }
    save_tree(tree)?;
    Ok(item)
}

fn parse_detailed_outline_item(reply: &str) -> Option<String> {
    if let Some(list) = parse_detailed_outline_list(reply) {
        if let Some(s) = list.into_iter().next() {
            return Some(s);
        }
    }
    for obj in extract_json_objects(reply) {
        for key in ["item", "beat", "text", "细纲", "内容"] {
            if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    reply.lines().find_map(outline_beat_from_line)
}

fn parse_detailed_outline_list(reply: &str) -> Option<Vec<String>> {
    let keys = ["detailed_outline", "细纲", "items", "beats", "scenes"];
    for obj in extract_json_objects(reply) {
        let Some(field) = keys.iter().find_map(|k| obj.get(*k)) else {
            continue;
        };
        if let Some(list) = field.as_array() {
            let items: Vec<String> = list.iter().filter_map(json_value_to_beat).collect();
            if !items.is_empty() {
                return Some(items);
            }
        }
        if let Some(s) = field.as_str() {
            if let Some(items) = parse_json_string_array(s) {
                return Some(items);
            }
        }
        if field.is_object() {
            if let Some(items) = beats_from_json_map(&field.to_string()) {
                return Some(items);
            }
        }
    }
    if let Some(items) = extract_named_string_array(reply, &keys) {
        return Some(items);
    }
    let n = normalize_llm_json_text(reply);
    let wrapped = format!("{{{n}}}");
    if let Some(items) = extract_named_string_array(&wrapped, &keys) {
        return Some(items);
    }
    if let Some(items) = parse_json_string_array(&n) {
        return Some(items);
    }
    let lines: Vec<String> = reply.lines().filter_map(outline_beat_from_line).take(16).collect();
    if lines.len() >= 2 {
        Some(lines)
    } else {
        None
    }
}

async fn parse_detailed_outline_list_or_repair(
    app: Option<&tauri::AppHandle>,
    db: &Db,
    settings: &AppSettings,
    model: &str,
    novel_id: &str,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
    reply: &str,
) -> Result<Vec<String>, String> {
    if reply.trim().is_empty() {
        return Err(if loc.is_zh() {
            "模型未返回细纲".into()
        } else {
            "Model returned an empty detailed outline".into()
        });
    }
    if let Some(items) = parse_detailed_outline_list(reply) {
        if !items.is_empty() {
            return Ok(items);
        }
    }
    if settings.resolve_compat(model).is_some_and(|ep| ep.is_ready()) {
        if let Ok(ex) = crate::llm_extract::extract::<crate::llm_extract::DetailedOutlineExtract>(
            settings,
            model,
            &prompts::repair_detailed_outline_json_system(loc),
            reply,
        )
        .await
        {
            let items: Vec<String> = ex
                .detailed_outline
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !items.is_empty() {
                return Ok(items);
            }
        }
    }
    let (fixed, _) = llm_complete_ex(
        app,
        db,
        settings,
        &prompts::repair_detailed_outline_json_system(loc),
        &prompts::repair_detailed_outline_json_user(loc, reply),
        Some(model),
        Some(novel_id),
        cancel,
        true,
    )
    .await?;
    if let Some(items) = parse_detailed_outline_list(&fixed) {
        if !items.is_empty() {
            return Ok(items);
        }
    }
    let snip: String = reply.chars().take(280).collect();
    Err(if loc.is_zh() {
        format!("细纲解析失败，结构树未改动。请重试。\n\n模型原文摘录：\n{snip}")
    } else {
        format!(
            "Failed to parse detailed outline; tree unchanged. Please retry.\n\nModel snippet:\n{snip}"
        )
    })
}

#[cfg(test)]
mod detailed_outline_parse_tests {
    use super::{parse_detailed_outline_item, parse_detailed_outline_list};

    #[test]
    fn parses_json_array() {
        let r = parse_detailed_outline_list(r#"{"detailed_outline":["开场上船","旧识现身"]}"#).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], "开场上船");
    }

    #[test]
    fn parses_pretty_json_without_wrapping_object() {
        let raw = r#"
"detailed_outline": [
  "开场上船，夜雾压港",
  "旧识现身拦路",
  "被迫改道内河"
]
"#;
        let r = parse_detailed_outline_list(raw).unwrap();
        assert_eq!(r, vec!["开场上船，夜雾压港", "旧识现身拦路", "被迫改道内河"]);
    }

    #[test]
    fn line_fallback_strips_json_quotes_and_commas() {
        let raw = r#"
"detailed_outline": [
"码头遇雨，旧识拦路",
"被迫改道内河",
]
"#;
        let r = parse_detailed_outline_list(raw).unwrap();
        assert!(!r.iter().any(|s| s.contains("detailed_outline") || s.contains('"')));
        assert_eq!(r[0], "码头遇雨，旧识拦路");
        assert_eq!(r[1], "被迫改道内河");
    }

    #[test]
    fn parses_json_item() {
        let r = parse_detailed_outline_item(r#"{"item":"码头遇雨，旧识拦路"}"#).unwrap();
        assert_eq!(r, "码头遇雨，旧识拦路");
    }

    #[test]
    fn parses_array_of_objects() {
        let raw = r#"{"detailed_outline":[{"scene":"开场上船，夜雾压港"},{"who":"旧识","what":"拦路","result":"改道"}]}"#;
        let r = parse_detailed_outline_list(raw).unwrap();
        assert_eq!(r[0], "开场上船，夜雾压港");
        assert!(r[1].contains("旧识") && r[1].contains("拦路"));
    }

    #[test]
    fn parses_truncated_string_array() {
        let raw = r#"{"detailed_outline":["开场上船，夜雾压港","旧识现身拦路","被迫改道内"#;
        let r = parse_detailed_outline_list(raw).unwrap();
        assert_eq!(r, vec!["开场上船，夜雾压港", "旧识现身拦路"]);
    }

    #[test]
    fn parses_numbered_object_map() {
        let raw = r#"{"detailed_outline":{"1":"开场上船，夜雾压港","2":"旧识现身拦路"}}"#;
        let r = parse_detailed_outline_list(raw).unwrap();
        assert_eq!(r, vec!["开场上船，夜雾压港", "旧识现身拦路"]);
    }
}

#[cfg(test)]
mod detailed_outline_materials_tests {
    use super::chapter_context;
    use crate::models::{
        CoreLawsPayload, KnowledgeCardPayload, NodeKind, NodePosition, NovelTree, TreeNode,
    };
    use crate::prompts::PromptLocale;

    fn node(id: &str, kind: NodeKind) -> TreeNode {
        TreeNode {
            id: id.into(),
            kind,
            label: id.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition { x: 0.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        }
    }

    #[test]
    fn slim_outline_keeps_worldview_tail_past_500() {
        const MARKER: &str = "SLIM_MUST_KEEP_NO_PRICELESS_GOD";
        let mut root = node("root", NodeKind::Novel);
        root.linked_knowledge_ids = vec!["core".into()];
        let ch = node("ch1", NodeKind::Chapter);
        let mut core = node("core", NodeKind::Knowledge);
        core.knowledge = Some(KnowledgeCardPayload {
            slot: "wv_core_laws".into(),
            core_laws: Some(CoreLawsPayload {
                premise: format!("{}{MARKER}", "甲".repeat(600)),
                ..Default::default()
            }),
            ..Default::default()
        });
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![root, ch, core],
            edges: vec![],
        };
        let (_info, cards) = chapter_context(&tree, "n", "ch1", PromptLocale::ZhCn, true);
        assert!(
            cards.contains(MARKER),
            "slim outline context must keep worldview past the 500-char ordinary cap: {cards}"
        );
    }
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
    const TOTAL: u32 = 8;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    // Chat 面板所选模型；非空时覆盖本轮 chat_model。
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let model_name = llm_model(&settings);
    emit_chapter_progress_detail(
        &app,
        &novel_id,
        "confirm_model",
        1,
        TOTAL,
        Some(&model_name),
    );

    emit_chapter_progress(&app, &novel_id, "context", 2, TOTAL);

    // 正文前先确保细纲存在（空则从简纲进化；已有则保留手改）
    emit_chapter_progress(&app, &novel_id, "detailed_outline", 3, TOTAL);
    let _ = generate_detailed_outline_inner(
        Some(&app),
        &state.db,
        &novel_id,
        &node_id,
        &user_brief,
        model.as_deref(),
        Some(cancel.clone()),
        false,
    )
    .await?;

    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
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
    let write_brief = crate::write_prompts::merge_user_brief(
        &user_brief,
        &crate::write_prompts::write_prompt_append(
            &tree,
            crate::write_prompts::WritePromptKind::Generate,
        ),
    );
    let root_ref = root_generate_reference(&tree, &novel.synopsis, loc);
    let (chapter_info, cards) = chapter_context(&tree, &novel_id, &node_id, loc, false);
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
    let system = prompts::generate_chapter_system(
        loc,
        &novel.title,
        &novel.synopsis,
        wmin,
        wmax,
        novel.chapter_count,
    );
    let user = prompts::generate_chapter_user(
        loc,
        &root_ref,
        &prior_hist,
        &chapter_info,
        &cards,
        &contract,
        &write_brief,
        wmin,
        wmax,
    );
    let model = model_name;
    emit_chapter_progress(&app, &novel_id, "writing", 4, TOTAL);
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
    // 字数只靠首轮 Prompt 交给 AI；落地轮不做字数验证。
    let mut repaired_n = 0usize;
    emit_chapter_progress(&app, &novel_id, "check_beats", 5, TOTAL);
    if !used_mock && !land_beats.is_empty() {
        let missing = chapter_constraints::missing_beats(&content, &land_beats);
        if !missing.is_empty() {
            repaired_n = missing.len();
            emit_chapter_progress(&app, &novel_id, "repair_land", 6, TOTAL);
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

    emit_chapter_progress(&app, &novel_id, "check_lore", 7, TOTAL);
    let lore = lore_canon_text(&tree);
    let lore_fixed = verify_lore_pass(
        &app,
        &state,
        &settings,
        &novel_id,
        &model,
        loc,
        &lore,
        &mut content,
        used_mock,
        cancel.clone(),
    )
    .await?;

    emit_chapter_progress(&app, &novel_id, "saving", 8, TOTAL);
    let words = count_words(&content);
    content.push_str(&prompts::generate_footer(loc, &node_id, &model));
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    // 预生成不抽取章节记忆；记忆仅手动抽取。
    let mut message = prompts::generate_done_msg(loc, &model, words, used_mock, repaired_n);
    if lore_fixed {
        message.push_str(&prompts::lore_fixed_note(loc));
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
    const TOTAL: u32 = 5;
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    // Chat 面板所选模型；非空时覆盖本轮 chat_model。
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let refine_model = llm_model(&settings);
    emit_chapter_progress_detail(
        &app,
        &novel_id,
        "confirm_model",
        1,
        TOTAL,
        Some(&refine_model),
    );
    emit_chapter_progress(&app, &novel_id, "context", 2, TOTAL);

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
    let (chapter_info, cards) = chapter_context(&tree, &novel_id, &node_id, loc, false);

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
    let system = prompts::refine_chapter_system(
        loc,
        &novel.title,
        linked_n,
        wmin,
        wmax,
        full_body,
    );
    let brief = crate::write_prompts::merge_user_brief(
        &user_brief.unwrap_or_default(),
        &crate::write_prompts::write_prompt_append(
            &tree,
            crate::write_prompts::WritePromptKind::Refine,
        ),
    );
    let user = prompts::refine_chapter_user(
        loc,
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
    emit_chapter_progress(&app, &novel_id, "refining", 3, TOTAL);
    // 字数只靠提示词约束，不再额外跑篇幅校准。
    let (mut content, used_mock) = llm_complete(
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
    emit_chapter_progress(&app, &novel_id, "check_lore", 4, TOTAL);
    let lore = lore_canon_text(&tree);
    let lore_fixed = verify_lore_pass(
        &app,
        &state,
        &settings,
        &novel_id,
        &refine_model,
        loc,
        &lore,
        &mut content,
        used_mock,
        cancel.clone(),
    )
    .await?;
    emit_chapter_progress(&app, &novel_id, "saving", 5, TOTAL);
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let words = count_words(&content);
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    // 精修不再自动抽取记忆；请在记忆面板手动提取。
    let mut message =
        prompts::refine_done_msg(loc, &refine_model, linked_n, words, used_mock, full_body);
    if lore_fixed {
        message.push_str(&prompts::lore_fixed_note(loc));
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

fn strip_outer_md_fence(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let mut lines: Vec<&str> = t.lines().collect();
    if lines.first().is_some_and(|l| l.starts_with("```")) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

/// ponytail: 设定校对一轮 LLM（判断+就地修订）；总长封顶以免章+设定撑爆上下文。升级：分卡评审若误漏增多。
const LORE_VERIFY_TOTAL_CAP: usize = 12_000;

fn lore_canon_text(tree: &NovelTree) -> String {
    let mut parts: Vec<String> = Vec::new();
    for slot in WV_MAIN_SLOTS {
        let Some(n) = knowledge_by_slot(tree, slot) else {
            continue;
        };
        let body = crate::core_laws_fmt::knowledge_card_inject_body(tree, n);
        let t = crate::kb_context::truncate_chars(
            &body,
            crate::core_laws_fmt::knowledge_card_inject_cap(n),
        );
        if t.trim().chars().count() < 12 {
            continue;
        }
        let title = n.label.trim();
        parts.push(if title.is_empty() {
            t
        } else {
            format!("【{title}】\n{t}")
        });
    }
    crate::kb_context::truncate_chars(&parts.join("\n\n"), LORE_VERIFY_TOTAL_CAP)
}

fn lore_verify_accepts_patch(original: &str, out: &str) -> Option<String> {
    let t = strip_outer_md_fence(out);
    let head = t.lines().next().unwrap_or("").trim();
    let head_up = head.to_ascii_uppercase();
    if head_up == "LORE_OK" || head_up.starts_with("LORE_OK") {
        return None;
    }
    if t.trim().chars().count() > original.trim().chars().count() / 2 {
        Some(t)
    } else {
        None
    }
}

async fn verify_lore_pass(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel_id: &str,
    model: &str,
    loc: PromptLocale,
    lore: &str,
    content: &mut String,
    used_mock: bool,
    cancel: Arc<AtomicBool>,
) -> Result<bool, String> {
    if used_mock || lore.trim().chars().count() < 20 {
        return Ok(false);
    }
    let sys = prompts::verify_lore_chapter_system(loc);
    let user = prompts::verify_lore_chapter_user(loc, lore, content);
    match llm_complete(
        Some(app),
        state,
        settings,
        &sys,
        &user,
        Some(model),
        Some(novel_id),
        Some(cancel),
    )
    .await
    {
        Ok((out, mock2)) => {
            if mock2 {
                return Ok(false);
            }
            if let Some(fixed) = lore_verify_accepts_patch(content, &out) {
                *content = fixed;
                return Ok(true);
            }
            Ok(false)
        }
        Err(e) if e == "cancelled" => Err(e),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod lore_verify_tests {
    use super::{lore_canon_text, lore_verify_accepts_patch};
    use crate::models::NovelTree;

    #[test]
    fn lore_ok_keeps_original() {
        let orig = "甲".repeat(80);
        assert!(lore_verify_accepts_patch(&orig, "LORE_OK").is_none());
        assert!(lore_verify_accepts_patch(&orig, "lore_ok\nextra").is_none());
        assert!(lore_verify_accepts_patch(&orig, "```\nLORE_OK\n```").is_none());
    }

    #[test]
    fn short_output_rejected() {
        let orig = "甲".repeat(80);
        assert!(lore_verify_accepts_patch(&orig, "太短").is_none());
    }

    #[test]
    fn long_patch_accepted() {
        let orig = "甲".repeat(80);
        let patch = "乙".repeat(50);
        assert_eq!(
            lore_verify_accepts_patch(&orig, &patch).as_deref(),
            Some(patch.as_str())
        );
    }

    #[test]
    fn empty_tree_has_no_lore() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![],
            edges: vec![],
        };
        assert!(lore_canon_text(&tree).trim().is_empty());
    }
}

/// 正文编辑区：按用户意见改写单段；模型同全局 Chat（`chat_model`）。
#[tauri::command]
pub async fn rewrite_chapter_paragraph(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    paragraph: String,
    instruction: String,
    model: Option<String>,
) -> Result<String, String> {
    let para = paragraph.trim();
    let note = instruction.trim();
    if para.is_empty() {
        return Err("段落为空".into());
    }
    if note.is_empty() {
        return Err("请填写修改意见".into());
    }
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
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let chat_model = llm_model(&settings);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[novel.title.as_str(), novel.synopsis.as_str(), para, note],
    );
    let system = prompts::rewrite_paragraph_system(loc);
    let user = prompts::rewrite_paragraph_user(loc, para, note);
    let (content, _) = llm_complete_ex(
        Some(&app),
        &state.db,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id.as_str()),
        Some(cancel),
        false,
    )
    .await?;
    let out = strip_outer_md_fence(&content);
    if out.is_empty() {
        return Err(if loc.is_zh() {
            "模型未返回有效段落".into()
        } else {
            "Model returned an empty paragraph".into()
        });
    }
    Ok(out)
}

const BODY_SUGGEST_CTX_CAP: usize = 800;
const BODY_SUGGEST_OUTLINE_CAP: usize = 4000;
const BODY_SUGGEST_COUNT: usize = 6;

fn clip_suggest_ctx(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= BODY_SUGGEST_CTX_CAP {
        return t.to_string();
    }
    t.chars()
        .rev()
        .take(BODY_SUGGEST_CTX_CAP)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn take_six_suggestions(items: Vec<String>) -> Option<Vec<String>> {
    let out: Vec<String> = items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .take(BODY_SUGGEST_COUNT)
        .collect();
    (out.len() == BODY_SUGGEST_COUNT).then_some(out)
}

fn parse_body_suggestions(raw: &str) -> Option<Vec<String>> {
    let s = strip_outer_md_fence(raw);
    if let Some(items) = extract_named_string_array(&s, &["suggestions", "items"]) {
        if let Some(six) = take_six_suggestions(items) {
            return Some(six);
        }
    }
    if let Some(items) = parse_json_string_array(&s) {
        if let Some(six) = take_six_suggestions(items) {
            return Some(six);
        }
    }
    let lines: Vec<String> = s
        .lines()
        .filter_map(outline_beat_from_line)
        .collect();
    take_six_suggestions(lines)
}

fn format_body_suggest_outline(items: &[String]) -> String {
    let joined = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, s)| format!("{}. {s}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    if joined.chars().count() <= BODY_SUGGEST_OUTLINE_CAP {
        return joined;
    }
    joined.chars().take(BODY_SUGGEST_OUTLINE_CAP).collect()
}

/// 正文编辑区：本章细纲 + 上一段 + 当前段已写 → 6 条下一步怎么写。
#[tauri::command]
pub async fn suggest_body_next(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    current: String,
    prev_paragraph: String,
    model: Option<String>,
) -> Result<Vec<String>, String> {
    let current = clip_suggest_ctx(&current);
    let prev = clip_suggest_ctx(&prev_paragraph);
    let tree = get_tree(novel_id.clone())?;
    let node = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| "章节不存在".to_string())?;
    if !matches!(node.kind, NodeKind::Chapter) {
        return Err("仅章节可续写建议".into());
    }
    let outline = format_body_suggest_outline(&node.detailed_outline);
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
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let chat_model = llm_model(&settings);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            outline.as_str(),
            current.as_str(),
            prev.as_str(),
        ],
    );
    let system = prompts::suggest_body_next_system(loc);
    let user = prompts::suggest_body_next_user(loc, &outline, &current, &prev);
    let (content, _) = llm_complete_ex(
        Some(&app),
        &state.db,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id.as_str()),
        Some(cancel),
        false,
    )
    .await?;
    parse_body_suggestions(&content).ok_or_else(|| {
        if loc.is_zh() {
            "模型未返回 6 条有效建议".into()
        } else {
            "Model did not return 6 suggestions".into()
        }
    })
}

/// 人物整卡 AI 改写：参考书（世界观/其他卡）总上限
const CHARACTER_REWRITE_REF_CAP: usize = 5000;

const WV_MAIN_SLOTS: [&str; 7] = [
    "wv_core_laws",
    "wv_spatiotemporal",
    "wv_social_power",
    "wv_existence",
    "wv_info_flow",
    "wv_history_culture",
    "story_rules",
];

fn is_reserved_knowledge_inject_slot(slot: &str) -> bool {
    slot.starts_with("wv_")
        || slot == "story_rules"
        || slot == crate::write_prompts::WRITE_PROMPTS_SLOT
        || crate::story_rules_fmt::is_story_rules_fan_slot(slot)
}

fn is_story_rules_knowledge_node(n: &TreeNode) -> bool {
    if !matches!(n.kind, NodeKind::Knowledge) {
        return false;
    }
    let slot = n
        .knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim();
    slot == "story_rules" || crate::story_rules_fmt::is_story_rules_fan_slot(slot)
}

/// 写作/Chat/AI 改写参考书：世界观 + 人物 + 剧情 + 其余知识卡 + 章节大纲。
fn assemble_novel_rewrite_reference(
    novel: &NovelProject,
    tree: &NovelTree,
    exclude_character_id: Option<&str>,
    exclude_knowledge_id: Option<&str>,
    loc_is_zh: bool,
) -> String {
    let cap = |s: &str, n: usize| crate::kb_context::truncate_chars(s, n);
    let mut sections: Vec<String> = Vec::new();

    let head = if loc_is_zh {
        format!(
            "【书名】{}\n【简介】{}",
            novel.title.trim(),
            novel.synopsis.trim()
        )
    } else {
        format!(
            "[Title] {}\n[Synopsis] {}",
            novel.title.trim(),
            novel.synopsis.trim()
        )
    };
    sections.push(head);
    let feat = crate::novel_features::format_novel_features_block(&novel.features);
    if !feat.trim().is_empty() {
        sections.push(feat);
    }

    // ponytail: skip all-chapter outlines (was duplicating write-chapter history). Cap: add chapter titles only if rewrite quality suffers.

    let mut wv = String::new();
    for slot in WV_MAIN_SLOTS {
        if let Some(n) = knowledge_by_slot(tree, slot) {
            let body = if slot == "story_rules" {
                crate::story_rules_fmt::format_story_rules_full(tree, n)
            } else {
                crate::core_laws_fmt::knowledge_card_inject_body(tree, n)
            };
            if body.trim().is_empty() {
                continue;
            }
            wv.push_str(&format!(
                "### {}\n{}\n\n",
                n.label.trim(),
                cap(&body, 800)
            ));
        }
    }
    if !wv.trim().is_empty() {
        sections.push(if loc_is_zh {
            format!("【世界观与故事规则】\n{}", wv.trim())
        } else {
            format!("[Worldview & story rules]\n{}", wv.trim())
        });
    }

    let mut chars = String::new();
    for n in tree.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Character)) {
        if exclude_character_id == Some(n.id.as_str()) {
            continue;
        }
        let Some(c) = &n.character else { continue };
        let body = cap(
            &crate::character_fmt::format_character_full(&n.label, c),
            400,
        );
        if body.trim().is_empty() {
            continue;
        }
        chars.push_str(&format!("### {}\n{}\n\n", n.label.trim(), body));
    }
    if !chars.trim().is_empty() {
        sections.push(if loc_is_zh {
            format!("【人物卡】\n{}", chars.trim())
        } else {
            format!("[Characters]\n{}", chars.trim())
        });
    }

    let mut plots = String::new();
    for n in tree.nodes.iter().filter(|n| matches!(n.kind, NodeKind::SidePlot)) {
        if !plot_is_injectable(n) {
            continue;
        }
        let outline = n.outline.trim();
        if outline.is_empty() {
            continue;
        }
        plots.push_str(&format!(
            "- 「{}」\n  {}\n",
            n.label.trim(),
            cap(outline, 240)
        ));
    }
    if !plots.trim().is_empty() {
        sections.push(if loc_is_zh {
            format!("【剧情卡】\n{}", plots.trim())
        } else {
            format!("[Plot cards]\n{}", plots.trim())
        });
    }

    let mut kn = String::new();
    for n in tree.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Knowledge)) {
        if exclude_knowledge_id == Some(n.id.as_str()) {
            continue;
        }
        let slot = n
            .knowledge
            .as_ref()
            .map(|k| k.slot.as_str())
            .unwrap_or("")
            .trim();
        if is_reserved_knowledge_inject_slot(slot)
            || crate::write_prompts::is_write_prompt_excluded_id(tree, &n.id)
        {
            continue;
        }
        let body = crate::core_laws_fmt::knowledge_card_inject_body(tree, n);
        if body.trim().is_empty() {
            continue;
        }
        kn.push_str(&format!(
            "### {}\n{}\n\n",
            n.label.trim(),
            cap(&body, 400)
        ));
    }
    if !kn.trim().is_empty() {
        sections.push(if loc_is_zh {
            format!("【其他知识卡】\n{}", kn.trim())
        } else {
            format!("[Other knowledge cards]\n{}", kn.trim())
        });
    }

    cap(&sections.join("\n\n"), CHARACTER_REWRITE_REF_CAP)
}

/// 世界观 / 设定字段按提示词改写或新写（允许当前内容为空）。
#[tauri::command]
pub async fn rewrite_text_field(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    field_label: String,
    current: String,
    instruction: String,
    context: Option<String>,
    model: Option<String>,
    node_id: Option<String>,
) -> Result<String, String> {
    let note = instruction.trim();
    if note.is_empty() {
        return Err("请填写提示词".into());
    }
    let label = field_label.trim();
    if label.is_empty() {
        return Err("字段名为空".into());
    }
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
    let mut settings = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model.as_deref());
    let chat_model = llm_model(&settings);
    let ctx = context.unwrap_or_default();
    let tree = if prompts::is_character_whole_rewrite_label(label)
        || prompts::is_story_rules_block_whole_label(label)
        || node_id
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    {
        Some(get_tree(novel_id.clone())?)
    } else {
        None
    };
    let story_rules_ai = prompts::is_story_rules_block_whole_label(label)
        || tree.as_ref().and_then(|t| {
            node_id
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .and_then(|id| t.nodes.iter().find(|n| n.id == id))
                .map(is_story_rules_knowledge_node)
        })
        .unwrap_or(false);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            label,
            current.as_str(),
            note,
            ctx.as_str(),
        ],
    );
    let system = if prompts::is_character_whole_rewrite_label(label) {
        prompts::rewrite_character_card_system(loc)
    } else if prompts::is_story_rules_block_whole_label(label) {
        prompts::rewrite_story_rules_block_system(loc)
    } else {
        prompts::rewrite_text_field_system(loc)
    };
    let user = if prompts::is_character_whole_rewrite_label(label) {
        let tree = tree.as_ref().expect("character whole rewrite tree");
        let exclude = node_id
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let reference = assemble_novel_rewrite_reference(&novel, tree, exclude, None, loc.is_zh());
        if loc.is_zh() {
            format!(
                "【参考书（世界观与其他卡面；须与下列设定一致）】\n{reference}\n\n【人物卡当前内容】\n{cur}\n\n【提示词与 JSON 要求】\n{note}\n\n请只输出填满全部字段的 JSON 对象：",
                cur = if current.trim().is_empty() {
                    "（空）"
                } else {
                    current.as_str()
                },
                note = note,
            )
        } else {
            format!(
                "[Reference — worldview and other cards; stay consistent]\n{reference}\n\n[Current character card]\n{cur}\n\n[Prompt and JSON schema]\n{note}\n\nOutput only the complete JSON object:",
                cur = if current.trim().is_empty() {
                    "(empty)"
                } else {
                    current.as_str()
                },
                note = note,
            )
        }
    } else if prompts::is_story_rules_block_whole_label(label) || story_rules_ai {
        let tree = tree.as_ref().expect("story rules rewrite tree");
        let exclude_kn = node_id
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let reference = assemble_novel_rewrite_reference(&novel, tree, None, exclude_kn, loc.is_zh());
        if prompts::is_story_rules_block_whole_label(label) {
            if loc.is_zh() {
                format!(
                    "【参考书（全书卡面；须与下列设定一致）】\n{reference}\n\n【当前卡片内容】\n{cur}\n\n【提示词与 JSON 要求】\n{note}\n\n请只输出填满全部字段的 JSON 对象：",
                    cur = if current.trim().is_empty() {
                        "（空）"
                    } else {
                        current.as_str()
                    },
                    note = note,
                )
            } else {
                format!(
                    "[Reference — all cards; stay consistent]\n{reference}\n\n[Current card]\n{cur}\n\n[Prompt and JSON schema]\n{note}\n\nOutput only the complete JSON object:",
                    cur = if current.trim().is_empty() {
                        "(empty)"
                    } else {
                        current.as_str()
                    },
                    note = note,
                )
            }
        } else if loc.is_zh() {
            format!(
                "【参考书（全书卡面；须与下列设定一致）】\n{reference}\n\n{body}",
                body = prompts::rewrite_text_field_user(loc, label, &current, note, &ctx),
            )
        } else {
            format!(
                "[Reference — all cards; stay consistent]\n{reference}\n\n{body}",
                body = prompts::rewrite_text_field_user(loc, label, &current, note, &ctx),
            )
        }
    } else {
        prompts::rewrite_text_field_user(loc, label, &current, note, &ctx)
    };
    let (content, _) = llm_complete_ex(
        Some(&app),
        &state.db,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id.as_str()),
        Some(cancel),
        false,
    )
    .await?;
    let out = strip_outer_md_fence(&content);
    if out.is_empty() {
        return Err(if loc.is_zh() {
            "模型未返回有效内容".into()
        } else {
            "Model returned empty text".into()
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerateWorldviewChatResult {
    pub assistant: String,
    pub worldview: serde_json::Value,
}

fn knowledge_by_slot<'a>(tree: &'a NovelTree, slot: &str) -> Option<&'a TreeNode> {
    tree.nodes.iter().find(|n| {
        matches!(n.kind, NodeKind::Knowledge)
            && n.knowledge
                .as_ref()
                .map(|k| k.slot.trim() == slot)
                .unwrap_or(false)
    })
}

fn worldview_snapshot_json(tree: &NovelTree) -> String {
    let slot_obj = |slot: &str| -> serde_json::Value {
        knowledge_by_slot(tree, slot)
            .and_then(|n| n.knowledge.as_ref())
            .map(|k| {
                serde_json::json!({
                    "extracted": k.extracted,
                    "core_laws": k.core_laws,
                    "spatiotemporal": k.spatiotemporal,
                    "social_power": k.social_power,
                    "existence": k.existence,
                    "info_flow": k.info_flow,
                    "history_culture": k.history_culture,
                })
            })
            .unwrap_or(serde_json::json!({}))
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "wv_core_laws": slot_obj("wv_core_laws"),
        "wv_spatiotemporal": slot_obj("wv_spatiotemporal"),
        "wv_social_power": slot_obj("wv_social_power"),
        "wv_existence": slot_obj("wv_existence"),
        "wv_info_flow": slot_obj("wv_info_flow"),
        "wv_history_culture": slot_obj("wv_history_culture"),
        "story_rules": knowledge_by_slot(tree, "story_rules")
            .and_then(|n| n.knowledge.as_ref())
            .map(|k| {
                serde_json::json!({
                    "extracted": k.extracted,
                })
            })
            .unwrap_or(serde_json::json!({})),
        "story_rules_blocks": crate::story_rules_ops::story_rules_blocks_snapshot(tree),
    }))
    .unwrap_or_else(|_| "{}".into())
}

pub fn worldview_snapshot_value(tree: &NovelTree) -> serde_json::Value {
    serde_json::from_str(&worldview_snapshot_json(tree)).unwrap_or(serde_json::json!({}))
}

fn parse_worldview_chat_json(text: &str) -> Result<(String, serde_json::Value), String> {
    for v in extract_json_objects(text) {
        let reply = v
            .get("reply")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if let Some(wv) = v.get("worldview") {
            if !reply.is_empty() {
                return Ok((reply, wv.clone()));
            }
        }
    }
    Err("未能解析世界观 JSON（需含 reply 与 worldview）".into())
}

fn mock_worldview_json() -> serde_json::Value {
    serde_json::json!({
        "core_laws": {
            "premise": "（Mock）齿轮与秘术共存的世界",
            "taboos": ["禁止篡改集体记忆"],
            "power_system": "蒸汽核心驱动魔法",
            "power_expression": "义体与符文并用",
            "axioms": [{
                "name": "等价交换",
                "statement": "魔法必消耗燃料",
                "boundary": "不能无中生有",
                "cost": "煤炭或生命",
                "mechanism": "锅炉符文阵"
            }]
        },
        "spatiotemporal": {
            "premise": "大陆被雾海分割",
            "era": "工业革命晚期",
            "ecology": "雾中异兽",
            "world_pattern": "双城对立",
            "atmosphere": "潮湿、煤烟、煤气灯",
            "locations": [{
                "name": "灰港",
                "features": "贸易枢纽",
                "terrain": "海湾",
                "faction": "商会"
            }]
        },
        "social_power": {
            "premise": "旧贵族与工坊主博弈",
            "class_structure": "三层：贵族/市民/劳工",
            "political_system": "议会制门面、寡头实权",
            "power_visibility": "明面法律、暗面行会",
            "races": [{
                "name": "人类",
                "features": "适应性强",
                "population": "多数",
                "social_status": "分层明显"
            }],
            "factions": [{
                "name": "铁冠议会",
                "faction_type": "寡头",
                "goal": "垄断蒸汽核心",
                "means": "专利与雇佣兵",
                "power_base": "工厂与舰队"
            }]
        },
        "existence": {
            "premise": "死亡可被记录但不可复生",
            "death": "灵魂散入雾海",
            "calendar": "帝国历",
            "lifespan": "约75年",
            "disease_reproduction": "瘟疫随雾季爆发"
        },
        "info_flow": {
            "premise": "电报与口信并存",
            "info_speed": "城间一日、跨洋一周",
            "info_barrier": "审查与加密",
            "rumor_truth": "官方与地下各一套",
            "knowledge_carrier": "报纸、行会密档"
        },
        "history_culture": {
            "premise": "雾海扩张迫使各城结盟",
            "customs": "祭雾节、行会密誓",
            "economy": "蒸汽贸易与专利垄断",
            "daily_slices": "煤气灯下的早市与夜班工厂",
            "religions": [{
                "name": "雾神教",
                "core_belief": "雾海是试炼",
                "followers_scope": "港口劳工与船员"
            }],
            "major_events": [{
                "title": "灰港条约",
                "event": "双城停战并瓜分航线",
                "long_term_impact": "商会寡头长期掌权"
            }]
        },
        "story_rules": {
            "extracted": "第三人称限知；每章结尾留悬念；对话简洁。"
        }
    })
}

/// 根节点世界观 Chat：多轮对话生成整套世界观 JSON；`slot` 非空时只补该卡全部字段。
pub async fn generate_worldview_chat_inner(
    app: Option<&tauri::AppHandle>,
    db: &crate::db::Db,
    novel_id: &str,
    messages: &[ChatTurn],
    model: Option<&str>,
    slot: Option<&str>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<GenerateWorldviewChatResult, String> {
    let json_key = match slot.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(
            crate::worldview_ops::worldview_json_key_for_slot(s)
                .ok_or_else(|| format!("未知世界观槽位: {s}"))?,
        ),
        None => None,
    };
    let latest = messages
        .iter()
        .rev()
        .find(|m| m.role == "user" && !m.content.trim().is_empty())
        .ok_or_else(|| "请发送消息".to_string())?;
    let tree = get_tree(novel_id.to_string())?;
    let novel = db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model);
    let chat_model = llm_model(&settings);
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
            latest.content.as_str(),
        ],
    );
    let snapshot = worldview_snapshot_json(&tree);
    let features_block = crate::novel_features::format_novel_features_block(&novel.features);
    let mut history = String::new();
    for m in messages {
        let role = m.role.trim();
        if role != "user" && role != "assistant" {
            continue;
        }
        let label = if role == "user" {
            if loc.is_zh() { "用户" } else { "User" }
        } else if loc.is_zh() {
            "助手"
        } else {
            "Assistant"
        };
        history.push_str(&format!("{label}：{}\n", m.content.trim()));
    }
    let system = match json_key {
        Some(key) => prompts::generate_worldview_slot_chat_system(loc, key),
        None => prompts::generate_worldview_chat_system(loc),
    };
    let user = prompts::generate_worldview_chat_user(
        loc,
        &novel.title,
        &novel.synopsis,
        root_outline,
        &features_block,
        &snapshot,
        &history,
        latest.content.trim(),
        json_key,
    );
    let (content, used_mock) = llm_complete_ex(
        app,
        db,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        cancel,
        true,
    )
    .await?;
    if used_mock {
        let mut worldview = mock_worldview_json();
        if let Some(key) = json_key {
            worldview = crate::worldview_ops::keep_worldview_slot(worldview, key);
        }
        return Ok(GenerateWorldviewChatResult {
            assistant: if loc.is_zh() {
                "（Mock）已生成一套示例世界观，请配置 API Key 后重新生成。".into()
            } else {
                "(Mock) Sample worldview generated — configure API Key for real output.".into()
            },
            worldview,
        });
    }
    let (assistant, mut worldview) = parse_worldview_chat_json(&content)?;
    if let Some(key) = json_key {
        worldview = crate::worldview_ops::keep_worldview_slot(worldview, key);
    }
    Ok(GenerateWorldviewChatResult {
        assistant,
        worldview,
    })
}

#[tauri::command]
pub async fn generate_worldview_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    messages: Vec<ChatTurn>,
    model: Option<String>,
    slot: Option<String>,
) -> Result<GenerateWorldviewChatResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    generate_worldview_chat_inner(
        Some(&app),
        &state.db,
        &novel_id,
        &messages,
        model.as_deref(),
        slot.as_deref(),
        Some(cancel),
    )
    .await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerateStoryRulesChatResult {
    pub assistant: String,
    pub blocks: serde_json::Value,
}

fn story_rules_blocks_snapshot_json(tree: &NovelTree) -> String {
    let rules = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Knowledge) && {
            n.knowledge
                .as_ref()
                .map(|k| k.slot.trim() == "story_rules")
                .unwrap_or(false)
        });
    if rules.is_none() {
        return "{}".into();
    }
    let rules = rules.unwrap();
    let mut obj = serde_json::Map::new();
    for child in crate::story_rules_fmt::linked_story_rules_blocks(tree, &rules.id) {
        let slot = crate::story_rules_fmt::knowledge_slot(child);
        let k = child.knowledge.as_ref();
        let val = match slot {
            "sr_surface_setting" => k.and_then(|x| x.surface_setting.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(serde_json::json!({}))
            }),
            "sr_story_engine" => k.and_then(|x| x.story_engine.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(serde_json::json!({}))
            }),
            "sr_fulfillment_system" => k.and_then(|x| x.fulfillment_system.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(serde_json::json!({}))
            }),
            "sr_constraint_redlines" => k.and_then(|x| x.constraint_redlines.as_ref()).map(|d| {
                serde_json::to_value(d).unwrap_or(serde_json::json!({}))
            }),
            _ => None,
        };
        if let Some(v) = val {
            let key = match slot {
                "sr_surface_setting" => "surface_setting",
                "sr_story_engine" => "story_engine",
                "sr_fulfillment_system" => "fulfillment_system",
                "sr_constraint_redlines" => "constraint_redlines",
                _ => continue,
            };
            obj.insert(key.into(), v);
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_else(|_| "{}".into())
}

fn coerce_story_rules_blocks(v: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(b) = v.get("blocks").filter(|x| x.is_object()) {
        return Some(b.clone());
    }
  // 模型常把四卡键放在顶层
    const KEYS: [&str; 4] = [
        "surface_setting",
        "story_engine",
        "fulfillment_system",
        "constraint_redlines",
    ];
    let mut obj = serde_json::Map::new();
    for k in KEYS {
        if let Some(val) = v.get(k).filter(|x| x.is_object()) {
            obj.insert(k.into(), val.clone());
        }
    }
    if obj.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(obj))
    }
}

fn story_rules_reply_from_value(v: &serde_json::Value, loc_is_zh: bool) -> String {
    for key in ["reply", "message", "assistant", "说明", "回复"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    if loc_is_zh {
        "已根据对话更新故事规则四卡。".into()
    } else {
        "Story rules four cards updated from chat.".into()
    }
}

fn story_rules_chapter_context(tree: &NovelTree, loc_is_zh: bool) -> String {
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
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.id.cmp(&b.id))
    });
    if chapters.is_empty() {
        return if loc_is_zh {
            "（尚无章节节点）".into()
        } else {
            "(No chapter nodes on tree)".into()
        };
    }
    let mut lines: Vec<String> = Vec::new();
    for (i, ch) in chapters.iter().enumerate() {
        let title = ch.label.trim();
        let outline = ch.outline.trim();
        if outline.is_empty() {
            lines.push(format!("{}. {}", i + 1, title));
        } else {
            lines.push(format!("{}. {} — {}", i + 1, title, outline));
        }
    }
    lines.join("\n")
}

fn parse_story_rules_chat_json(text: &str, loc_is_zh: bool) -> Result<(String, serde_json::Value), String> {
    for v in extract_json_objects(text) {
        if let Some(blocks) = coerce_story_rules_blocks(&v) {
            return Ok((story_rules_reply_from_value(&v, loc_is_zh), blocks));
        }
    }
    Err(if loc_is_zh {
        "未能解析故事规则 JSON（需含 reply 与 blocks，或顶层 surface_setting / story_engine / fulfillment_system / constraint_redlines）".into()
    } else {
        "Could not parse story-rules JSON (need reply+blocks or top-level four card objects)".into()
    })
}

/// 故事规则 Chat：生成右侧四卡 JSON。
pub async fn generate_story_rules_chat_inner(
    app: Option<&tauri::AppHandle>,
    db: &crate::db::Db,
    novel_id: &str,
    messages: &[ChatTurn],
    model: Option<&str>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<GenerateStoryRulesChatResult, String> {
    let latest = messages
        .iter()
        .rev()
        .find(|m| m.role == "user" && !m.content.trim().is_empty())
        .ok_or_else(|| "请发送消息".to_string())?;
    let tree = get_tree(novel_id.to_string())?;
    let novel = db
        .get_novel(novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let mut settings = db.get_settings().map_err(|e| e.to_string())?;
    apply_chat_model_override(&mut settings.chat_model, model);
    let chat_model = llm_model(&settings);
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[novel.title.as_str(), novel.synopsis.as_str(), latest.content.as_str()],
    );
    let features_block = crate::novel_features::format_novel_features_block(&novel.features);
    let wv_snapshot = worldview_snapshot_json(&tree);
    let blocks_snapshot = story_rules_blocks_snapshot_json(&tree);
    let chapter_context = story_rules_chapter_context(&tree, loc.is_zh());
    let novel_reference =
        assemble_novel_rewrite_reference(&novel, &tree, None, None, loc.is_zh());
    let mut history = String::new();
    for m in messages {
        let role = m.role.trim();
        if role != "user" && role != "assistant" {
            continue;
        }
        let label = if role == "user" {
            if loc.is_zh() { "用户" } else { "User" }
        } else if loc.is_zh() {
            "助手"
        } else {
            "Assistant"
        };
        history.push_str(&format!("{label}：{}\n", m.content.trim()));
    }
    let system = prompts::generate_story_rules_chat_system(loc);
    let user = prompts::generate_story_rules_chat_user(
        loc,
        &novel.title,
        &novel.synopsis,
        &features_block,
        &wv_snapshot,
        &blocks_snapshot,
        &chapter_context,
        &novel_reference,
        &history,
        latest.content.trim(),
    );
    let (content, used_mock) = llm_complete_ex(
        app,
        db,
        &settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        cancel,
        true,
    )
    .await?;
    let (assistant, blocks) = if used_mock {
        (
            "（Mock）已生成故事规则四卡草案。".into(),
            serde_json::json!({
                "surface_setting": {
                    "premise": "第三人称限知；每章留悬念",
                    "core_conflict": "真相与生存",
                    "reader_promise": "每章爽点与钩子",
                    "target_audience": "都市悬疑爱好者",
                    "tone_reference": "冷峻快节奏",
                    "commercial_tags": "悬疑、反转",
                    "extended_premise": "主角在规则游戏中求生"
                },
                "story_engine": {
                    "premise": "双线并进",
                    "bright_line": "追查案件",
                    "dark_line": "幕后操盘",
                    "suspense_setup": "每章揭示一层",
                    "conflict_engine": "规则惩罚升级",
                    "external_conflict": "势力围堵",
                    "internal_conflict": "信任崩塌",
                    "relational_conflict": "盟友互疑",
                    "progression_cycle": "危机-破局-新危机",
                    "protagonist_dilemma": "救人或通关"
                },
                "fulfillment_system": {
                    "premise": "小胜累积大反转",
                    "growth_path": "认知与能力同步升级",
                    "ending_texture": "释然中带代价",
                    "payoff_syntax": ["绝境反杀", "信息差翻盘"],
                    "emotional_rhythm": "压-放-再压",
                    "tension_archetypes": ["章末钩子", "卷末真相"]
                },
                "constraint_redlines": {
                    "premise": "严禁在前期解释全部规则",
                    "redlines": ["禁止说教式世界观灌输", "禁止主角全知"]
                }
            }),
        )
    } else {
        parse_story_rules_chat_json(&content, loc.is_zh())?
    };
    Ok(GenerateStoryRulesChatResult {
        assistant,
        blocks,
    })
}

#[tauri::command]
pub async fn generate_story_rules_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    messages: Vec<ChatTurn>,
    model: Option<String>,
) -> Result<GenerateStoryRulesChatResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };
    generate_story_rules_chat_inner(
        Some(&app),
        &state.db,
        &novel_id,
        &messages,
        model.as_deref(),
        Some(cancel),
    )
    .await
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
    let mut plot_ids = crate::tree_links::chapter_local_plot_ids(tree, node_id);
    for pid in crate::tree_links::chapter_inherited_plot_ids(tree, node_id) {
        if !plot_ids.iter().any(|id| id == &pid) {
            plot_ids.push(pid);
        }
    }

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
        if matches!(other.kind, NodeKind::Character)
            && !char_ids.iter().any(|id| id == other_id)
        {
            char_ids.push(other_id.to_string());
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
            let body = cap(
                &crate::character_fmt::format_character_full(&n.label, c),
                crate::character_fmt::CHARACTER_INJECT_CAP,
            );
            chars.push_str(&format!("- 「{}」\n{}\n", n.label, body));
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
    let model = llm_model(&settings);
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
                    detailed_outline: vec![],
                    character: None,
                    knowledge: None,
                    side_plot: Some(SidePlotMeta {
                        status,
                        absorbed: false,
                    }),
                    volume: None,
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
    if created > 0 {
        crate::tree_layout::apply_auto_layout(&mut tree);
    }
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

    let chat_model = llm_model(&settings);
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
    );
    if use_chapter_progress {
        emit_chapter_progress(app, novel_id, "planning", 2, TOTAL);
    } else {
        emit_chat_progress(app, novel_id, "thinking");
    }
    let (reply, used_mock) = llm_complete_ex(
        Some(app),
        &state.db,
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

/// 把知识卡关联公共库的正文写入 `extracted`（完整导入，有字数上限）。
pub fn fill_knowledge_card_inner(
    db: &Db,
    novel_id: &str,
    node_id: &str,
    book_ids: Option<Vec<String>>,
    max_chars: usize,
) -> Result<NovelTree, String> {
    let mut tree = get_tree(novel_id.to_string())?;
    let idx = tree
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    if !matches!(tree.nodes[idx].kind, NodeKind::Knowledge) {
        return Err("不是知识卡".into());
    }
    let mut payload = tree.nodes[idx].knowledge.clone().unwrap_or_default();
    if let Some(ids) = book_ids {
        payload.book_ids = ids;
    }
    if payload.book_ids.is_empty() {
        return Err("请先选择至少一个知识库".into());
    }
    let corpus = crate::kb_context::gather_knowledge_corpus(db, &payload.book_ids, max_chars.max(200))
        .map_err(|e| e.to_string())?;
    if corpus.trim().is_empty() {
        return Err("所选知识库没有可导入的分段（可先在知识库页重建索引）".into());
    }
    payload.extracted = corpus;
    let n = &mut tree.nodes[idx];
    n.outline = payload.extracted.chars().take(200).collect();
    n.knowledge = Some(payload);
    save_tree(tree.clone())?;
    Ok(tree)
}

#[tauri::command]
pub fn fill_knowledge_card(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<NovelTree, String> {
    fill_knowledge_card_inner(&state.db, &novel_id, &node_id, None, 14_000)
}

#[tauri::command]
pub fn list_public_knowledge_cards(
    state: State<'_, AppState>,
) -> Result<Vec<PublicKnowledgeCard>, String> {
    state
        .db
        .list_public_knowledge_cards()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_public_knowledge_card(
    state: State<'_, AppState>,
    id: Option<String>,
    title: String,
    book_ids: Vec<String>,
    extract_prompt: String,
    extracted: String,
) -> Result<PublicKnowledgeCard, String> {
    upsert_public_knowledge_card_inner(
        &state.db,
        id,
        title,
        book_ids,
        extract_prompt,
        extracted,
    )
}

#[tauri::command]
pub fn archive_public_knowledge_card(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<PublicKnowledgeCard, String> {
    state
        .db
        .set_public_knowledge_card_archived(&id, archived)
        .map_err(|e| e.to_string())?;
    state
        .db
        .get_public_knowledge_card(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "公共知识卡不存在".to_string())
}

#[tauri::command]
pub fn delete_public_knowledge_card(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .delete_public_knowledge_card(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_public_knowledge_card(
    state: State<'_, AppState>,
    novel_id: String,
    public_id: String,
    host_node_id: Option<String>,
) -> Result<NovelTree, String> {
    add_public_knowledge_card_inner(
        &state.db,
        &novel_id,
        &public_id,
        host_node_id.as_deref(),
    )
    .map(|(tree, _)| tree)
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
        let summary = c
            .map(|x| crate::character_fmt::format_character_full(&n.label, x))
            .unwrap_or_default();
        let one_line = summary
            .chars()
            .take(280)
            .collect::<String>()
            .replace('\n', " · ");
        char_lines.push(format!(
            "- {} | {}",
            n.label,
            if one_line.is_empty() {
                format!(
                    "role: {} | gender: {} | age: {}",
                    c.map(|x| x.role.as_str()).unwrap_or(""),
                    c.map(|x| x.gender.as_str()).unwrap_or(""),
                    c.map(|x| x.age.as_str()).unwrap_or(""),
                )
            } else {
                one_line
            }
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
    let model = image_prompt_model(&settings);
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

fn fallback_character_sheet_prompt(name: &str, card: &crate::models::CharacterCard) -> String {
    let mut bits = vec![name.trim().to_string()];
    for s in [
        card.gender.as_str(),
        card.age.as_str(),
        card.role.as_str(),
        card.style.as_str(),
        card.voice.body_language.as_str(),
    ] {
        if !s.trim().is_empty() {
            bits.push(s.trim().to_string());
        }
    }
    format!(
        "character design turnaround sheet of one person, four full-body views left to right: front view, left profile, back view, right profile, same face outfit and proportions, full body head to toe, even spacing, plain light gray studio background, {}, clean illustration, no text labels, no extra characters",
        bits.join(", ")
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterSheetResult {
    pub prompt: String,
    pub image_path: String,
}

/// 根据人物卡生成四向全身设定图英文提示词。
#[tauri::command]
pub async fn generate_character_sheet_prompt(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<String, String> {
    let tree = get_tree(novel_id.clone())?;
    let n = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id && matches!(n.kind, NodeKind::Character))
        .ok_or_else(|| "人物卡不存在".to_string())?;
    let card = n.character.clone().unwrap_or_default();
    let md = crate::character_fmt::format_character_full(&n.label, &card);
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let model = image_prompt_model(&settings);
    let sys = prompts::character_sheet_t2i_system();
    let user = prompts::character_sheet_t2i_user(&n.label, &md);
    match llm_complete(
        None,
        &state,
        &settings,
        sys,
        &user,
        Some(&model),
        Some(&novel_id),
        None,
    )
    .await
    {
        Ok((out, _)) => {
            let prompt = out.trim().trim_matches('"').trim().to_string();
            if prompt.is_empty() {
                Ok(fallback_character_sheet_prompt(&n.label, &card))
            } else {
                Ok(prompt)
            }
        }
        Err(_) => Ok(fallback_character_sheet_prompt(&n.label, &card)),
    }
}

/// 用 ComfyUI 文生图工作流生成四向全身设定图，并保存到本书目录。
#[tauri::command]
pub async fn generate_character_sheet(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    prompt: String,
) -> Result<CharacterSheetResult, String> {
    let tree = get_tree(novel_id.clone())?;
    let n = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id && matches!(n.kind, NodeKind::Character))
        .ok_or_else(|| "人物卡不存在".to_string())?;
    let card = n.character.clone().unwrap_or_default();
    let prompt = {
        let t = prompt.trim();
        if t.is_empty() {
            fallback_character_sheet_prompt(&n.label, &card)
        } else {
            t.to_string()
        }
    };
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let wf_raw = settings.comfyui_image_workflow.trim();
    if wf_raw.is_empty() {
        return Err("请先在设置粘贴 ComfyUI 文生图「导出（API）」工作流 JSON".into());
    }
    let wf: serde_json::Value =
        serde_json::from_str(wf_raw).map_err(|_| "文生图工作流 JSON 无效".to_string())?;
    crate::chapter_shots::probe_comfy(&settings.comfyui_url).await?;
    let injected =
        crate::chapter_shots::inject_prompt(&wf, &prompt, &settings.comfyui_prompt_node)?;
    let prompt_id =
        crate::chapter_shots::queue_prompt(&settings.comfyui_url, &injected).await?;
    let imgs = crate::chapter_shots::wait_for_output_images(
        &settings.comfyui_url,
        &prompt_id,
        std::time::Duration::from_secs(180),
    )
    .await?;
    let bytes = crate::chapter_shots::download_view(&settings.comfyui_url, &imgs[0]).await?;
    let dir = crate::paths::novel_dir(&novel_id).join("characters");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{node_id}-sheet.png"));
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(CharacterSheetResult {
        prompt,
        image_path: dest.to_string_lossy().to_string(),
    })
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
    let settings = db.get_settings().unwrap_or_default();
    crate::ai_log::set_enabled(settings.ai_interaction_log);
    let port = if settings.mcp_port == 0 {
        crate::mcp::DEFAULT_MCP_PORT
    } else {
        settings.mcp_port
    };
    Ok(AppState {
        db: Arc::new(db),
        chat_cancel: Default::default(),
        mcp: Arc::new(crate::mcp::McpRuntime::new(port)),
        app_handle: Arc::new(Mutex::new(None)),
        global_chat: Arc::new(crate::global_chat::GlobalChatRuntime::new()),
    })
}

#[tauri::command]
pub fn global_chat_list(
    state: State<'_, AppState>,
    novel_id: Option<String>,
) -> Result<Vec<crate::global_chat::GlobalChatMessage>, String> {
    Ok(state.global_chat.list(novel_id.as_deref()))
}

#[tauri::command]
pub async fn global_chat_send(
    state: State<'_, AppState>,
    content: String,
    route_name: Option<String>,
    path: Option<String>,
    novel_id: Option<String>,
    model: Option<String>,
) -> Result<Vec<crate::global_chat::GlobalChatMessage>, String> {
    let cancel = state.arm_chat_cancel(crate::global_chat::CANCEL_KEY);
    let _clear = ClearChatCancel {
        state: &*state,
        key: crate::global_chat::CANCEL_KEY.to_string(),
    };
    crate::global_chat::send(
        state.db.clone(),
        state.mcp.clone(),
        state.app_handle.clone(),
        state.global_chat.clone(),
        cancel,
        crate::global_chat::GlobalChatSendInput {
            content,
            route_name,
            path,
            novel_id,
            model,
        },
    )
    .await
}

#[tauri::command]
pub fn global_chat_clear(state: State<'_, AppState>, novel_id: Option<String>) -> Result<(), String> {
    state.global_chat.clear(novel_id.as_deref());
    Ok(())
}

#[tauri::command]
pub fn global_chat_cancel(state: State<'_, AppState>) -> Result<(), String> {
    state.request_chat_cancel(crate::global_chat::CANCEL_KEY);
    state.global_chat.abort_ask();
    Ok(())
}

#[tauri::command]
pub fn global_chat_answer_ask(
    state: State<'_, AppState>,
    answers: Vec<String>,
) -> Result<(), String> {
    state.global_chat.answer_ask(answers)
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
    fn parse_body_suggestions_json_object() {
        let raw = r#"{"suggestions":["甲","乙","丙","丁","戊","己"]}"#;
        assert_eq!(
            parse_body_suggestions(raw).unwrap(),
            vec!["甲", "乙", "丙", "丁", "戊", "己"]
        );
    }

    #[test]
    fn parse_body_suggestions_rejects_short() {
        assert!(parse_body_suggestions(r#"{"suggestions":["甲","乙"]}"#).is_none());
    }

    #[test]
    fn parse_story_rules_chat_flat_top_level() {
        let raw = r#"{"reply":"好的","surface_setting":{"premise":"a"},"story_engine":{"premise":"b"}}"#;
        let (reply, blocks) = parse_story_rules_chat_json(raw, true).expect("parse");
        assert_eq!(reply, "好的");
        assert!(blocks.get("surface_setting").is_some());
        assert!(blocks.get("story_engine").is_some());
    }

    #[test]
    fn parse_story_rules_chat_blocks_wrapper() {
        let raw = r#"{"blocks":{"constraint_redlines":{"premise":"x","redlines":["y"]}}}"#;
        let (reply, blocks) = parse_story_rules_chat_json(raw, true).expect("parse");
        assert!(reply.contains("故事规则"));
        assert!(blocks.get("constraint_redlines").is_some());
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
    fn chapter_label_and_outline_helpers() {
        let coerced = coerce_outlines_to_single_num(
            vec![serde_json::json!({"n": 1, "label": "第一章 · x", "outline": "a"})],
            5,
        );
        assert_eq!(coerced[0].get("n").and_then(|x| x.as_u64()), Some(5));
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
                    detailed_outline: vec![],
                    character: None,
                    knowledge: None,
                    side_plot: None,
                    volume: None,
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
                    detailed_outline: vec![],
                    character: None,
                    knowledge: None,
                    side_plot: None,
                    volume: None,
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
    }

    fn tn(id: &str, kind: NodeKind) -> TreeNode {
        TreeNode {
            id: id.into(),
            kind,
            label: id.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition { x: 0.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        }
    }

    #[test]
    fn knowledge_attach_host_uses_chapter_or_parent() {
        let mut tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                tn("root", NodeKind::Novel),
                tn("ch", NodeKind::Chapter),
                tn("kb", NodeKind::Knowledge),
            ],
            edges: vec![],
        };
        tree.nodes[1].linked_knowledge_ids = vec!["kb".into()];
        assert_eq!(knowledge_attach_host(&tree, None).unwrap(), "root");
        assert_eq!(knowledge_attach_host(&tree, Some("ch")).unwrap(), "ch");
        assert_eq!(knowledge_attach_host(&tree, Some("kb")).unwrap(), "ch");
        assert_eq!(knowledge_attach_host(&tree, Some("root")).unwrap(), "root");
    }

    #[test]
    fn resolve_node_ref_accepts_chapter_number_and_label() {
        let mut c1 = tn("ch-aaa", NodeKind::Chapter);
        c1.label = "第一章 · 夜色".into();
        c1.position.y = 10.0;
        let mut c2 = tn("ch-bbb", NodeKind::Chapter);
        c2.label = "第二章".into();
        c2.position.y = 20.0;
        let mut hero = tn("char-1", NodeKind::Character);
        hero.label = "林晚".into();
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![tn("root", NodeKind::Novel), c1, c2, hero],
            edges: vec![],
        };
        assert_eq!(resolve_node_ref(&tree, "ch-aaa").unwrap(), "ch-aaa");
        assert_eq!(resolve_node_ref(&tree, "第1章").unwrap(), "ch-aaa");
        assert_eq!(resolve_node_ref(&tree, "1").unwrap(), "ch-aaa");
        assert_eq!(resolve_node_ref(&tree, "第二章").unwrap(), "ch-bbb");
        assert_eq!(resolve_node_ref(&tree, "林晚").unwrap(), "char-1");
        assert_eq!(resolve_node_ref(&tree, "root").unwrap(), "root");
        let err = resolve_node_ref(&tree, "不存在的卡").unwrap_err();
        assert!(err.contains("无法找到"), "{err}");
        assert!(err.contains("第一章"), "{err}");
        // UUID 里的数字不能当成章号
        let err = resolve_node_ref(&tree, "ch-deadbeef-0002").unwrap_err();
        assert!(err.contains("无法找到"), "{err}");
    }
}


















































