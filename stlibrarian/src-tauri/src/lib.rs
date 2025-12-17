use tauri::command;
mod core;
use core::auth::*;
use core::db::*;
use core::models::ModelRecord;
use core::scanner::*;

#[command]
fn scan(path: String) {
    let conn = open_db("stl.db").unwrap();
    scan_folder(&conn, &path);
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

// ----------------- New commands -----------------

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

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scan,
            models,
            login_cmd,
            set_tags_cmd,
            delete_model_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
