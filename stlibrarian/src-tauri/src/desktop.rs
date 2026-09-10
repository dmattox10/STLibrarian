use crate::core::db;
use crate::core::models::{LibraryQuery, LibraryStats, ModelRecord, ScanReport, TagCount};
use crate::core::paths;
use crate::core::scanner;
use crate::core::thumbs;
use tauri::{command, Emitter};
use tauri_plugin_dialog::DialogExt;

fn conn() -> Result<rusqlite::Connection, String> {
    db::open_default().map_err(|e| format!("Could not open library: {}", e))
}

#[command]
fn library(query: Option<LibraryQuery>) -> Result<Vec<ModelRecord>, String> {
    let c = conn()?;
    db::list_models(&c, &query.unwrap_or_default()).map_err(|e| e.to_string())
}

#[command]
fn model(id: i64) -> Result<Option<ModelRecord>, String> {
    let c = conn()?;
    db::get_model(&c, id).map_err(|e| e.to_string())
}

#[command]
fn tags() -> Result<Vec<TagCount>, String> {
    let c = conn()?;
    db::list_tags(&c).map_err(|e| e.to_string())
}

#[command]
fn stats() -> Result<LibraryStats, String> {
    let c = conn()?;
    let mut s = db::library_stats(&c).map_err(|e| e.to_string())?;
    s.missing = db::find_missing(&c).map(|m| m.len() as i64).unwrap_or(0);
    Ok(s)
}

#[command]
async fn scan(app: tauri::AppHandle, path: String) -> Result<ScanReport, String> {
    // A library scan is minutes of blocking I/O. Running it on the async
    // runtime's worker would freeze every other command, the window included.
    tauri::async_runtime::spawn_blocking(move || {
        let c = conn()?;
        Ok(scanner::scan_folder_with(&c, &path, |progress| {
            app.emit("scan:progress", progress).ok();
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[command]
fn set_tags(model_id: i64, tags: Vec<String>) -> Result<(), String> {
    let c = conn()?;
    db::set_tags(&c, model_id, &tags).map_err(|e| e.to_string())
}

#[command]
fn delete_model(model_id: i64) -> Result<(), String> {
    let c = conn()?;
    db::delete_model(&c, model_id).map_err(|e| e.to_string())
}

#[command]
fn get_thumb(hash: String) -> Option<String> {
    thumbs::load_data_url(&hash)
}

#[command]
fn put_thumb(hash: String, data_url: String) -> Result<(), String> {
    thumbs::save(&hash, &data_url)?;
    let c = conn()?;
    db::set_has_thumb(&c, &hash, true).map_err(|e| e.to_string())
}

/// Hand the viewer the raw STL bytes.
///
/// Addressed by database id, never by a path from the webview. The renderer can
/// therefore only ever read files the scanner already indexed -- it cannot ask
/// for `~/.ssh/id_rsa`.
#[command]
fn read_model(id: i64) -> Result<tauri::ipc::Response, String> {
    let c = conn()?;
    let record = db::get_model(&c, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No such model".to_string())?;
    let bytes = std::fs::read(&record.path)
        .map_err(|e| format!("Could not read {}: {}", record.path, e))?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| p.to_string())
    })
    .await
    .ok()
    .flatten()
}

#[command]
fn prune_thumbs() -> Result<usize, String> {
    let c = conn()?;
    let live = db::all_hashes(&c).map_err(|e| e.to_string())?;
    Ok(thumbs::prune(&live))
}

#[command]
fn missing_files() -> Result<Vec<ModelRecord>, String> {
    let c = conn()?;
    let ids = db::find_missing(&c).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, _) in ids {
        if let Ok(Some(m)) = db::get_model(&c, id) {
            out.push(m);
        }
    }
    Ok(out)
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = paths::ensure_dirs() {
        log::error!("Could not create data directory: {}", e);
    }
    log::info!("Library at {}", paths::data_dir().display());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|_app| {
            // Surface a broken library at startup rather than on the first query.
            if let Err(e) = db::open_default() {
                log::error!("Could not open library database: {}", e);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library,
            model,
            tags,
            stats,
            scan,
            set_tags,
            delete_model,
            get_thumb,
            put_thumb,
            read_model,
            pick_folder,
            prune_thumbs,
            missing_files
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
