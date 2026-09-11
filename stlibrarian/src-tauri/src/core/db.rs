use crate::core::models::{Collection, LibraryQuery, ModelRecord, TagCount};
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
        CREATE TABLE IF NOT EXISTS collections (
            id         INTEGER PRIMARY KEY,
            name       TEXT UNIQUE NOT NULL COLLATE NOCASE,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS collection_models (
            collection_id INTEGER NOT NULL,
            model_id      INTEGER NOT NULL,
            added_at      INTEGER NOT NULL,
            UNIQUE(collection_id, model_id)
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
        ("volume", "REAL NOT NULL DEFAULT 0"),
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
        CREATE INDEX IF NOT EXISTS idx_coll_models_coll ON collection_models(collection_id);
        CREATE INDEX IF NOT EXISTS idx_coll_models_model ON collection_models(model_id);
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
    volume: f64,
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
                    dim_x=?6, dim_y=?7, dim_z=?8, mtime=?9, volume=?10
             WHERE id=?1",
            params![id, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime, volume],
        )?;
        return Ok((id, Upsert::Updated));
    }

    conn.execute(
        "INSERT INTO models (path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, volume)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![path, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime, volume],
    )?;
    Ok((conn.last_insert_rowid(), Upsert::Added))
}

pub fn list_models(conn: &Connection, q: &LibraryQuery) -> Result<Vec<ModelRecord>> {
    let mut sql = String::from(
        "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb, volume
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

    if let Some(collection_id) = q.collection {
        sql.push_str(
            " AND id IN (SELECT model_id FROM collection_models WHERE collection_id = ?)",
        );
        binds.push(Box::new(collection_id));
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
            volume: row.get(11)?,
            tags: Vec::new(),
            collections: Vec::new(),
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
    attach_collections(conn, &mut results)?;
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
            "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb, volume
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
                    volume: row.get(11)?,
                    tags: Vec::new(),
                    collections: Vec::new(),
                })
            },
        )
        .optional()?;

    match found {
        Some(m) => {
            let mut one = vec![m];
            attach_tags(conn, &mut one)?;
            attach_collections(conn, &mut one)?;
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
    // Otherwise a deleted model leaves a row pointing at nothing, and every
    // collection count that included it stays permanently wrong.
    conn.execute(
        "DELETE FROM collection_models WHERE model_id = ?1",
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

// ----------------------------------------------------------- collections ----

fn attach_collections(conn: &Connection, models: &mut [ModelRecord]) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = models.iter().map(|m| m.id.to_string()).collect();
    // Safe to interpolate: these are i64 read back out of the same table.
    let sql = format!(
        "SELECT cm.model_id, c.name
         FROM collection_models cm JOIN collections c ON c.id = cm.collection_id
         WHERE cm.model_id IN ({})
         ORDER BY c.name COLLATE NOCASE",
        ids.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut by_id: HashMap<i64, Vec<String>> = HashMap::new();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows.flatten() {
        by_id.entry(row.0).or_default().push(row.1);
    }
    for m in models.iter_mut() {
        if let Some(names) = by_id.remove(&m.id) {
            m.collections = names;
        }
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn create_collection(conn: &Connection, name: &str) -> std::result::Result<Collection, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A collection needs a name".into());
    }
    if name.len() > 120 {
        return Err("That name is too long".into());
    }
    conn.execute(
        "INSERT INTO collections (name, created_at) VALUES (?1, ?2)",
        params![name, now()],
    )
    .map_err(|e| {
        // The name column is UNIQUE COLLATE NOCASE, so "Dragons" and "dragons"
        // collide. Say so plainly instead of surfacing a constraint error.
        if e.to_string().contains("UNIQUE") {
            format!("A collection called \"{}\" already exists", name)
        } else {
            format!("Could not create collection: {}", e)
        }
    })?;
    Ok(Collection {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        count: 0,
    })
}

pub fn rename_collection(
    conn: &Connection,
    id: i64,
    name: &str,
) -> std::result::Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A collection needs a name".into());
    }
    conn.execute(
        "UPDATE collections SET name = ?2 WHERE id = ?1",
        params![id, name],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("A collection called \"{}\" already exists", name)
        } else {
            format!("Could not rename collection: {}", e)
        }
    })?;
    Ok(())
}

/// Removes the collection, not the models in it.
pub fn delete_collection(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM collection_models WHERE collection_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
    Ok(())
}

/// Every collection with its size. Unlike tags, empty collections are kept --
/// you make one before you fill it.
pub fn list_collections(conn: &Connection) -> Result<Vec<Collection>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, COUNT(cm.model_id)
         FROM collections c
         LEFT JOIN collection_models cm ON cm.collection_id = c.id
         GROUP BY c.id
         ORDER BY c.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Collection {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn add_to_collection(conn: &Connection, collection_id: i64, model_ids: &[i64]) -> Result<usize> {
    if model_ids.is_empty() {
        return Ok(0);
    }
    // One transaction for the whole selection: tagging forty models is one
    // action to the user and should be one to the database.
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO collection_models (collection_id, model_id, added_at)
             VALUES (?1, ?2, ?3)",
        )?;
        let stamp = now();
        for id in model_ids {
            changed += stmt.execute(params![collection_id, id, stamp])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

pub fn remove_from_collection(
    conn: &Connection,
    collection_id: i64,
    model_ids: &[i64],
) -> Result<usize> {
    if model_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt =
            tx.prepare("DELETE FROM collection_models WHERE collection_id = ?1 AND model_id = ?2")?;
        for id in model_ids {
            changed += stmt.execute(params![collection_id, id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

// ------------------------------------------------------------- bulk tags ----

pub fn add_tag_to_models(conn: &Connection, model_ids: &[i64], tag: &str) -> Result<usize> {
    let tag = tag.trim();
    if tag.is_empty() || model_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let tag_id = add_tag(&tx, tag)?;
    let mut changed = 0;
    {
        let mut stmt =
            tx.prepare("INSERT OR IGNORE INTO model_tags (model_id, tag_id) VALUES (?1, ?2)")?;
        for id in model_ids {
            changed += stmt.execute(params![id, tag_id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

pub fn remove_tag_from_models(conn: &Connection, model_ids: &[i64], tag: &str) -> Result<usize> {
    let tag = tag.trim();
    if tag.is_empty() || model_ids.is_empty() {
        return Ok(0);
    }
    let tag_id: Option<i64> = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| {
            r.get(0)
        })
        .optional()?;
    let tag_id = match tag_id {
        Some(id) => id,
        None => return Ok(0),
    };

    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare("DELETE FROM model_tags WHERE model_id = ?1 AND tag_id = ?2")?;
        for id in model_ids {
            changed += stmt.execute(params![id, tag_id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A real on-disk database that removes itself, WAL sidecars included.
    struct TempDb {
        conn: Connection,
        path: std::path::PathBuf,
    }

    impl std::ops::Deref for TempDb {
        type Target = Connection;
        fn deref(&self) -> &Connection {
            &self.conn
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.path.display(), suffix));
            }
        }
    }

    fn db() -> TempDb {
        let path = std::env::temp_dir().join(format!(
            "stlib_db_{}_{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let conn = open_db(&path).unwrap();
        TempDb { conn, path }
    }

    fn model(conn: &Connection, name: &str) -> i64 {
        upsert_model(
            conn,
            &format!("/models/{}.stl", name),
            name,
            1024,
            &format!("{:x>64}", name),
            42,
            [10.0, 20.0, 30.0],
            6000.0,
            1000,
        )
        .unwrap()
        .0
    }

    fn names(conn: &Connection, q: &LibraryQuery) -> Vec<String> {
        let mut out: Vec<String> = list_models(conn, q).unwrap().into_iter().map(|m| m.name).collect();
        out.sort();
        out
    }

    #[test]
    fn collections_are_created_listed_and_counted() {
        let conn = db();
        let dragons = create_collection(&conn, "Dragons").unwrap();
        create_collection(&conn, "Terrain").unwrap();

        // An empty collection still appears -- you make one before you fill it.
        let listed = list_collections(&conn).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Dragons");
        assert_eq!(listed[0].count, 0);

        let a = model(&conn, "smaug");
        let b = model(&conn, "wyvern");
        assert_eq!(add_to_collection(&conn, dragons.id, &[a, b]).unwrap(), 2);

        let listed = list_collections(&conn).unwrap();
        assert_eq!(listed[0].count, 2);
        assert_eq!(listed[1].count, 0);
    }

    #[test]
    fn adding_the_same_model_twice_does_not_duplicate_it() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");

        assert_eq!(add_to_collection(&conn, c.id, &[a]).unwrap(), 1);
        // Second add reports nothing changed rather than creating a second row.
        assert_eq!(add_to_collection(&conn, c.id, &[a]).unwrap(), 0);
        assert_eq!(list_collections(&conn).unwrap()[0].count, 1);
    }

    #[test]
    fn duplicate_names_are_refused_regardless_of_case() {
        let conn = db();
        create_collection(&conn, "Dragons").unwrap();
        // COLLATE NOCASE: "dragons" and "Dragons" are the same collection, and
        // silently making two would be worse than refusing.
        let err = create_collection(&conn, "dragons").unwrap_err();
        assert!(err.contains("already exists"), "got: {}", err);
        assert!(create_collection(&conn, "  ").is_err());
        assert_eq!(list_collections(&conn).unwrap().len(), 1);
    }

    #[test]
    fn filtering_by_collection_narrows_the_library() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        model(&conn, "bracket");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        assert_eq!(names(&conn, &LibraryQuery::default()).len(), 2);
        assert_eq!(
            names(
                &conn,
                &LibraryQuery {
                    collection: Some(c.id),
                    ..Default::default()
                }
            ),
            vec!["smaug"]
        );
    }

    #[test]
    fn collection_and_tag_and_search_combine_rather_than_replace() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let smaug = model(&conn, "smaug");
        let wyvern = model(&conn, "wyvern");
        add_to_collection(&conn, c.id, &[smaug, wyvern]).unwrap();
        tag_model(&conn, smaug, "painted").unwrap();

        // Searching inside a collection must not fall back to the whole library.
        assert_eq!(
            names(
                &conn,
                &LibraryQuery {
                    collection: Some(c.id),
                    tags: vec!["painted".into()],
                    ..Default::default()
                }
            ),
            vec!["smaug"]
        );
        assert_eq!(
            names(
                &conn,
                &LibraryQuery {
                    collection: Some(c.id),
                    search: Some("wyv".into()),
                    ..Default::default()
                }
            ),
            vec!["wyvern"]
        );
    }

    #[test]
    fn membership_is_reported_on_the_model() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        let found = get_model(&conn, a).unwrap().unwrap();
        assert_eq!(found.collections, vec!["Dragons"]);
        let listed = list_models(&conn, &LibraryQuery::default()).unwrap();
        assert_eq!(listed[0].collections, vec!["Dragons"]);
    }

    #[test]
    fn removing_from_a_collection_leaves_the_model_alone() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        assert_eq!(remove_from_collection(&conn, c.id, &[a]).unwrap(), 1);
        assert_eq!(list_collections(&conn).unwrap()[0].count, 0);
        // The model itself survives.
        assert!(get_model(&conn, a).unwrap().is_some());
    }

    #[test]
    fn deleting_a_collection_does_not_delete_its_models() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        delete_collection(&conn, c.id).unwrap();
        assert!(list_collections(&conn).unwrap().is_empty());
        assert!(get_model(&conn, a).unwrap().is_some());
        assert_eq!(get_model(&conn, a).unwrap().unwrap().collections.len(), 0);
    }

    /// The reason delete_model touches collection_models: otherwise every
    /// collection that held the model reports a count that can never be right.
    #[test]
    fn deleting_a_model_removes_it_from_collections() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        let b = model(&conn, "wyvern");
        add_to_collection(&conn, c.id, &[a, b]).unwrap();

        delete_model(&conn, a).unwrap();
        assert_eq!(list_collections(&conn).unwrap()[0].count, 1);
    }

    #[test]
    fn renaming_a_collection_keeps_its_members() {
        let conn = db();
        let c = create_collection(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        rename_collection(&conn, c.id, "Big Lizards").unwrap();
        let listed = list_collections(&conn).unwrap();
        assert_eq!(listed[0].name, "Big Lizards");
        assert_eq!(listed[0].count, 1);
        assert_eq!(get_model(&conn, a).unwrap().unwrap().collections, vec!["Big Lizards"]);
    }

    // ------------------------------------------------------------ bulk ----

    #[test]
    fn one_tag_lands_on_every_selected_model() {
        let conn = db();
        let ids: Vec<i64> = ["a", "b", "c"].iter().map(|n| model(&conn, n)).collect();

        assert_eq!(add_tag_to_models(&conn, &ids, "minis").unwrap(), 3);
        assert_eq!(
            names(
                &conn,
                &LibraryQuery {
                    tags: vec!["minis".into()],
                    ..Default::default()
                }
            ),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn bulk_tagging_does_not_disturb_tags_already_there() {
        let conn = db();
        let a = model(&conn, "a");
        let b = model(&conn, "b");
        tag_model(&conn, a, "painted").unwrap();

        add_tag_to_models(&conn, &[a, b], "minis").unwrap();
        let mut tags = get_model(&conn, a).unwrap().unwrap().tags;
        tags.sort();
        // set_tags replaces; bulk add must not.
        assert_eq!(tags, vec!["minis", "painted"]);
    }

    #[test]
    fn re_applying_a_tag_reports_nothing_changed() {
        let conn = db();
        let ids: Vec<i64> = ["a", "b"].iter().map(|n| model(&conn, n)).collect();
        assert_eq!(add_tag_to_models(&conn, &ids, "minis").unwrap(), 2);
        assert_eq!(add_tag_to_models(&conn, &ids, "minis").unwrap(), 0);
    }

    #[test]
    fn bulk_remove_takes_the_tag_off_only_the_selection() {
        let conn = db();
        let a = model(&conn, "a");
        let b = model(&conn, "b");
        add_tag_to_models(&conn, &[a, b], "minis").unwrap();

        assert_eq!(remove_tag_from_models(&conn, &[a], "minis").unwrap(), 1);
        assert!(get_model(&conn, a).unwrap().unwrap().tags.is_empty());
        assert_eq!(get_model(&conn, b).unwrap().unwrap().tags, vec!["minis"]);
    }

    #[test]
    fn bulk_operations_on_nothing_are_harmless() {
        let conn = db();
        let a = model(&conn, "a");
        assert_eq!(add_tag_to_models(&conn, &[], "minis").unwrap(), 0);
        assert_eq!(add_tag_to_models(&conn, &[a], "   ").unwrap(), 0);
        assert_eq!(remove_tag_from_models(&conn, &[a], "never-existed").unwrap(), 0);
        assert_eq!(add_to_collection(&conn, 999, &[]).unwrap(), 0);
    }
}

/// The file behind a content hash. Thumbnails are addressed by hash, so this
/// is how a preview request finds something to render.
pub fn path_for_hash(conn: &Connection, hash: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT path FROM models WHERE hash = ?1 LIMIT 1",
        params![hash],
        |r| r.get(0),
    )
    .optional()
}
