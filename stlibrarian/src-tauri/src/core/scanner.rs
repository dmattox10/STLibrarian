use crate::core::db::{insert_model, tag_model};
use log::info;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn scan_folder(conn: &Connection, root: &str) {
    info!("Scanning folder: {}", root);
    walk(conn, Path::new(root));
}

fn walk(conn: &Connection, path: &Path) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(conn, &p);
            } else if is_stl(&p) {
                index_file(conn, &p);
            }
        }
    }
}

fn is_stl(path: &Path) -> bool {
    path.extension().map(|e| e == "stl").unwrap_or(false)
}

fn index_file(conn: &Connection, path: &Path) {
    if let Ok(bytes) = fs::read(path) {
        let size = bytes.len() as i64;
        let hash = hash_bytes(&bytes);
        let path_str = path.to_string_lossy().to_string();
        info!("Indexing STL: {}", path_str);
        let _ = insert_model(conn, &path_str, size, &hash);

        // assign default "untagged" tag
        let model_id = conn
            .query_row("SELECT id FROM models WHERE path = ?1", [path_str], |row| {
                row.get(0)
            })
            .unwrap();
        tag_model(conn, model_id, "untagged");
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
