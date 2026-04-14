pub mod ai_insights;
pub mod classifier;
pub mod commands;
pub mod model_manager;
pub mod query_engine;
pub mod suggester;

use ai_insights::{generate_ai_insights, natural_language_to_sql, InsightEngine};
use commands::{add_recent_file, create_duckdb, get_recent_files, load_csv_into_duckdb, parse_csv, query_duckdb, DuckDbState};
use model_manager::{check_model_status, start_model_download, ModelManager};
use query_engine::translate_query;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");
            let model_mgr = Arc::new(ModelManager::new(data_dir));
            let engine = Arc::new(Mutex::new(InsightEngine::new()));
            app.manage(model_mgr.clone());
            app.manage(engine.clone());
            app.manage(Arc::new(Mutex::new(create_duckdb())) as DuckDbState);

            // Preload AI model in background if already downloaded
            if model_mgr.is_downloaded() {
                let model_path = model_mgr.model_path();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let mut engine = engine.lock().await;
                        if let Err(e) = engine.load_model(&model_path) {
                            eprintln!("Background model preload failed: {}", e);
                        }
                    });
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_csv,
            get_recent_files,
            add_recent_file,
            check_model_status,
            start_model_download,
            generate_ai_insights,
            load_csv_into_duckdb,
            query_duckdb,
            natural_language_to_sql,
            translate_query
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
