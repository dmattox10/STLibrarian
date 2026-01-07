mod core;

use crate::core::auth::*;
use crate::core::db::*;
use crate::core::models::ModelRecord;
use crate::core::scanner::scan_folder_with_events; // the new scanner that emits progress
use tauri::{command, Window};

// ----------------- Commands -----------------

#[command]
async fn scan(path: String, window: Window) -> Result<(), String> {
    let conn = open_db("stl.db").map_err(|e| e.to_string())?;

    // Spawn background scan so frontend isn't blocked
    tauri::async_runtime::spawn(async move {
        scan_folder_with_events(&conn, &path, &window);
    });

    Ok(())
}

#[command]
fn models() -> Vec<ModelRecord> {
    let conn = open_db("stl.db").unwrap();
    list_models(&conn).unwrap_or_default()
}

#[command]
fn login_cmd(user: String, pass: String) -> Option<AuthResponse> {
    login(&user, &pass)
}

#[command]
fn set_tags_cmd(model_id: i64, tags: Vec<String>) {
    let conn = open_db("stl.db").unwrap();
    set_tags(&conn, model_id, &tags);
}

#[command]
fn delete_model_cmd(model_id: i64) {
    let conn = open_db("stl.db").unwrap();
    delete_model(&conn, model_id);
}

// ----------------- Run App -----------------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init()) // dialog plugin
        .invoke_handler(tauri::generate_handler![
            scan, // only one scan now
            models,
            login_cmd,
            set_tags_cmd,
            delete_model_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
