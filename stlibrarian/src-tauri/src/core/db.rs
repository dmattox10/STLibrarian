use crate::core::models::{LibraryQuery, ModelRecord, TagCount};
use log::error;
use rusqlite::{params, Connection, OptionalExtension, Result};
use std::collections::HashMap;
use std::path::Path;

pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = Connection::open(path)?;
    // WAL lets the scanner write while the grid reads. `journal_mode` returns a
    // row, so it cannot go through execute_batch.
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Opens the library at the standard location.
pub fn open_default() -> Result<Connection> {
    open_db(&crate::core::paths::db_path())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = match conn.prepare(&format!("PRAGMA table_info({})", table)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let found = stmt.query_map([], |r| r.get::<_, String>(1)).map(|rows| {
        rows.filter_map(std::result::Result::ok)
            .any(|c| c == column)
    });
    found.unwrap_or(false)
}

fn migrate(conn: &Connection) -> Result<()> {
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
        CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY,
            username      TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL DEFAULT 'user',
            created_at    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            token      TEXT PRIMARY KEY,
            user_id    INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        );
        ",
    )?;

    // v1 shipped models(id, path, size, hash) only. Existing libraries must
    // survive the upgrade, so every new column is added in place with a default
    // rather than by recreating the table.
    for (column, decl) in [
        ("name", "TEXT NOT NULL DEFAULT ''"),
        ("triangles", "INTEGER NOT NULL DEFAULT 0"),
        ("dim_x", "REAL NOT NULL DEFAULT 0"),
        ("dim_y", "REAL NOT NULL DEFAULT 0"),
        ("dim_z", "REAL NOT NULL DEFAULT 0"),
        ("mtime", "INTEGER NOT NULL DEFAULT 0"),
        ("has_thumb", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !has_column(conn, "models", column) {
            conn.execute_batch(&format!(
                "ALTER TABLE models ADD COLUMN {} {};",
                column, decl
            ))?;
        }
    }

    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_models_hash ON models(hash);
        CREATE INDEX IF NOT EXISTS idx_models_name ON models(name);
        CREATE INDEX IF NOT EXISTS idx_model_tags_model ON model_tags(model_id);
        CREATE INDEX IF NOT EXISTS idx_model_tags_tag ON model_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
        ",
    )?;
    Ok(())
}

/// What `upsert_model` did to the row.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Upsert {
    Added,
    Updated,
    Unchanged,
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_model(
    conn: &Connection,
    path: &str,
    name: &str,
    size: i64,
    hash: &str,
    triangles: i64,
    dims: [f64; 3],
    mtime: i64,
) -> Result<(i64, Upsert)> {
    let existing: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT id, hash, mtime FROM models WHERE path = ?1",
            params![path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    if let Some((id, old_hash, old_mtime)) = existing {
        if old_hash == hash && old_mtime == mtime {
            return Ok((id, Upsert::Unchanged));
        }
        conn.execute(
            "UPDATE models SET name=?2, size=?3, hash=?4, triangles=?5,
                    dim_x=?6, dim_y=?7, dim_z=?8, mtime=?9
             WHERE id=?1",
            params![id, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime],
        )?;
        return Ok((id, Upsert::Updated));
    }

    conn.execute(
        "INSERT INTO models (path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![path, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime],
    )?;
    Ok((conn.last_insert_rowid(), Upsert::Added))
}

pub fn list_models(conn: &Connection, q: &LibraryQuery) -> Result<Vec<ModelRecord>> {
    let mut sql = String::from(
        "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb
         FROM models WHERE 1=1",
    );
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(search) = q.search.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sql.push_str(" AND (name LIKE ?  OR path LIKE ?)");
        let like = format!("%{}%", search);
        binds.push(Box::new(like.clone()));
        binds.push(Box::new(like));
    }

    // Every listed tag must be present, so count the distinct matches and
    // require the full set.
    let tags: Vec<&String> = q.tags.iter().filter(|t| !t.trim().is_empty()).collect();
    if !tags.is_empty() {
        let placeholders = vec!["?"; tags.len()].join(",");
        sql.push_str(&format!(
            " AND id IN (SELECT mt.model_id FROM model_tags mt
                          JOIN tags t ON t.id = mt.tag_id
                          WHERE t.name IN ({})
                          GROUP BY mt.model_id
                          HAVING COUNT(DISTINCT t.name) = ?)",
            placeholders
        ));
        for t in &tags {
            binds.push(Box::new((*t).clone()));
        }
        binds.push(Box::new(tags.len() as i64));
    }

    // Whitelisted, because this goes into the SQL string.
    sql.push_str(match q.sort.as_deref() {
        Some("name") => " ORDER BY name COLLATE NOCASE ASC",
        Some("name_desc") => " ORDER BY name COLLATE NOCASE DESC",
        Some("size") => " ORDER BY size DESC",
        Some("size_asc") => " ORDER BY size ASC",
        Some("triangles") => " ORDER BY triangles DESC",
        Some("tallest") => " ORDER BY dim_z DESC",
        Some("oldest") => " ORDER BY mtime ASC",
        _ => " ORDER BY mtime DESC",
    });

    sql.push_str(" LIMIT ? OFFSET ?");
    binds.push(Box::new(q.limit.unwrap_or(500).clamp(1, 5000)));
    binds.push(Box::new(q.offset.unwrap_or(0).max(0)));

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(bind_refs.as_slice(), |row| {
        Ok(ModelRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            size: row.get(3)?,
            hash: row.get(4)?,
            triangles: row.get(5)?,
            dim_x: row.get(6)?,
            dim_y: row.get(7)?,
            dim_z: row.get(8)?,
            mtime: row.get(9)?,
            has_thumb: row.get::<_, i64>(10)? != 0,
            tags: Vec::new(),
        })
    })?;

    let mut results: Vec<ModelRecord> = Vec::new();
    for r in rows {
        match r {
            Ok(m) => results.push(m),
            Err(e) => error!("Error loading model row: {}", e),
        }
    }

    // One query for every tag on the page. The old code prepared a fresh
    // statement inside the row callback -- a query per model, which is what
    // made a large library crawl.
    attach_tags(conn, &mut results)?;
    Ok(results)
}

fn attach_tags(conn: &Connection, models: &mut [ModelRecord]) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = models.iter().map(|m| m.id.to_string()).collect();
    // Safe to interpolate: these are i64 read back out of the same table.
    let sql = format!(
        "SELECT mt.model_id, t.name
         FROM model_tags mt JOIN tags t ON t.id = mt.tag_id
         WHERE mt.model_id IN ({})
         ORDER BY t.name COLLATE NOCASE",
        ids.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut by_id: HashMap<i64, Vec<String>> = HashMap::new();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows.flatten() {
        by_id.entry(row.0).or_default().push(row.1);
    }
    for m in models.iter_mut() {
        if let Some(tags) = by_id.remove(&m.id) {
            m.tags = tags;
        }
    }
    Ok(())
}

pub fn get_model(conn: &Connection, id: i64) -> Result<Option<ModelRecord>> {
    let found: Option<ModelRecord> = conn
        .query_row(
            "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb
             FROM models WHERE id = ?1",
            params![id],
            |row| {
                Ok(ModelRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    size: row.get(3)?,
                    hash: row.get(4)?,
                    triangles: row.get(5)?,
                    dim_x: row.get(6)?,
                    dim_y: row.get(7)?,
                    dim_z: row.get(8)?,
                    mtime: row.get(9)?,
                    has_thumb: row.get::<_, i64>(10)? != 0,
                    tags: Vec::new(),
                })
            },
        )
        .optional()?;

    match found {
        Some(m) => {
            let mut one = vec![m];
            attach_tags(conn, &mut one)?;
            Ok(one.pop())
        }
        None => Ok(None),
    }
}

pub fn count_models(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
}

pub fn add_tag(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![name])?;
    conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |row| {
        row.get(0)
    })
}

pub fn tag_model(conn: &Connection, model_id: i64, tag: &str) -> Result<()> {
    let tag_id = add_tag(conn, tag)?;
    conn.execute(
        "INSERT OR IGNORE INTO model_tags (model_id, tag_id) VALUES (?1, ?2)",
        params![model_id, tag_id],
    )?;
    Ok(())
}

pub fn set_tags(conn: &Connection, model_id: i64, tags: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM model_tags WHERE model_id = ?1",
        params![model_id],
    )?;
    for tag in tags {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            tag_model(conn, model_id, trimmed)?;
        }
    }
    Ok(())
}

/// Tags with how many models carry each -- the sidebar's whole data source.
pub fn list_tags(conn: &Connection) -> Result<Vec<TagCount>> {
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(mt.model_id)
         FROM tags t LEFT JOIN model_tags mt ON mt.tag_id = t.id
         GROUP BY t.id
         HAVING COUNT(mt.model_id) > 0
         ORDER BY COUNT(mt.model_id) DESC, t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TagCount {
            name: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn delete_model(conn: &Connection, model_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM model_tags WHERE model_id = ?1",
        params![model_id],
    )?;
    conn.execute("DELETE FROM models WHERE id = ?1", params![model_id])?;
    Ok(())
}

pub fn set_has_thumb(conn: &Connection, hash: &str, has: bool) -> Result<()> {
    conn.execute(
        "UPDATE models SET has_thumb = ?2 WHERE hash = ?1",
        params![hash, if has { 1 } else { 0 }],
    )?;
    Ok(())
}

/// Rows whose file is gone from disk. Returned rather than deleted, because
/// an unplugged external drive must not empty somebody's library.
pub fn find_missing(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, path FROM models")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows
        .flatten()
        .filter(|(_, p)| !Path::new(p).exists())
        .collect())
}

pub fn library_stats(conn: &Connection) -> Result<crate::core::models::LibraryStats> {
    let (models, total_bytes, without_thumb): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0), SUM(CASE WHEN has_thumb = 0 THEN 1 ELSE 0 END)
         FROM models",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0))),
    )?;
    let tags: i64 = conn.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0))?;
    Ok(crate::core::models::LibraryStats {
        models,
        total_bytes,
        tags,
        missing: 0,
        without_thumb,
    })
}

/// Every hash currently in the library -- the input to a thumbnail sweep.
pub fn all_hashes(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT hash FROM models")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}
