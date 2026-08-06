mod catalog;
mod chunk;
mod commands;
mod db;
mod knowledge;
mod llm;
mod models;
mod paths;
mod prompts;
mod sample;
mod secret;

use std::sync::Arc;
use tauri::Manager;

pub struct AppState {
    pub db: Arc<db::Db>,
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
            commands::list_genres,
            commands::list_knowledge_bases,
            commands::import_knowledge_text,
            commands::list_novels,
            commands::get_novel,
            commands::archive_novel,
            commands::delete_novel,
            commands::create_novel,
            commands::create_novel_chat,
            commands::get_tree,
            commands::save_tree,
            commands::get_chapter,
            commands::generate_chapter,
            commands::refine_chapter,
            commands::list_chat_messages,
            commands::chat_send,
            commands::card_chat_send,
            commands::pick_cover,
            commands::set_cover,
            commands::pick_text_file,
            commands::app_data_root,
            commands::get_token_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nove Work");
}
