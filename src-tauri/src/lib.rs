mod novel_features;
mod worldview_ops;
mod catalog;
mod chapter_constraints;
mod chapter_memory;
mod chapter_shots;
mod chapter_tts;
mod chunk;
mod commands;
mod core_laws_fmt;
mod db;
mod ai_log;
mod compat_messages;
mod global_chat;
mod kb_context;
mod knowledge;
mod knowledge_vec;
mod llm;
mod llm_extract;
mod mcp;
mod models;
mod paths;
mod prompts;
mod sample;
mod info_flow_fmt;
mod existence_fmt;
mod character_fmt;
mod history_culture_fmt;
mod social_power_fmt;
mod spatiotemporal_fmt;
mod story_rules_ops;
mod story_rules_fmt;
mod volume_fmt;
mod secret;
mod skills;
mod tree_layout;
mod tree_links;
mod write_prompts;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub struct AppState {
    pub db: Arc<db::Db>,
    /// 按 novel_id（开书 `__create__`）挂起的取消标志
    pub chat_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub mcp: Arc<mcp::McpRuntime>,
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    pub global_chat: Arc<global_chat::GlobalChatRuntime>,
}

impl AppState {
    pub fn arm_chat_cancel(&self, key: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.chat_cancel
            .lock()
            .unwrap()
            .insert(key.to_string(), flag.clone());
        flag
    }

    pub fn request_chat_cancel(&self, key: &str) {
        if let Some(f) = self.chat_cancel.lock().unwrap().get(key) {
            f.store(true, Ordering::SeqCst);
        }
    }

    pub fn clear_chat_cancel(&self, key: &str) {
        self.chat_cancel.lock().unwrap().remove(key);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = commands::init_state().expect("init app state");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            let handle = app.handle().clone();
            {
                use tauri::path::BaseDirectory;
                let bundled = handle
                    .path()
                    .resolve("skills", BaseDirectory::Resource)
                    .ok()
                    .filter(|p| p.is_dir());
                crate::skills::set_bundled_dir(bundled);
            }
            {
                let st = app.state::<AppState>();
                *st.app_handle.lock().unwrap() = Some(handle);
                let settings = st.db.get_settings().unwrap_or_default();
                let ctx = mcp::McpCtx {
                    db: st.db.clone(),
                    app: st.app_handle.clone(),
                    selection: st.mcp.selection.clone(),
                };
                mcp::restart(
                    ctx,
                    st.mcp.clone(),
                    settings.mcp_enabled,
                    settings.mcp_port,
                    settings.mcp_lan,
                );
            }
            let db = app.state::<AppState>().db.clone();
            // ponytail: fire-and-forget catalog refresh; UI reads last known seed until done
            tauri::async_runtime::spawn(async move {
                let _ = crate::catalog::refresh(&db).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::open_ai_log_dir,
            commands::get_mcp_status,
            commands::restart_mcp_server,
            commands::refresh_model_catalog,
            commands::fetch_compat_models,
            commands::refresh_compat_provider_models,
            commands::list_genres,
            commands::list_knowledge_bases,
            commands::rename_knowledge,
            commands::update_knowledge,
            commands::list_knowledge_chunks,
            commands::archive_knowledge,
            commands::delete_knowledge,
            commands::import_knowledge_text,
            commands::import_knowledge_url,
            commands::reextract_knowledge,
            commands::knowledge_index_status,
            commands::rebuild_knowledge_index,
            commands::list_novels,
            commands::get_novel,
            commands::update_novel_plan,
            commands::update_novel_features,
            commands::archive_novel,
            commands::delete_novel,
            commands::create_novel,
            commands::create_novel_chat,
            commands::get_tree,
            commands::save_tree,
            commands::save_tree_json,
            commands::delete_tree_card,
            commands::get_chapter,
            commands::play_chapter_tts,
            commands::stop_chapter_tts,
            commands::save_chapter,
            commands::get_chapter_memory,
            commands::list_all_chapter_memory,
            commands::set_chapter_memory,
            commands::regenerate_chapter_memory,
            commands::get_chapter_shots,
            commands::set_chapter_shots,
            commands::split_chapter_shots,
            commands::generate_shot_comfy_prompts,
            commands::submit_chapter_shots_comfyui,
            commands::preview_generate_chapter,
            commands::generate_chapter,
            commands::generate_detailed_outline,
            commands::regenerate_detailed_outline_item,
            commands::refine_chapter,
            commands::rewrite_chapter_paragraph,
            commands::suggest_body_next,
            commands::rewrite_text_field,
            commands::generate_worldview_chat,
            commands::generate_story_rules_chat,
            commands::preview_chapter_outline_brief,
            commands::plan_next_chapters,
            commands::regenerate_chapter_outline,
            commands::generate_chapter_plots,
            commands::consolidate_plot_cards,
            commands::generate_chapter_cards,
            commands::chat_cancel,
            commands::fill_knowledge_card,
            commands::list_public_knowledge_cards,
            commands::upsert_public_knowledge_card,
            commands::archive_public_knowledge_card,
            commands::delete_public_knowledge_card,
            commands::add_public_knowledge_card,
            commands::generate_cover_prompt,
            commands::generate_character_sheet_prompt,
            commands::generate_character_sheet,
            commands::pick_cover,
            commands::set_cover,
            commands::pick_text_file,
            commands::app_data_root,
            commands::list_chat_skills,
            commands::preview_chat_skill_match,
            commands::get_token_usage,
            commands::get_token_usage_month,
            commands::set_workspace_selection,
            commands::global_chat_list,
            commands::global_chat_send,
            commands::global_chat_clear,
            commands::global_chat_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novel Work");
}
