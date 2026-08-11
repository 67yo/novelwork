mod catalog;
mod chapter_constraints;
mod chapter_memory;
mod chat_tools;
mod chunk;
mod commands;
mod db;
mod kb_context;
mod knowledge;
mod llm;
mod models;
mod paths;
mod prompts;
mod sample;
mod secret;
mod skills;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub struct AppState {
    pub db: Arc<db::Db>,
    /// 按 novel_id（开书 `__create__`、知识库提取 `__knowledge_extract__`）挂起的取消标志
    pub chat_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
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
            commands::knowledge_extract_chat,
            commands::reextract_knowledge,
            commands::knowledge_index_status,
            commands::rebuild_knowledge_index,
            commands::list_novels,
            commands::get_novel,
            commands::update_novel_plan,
            commands::update_novel_canon,
            commands::sync_canon_settings,
            commands::archive_novel,
            commands::delete_novel,
            commands::create_novel,
            commands::create_novel_chat,
            commands::get_tree,
            commands::save_tree,
            commands::delete_tree_card,
            commands::get_chapter,
            commands::save_chapter,
            commands::get_chapter_memory,
            commands::list_all_chapter_memory,
            commands::set_chapter_memory,
            commands::regenerate_chapter_memory,
            commands::preview_generate_chapter,
            commands::generate_chapter,
            commands::refine_chapter,
            commands::preview_chapter_outline_brief,
            commands::plan_next_chapters,
            commands::regenerate_chapter_outline,
            commands::generate_chapter_plots,
            commands::consolidate_plot_cards,
            commands::generate_chapter_cards,
            commands::list_chat_messages,
            commands::list_card_chat_messages,
            commands::chat_send,
            commands::chat_cancel,
            commands::card_chat_send,
            commands::extract_knowledge_card,
            commands::generate_cover_prompt,
            commands::pick_cover,
            commands::set_cover,
            commands::pick_text_file,
            commands::app_data_root,
            commands::list_chat_skills,
            commands::preview_chat_skill_match,
            commands::get_token_usage,
            commands::get_token_usage_month,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nove Work");
}
