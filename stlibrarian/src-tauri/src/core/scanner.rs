// core/scanner.rs
use crate::core::db::{insert_model, tag_model};
use log::{error, info};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use tauri::{Emitter, Window}; // <--- Emitter trait added

pub fn scan_folder_with_events(conn: &Connection, root: &str, window: &Window) {
    info!("Scanning folder: {}", root);
    if let Err(e) = walk(conn, Path::new(root), window) {
        error!("Scan error: {:?}", e);
    }
}

fn walk(conn: &Connection, path: &Path, window: &Window) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();

        if p.is_dir() {
            walk(conn, &p, window)?; // recurse
        } else if is_stl(&p) {
            if let Err(e) = index_file(conn, &p, window) {
                error!("Failed to index {}: {:?}", p.display(), e);
            }
        }
    }
    Ok(())
}

fn is_stl(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_ascii_lowercase() == "stl")
        .unwrap_or(false)
}

fn index_file(
    conn: &Connection,
    path: &Path,
    window: &Window,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let size = bytes.len() as i64;
    let hash = hash_bytes(&bytes);
    let path_str = path.to_string_lossy().to_string();

    insert_model(conn, &path_str, size, &hash)?;

    let model_id = conn.query_row(
        "SELECT id FROM models WHERE path = ?1",
        [path_str.clone()],
        |row| row.get(0),
    )?;

    tag_model(conn, model_id, "untagged");

    // Emit event to frontend
    window.emit(
        "model-indexed",
        serde_json::json!({
            "id": model_id,
            "path": path_str,
            "size": size,
            "hash": hash,
            "tags": ["untagged"]
        }),
    )?;

    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
