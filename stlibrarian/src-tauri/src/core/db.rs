use crate::core::models::ModelRecord;
use log::{error, info};
use rusqlite::{params, Connection, Result};

pub fn open_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS models (
            id    INTEGER PRIMARY KEY,
            path  TEXT UNIQUE,
            size  INTEGER,
            hash  TEXT
        );
        CREATE TABLE IF NOT EXISTS tags (
            id   INTEGER PRIMARY KEY,
            name TEXT UNIQUE
        );
        CREATE TABLE IF NOT EXISTS model_tags (
            model_id INTEGER,
            tag_id   INTEGER,
            UNIQUE(model_id, tag_id)
        );
        ",
    )?;
    Ok(conn)
}

pub fn insert_model(conn: &Connection, path: &str, size: i64, hash: &str) -> Result<()> {
    info!("Inserting model: {}", path);
    conn.execute(
        "INSERT OR IGNORE INTO models (path, size, hash) VALUES (?1, ?2, ?3)",
        params![path, size, hash],
    )?;
    Ok(())
}

pub fn list_models(conn: &Connection) -> Result<Vec<ModelRecord>> {
    let mut stmt = conn.prepare("SELECT id, path, size, hash FROM models")?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let mut tag_stmt = conn.prepare(
            "SELECT t.name
             FROM tags t
             JOIN model_tags mt ON mt.tag_id = t.id
             WHERE mt.model_id = ?1",
        )?;
        let tags_iter = tag_stmt.query_map([id], |r| r.get(0))?;
        let tags: Vec<String> = tags_iter.filter_map(Result::ok).collect();

        Ok(ModelRecord {
            id,
            path: row.get(1)?,
            size: row.get(2)?,
            hash: row.get(3)?,
            tags,
        })
    })?;

    let mut results = Vec::new();
    for r in rows {
        match r {
            Ok(model) => results.push(model),
            Err(e) => error!("Error loading model row: {}", e),
        }
    }
    Ok(results)
}

// Add a new tag if it doesn't exist
pub fn add_tag(conn: &Connection, name: &str) -> i64 {
    conn.execute(
        "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
        params![name],
    )
    .ok();
    conn.query_row(
        "SELECT id FROM tags WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )
    .unwrap()
}

// Assign a tag to a model
pub fn tag_model(conn: &Connection, model_id: i64, tag: &str) {
    let tag_id = add_tag(conn, tag);
    if let Err(e) = conn.execute(
        "INSERT OR IGNORE INTO model_tags (model_id, tag_id) VALUES (?1, ?2)",
        params![model_id, tag_id],
    ) {
        error!("Failed to tag model {} with '{}': {}", model_id, tag, e);
    }
}

// Bulk set tags for a model
pub fn set_tags(conn: &Connection, model_id: i64, tags: &[String]) {
    if let Err(e) = conn.execute(
        "DELETE FROM model_tags WHERE model_id = ?1",
        params![model_id],
    ) {
        error!("Failed to clear tags for model {}: {}", model_id, e);
    }
    for tag in tags {
        tag_model(conn, model_id, tag);
    }
}

// Delete model and associated tags
pub fn delete_model(conn: &Connection, model_id: i64) {
    if let Err(e) = conn.execute(
        "DELETE FROM model_tags WHERE model_id = ?1",
        params![model_id],
    ) {
        error!("Failed to delete model_tags for {}: {}", model_id, e);
    }
    if let Err(e) = conn.execute("DELETE FROM models WHERE id = ?1", params![model_id]) {
        error!("Failed to delete model {}: {}", model_id, e);
    }
}
