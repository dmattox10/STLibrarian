use crate::core::models::{
    Collection, DuplicateGroup, LibraryQuery, ModelRecord, Package, PackageSuggestion, PrintRecord,
    Slice, TagCount,
};
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
        CREATE TABLE IF NOT EXISTS feedback (
            id           INTEGER PRIMARY KEY,
            kind         TEXT NOT NULL,
            rating       INTEGER,
            body         TEXT NOT NULL,
            author       TEXT NOT NULL DEFAULT '',
            printer      TEXT NOT NULL DEFAULT '',
            app_version  TEXT NOT NULL DEFAULT '',
            platform     TEXT NOT NULL DEFAULT '',
            may_publish  INTEGER NOT NULL DEFAULT 0,
            -- Nothing reaches the public site until this says approved.
            status       TEXT NOT NULL DEFAULT 'pending',
            submitted_at INTEGER NOT NULL,
            moderated_at INTEGER,
            contact      TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS slices (
            id              INTEGER PRIMARY KEY,
            path            TEXT UNIQUE NOT NULL,
            name            TEXT NOT NULL,
            dir             TEXT NOT NULL,
            parent_dir      TEXT NOT NULL,
            format          TEXT NOT NULL,
            size            INTEGER NOT NULL,
            mtime           INTEGER NOT NULL,
            layer_height    REAL,
            print_time_secs INTEGER,
            material_grams  REAL,
            printer         TEXT NOT NULL DEFAULT '',
            model_id        INTEGER,
            package_id      INTEGER
        );
        CREATE TABLE IF NOT EXISTS prints (
            id         INTEGER PRIMARY KEY,
            model_id   INTEGER NOT NULL,
            printed_at INTEGER NOT NULL,
            printer    TEXT NOT NULL DEFAULT '',
            material   TEXT NOT NULL DEFAULT '',
            settings   TEXT NOT NULL DEFAULT '',
            outcome    TEXT NOT NULL,
            notes      TEXT NOT NULL DEFAULT '',
            photo      TEXT,
            -- Which sliced file was actually run, when one is known. This is
            -- what turns a log entry into a repeatable job.
            slice_id   INTEGER,
            -- Who ran it. Null when the person has since been removed: the
            -- record is a shop's account of what came off which printer and
            -- outlives anyone's account.
            user_id    INTEGER
        );
        CREATE TABLE IF NOT EXISTS images (
            id    INTEGER PRIMARY KEY,
            path  TEXT UNIQUE NOT NULL,
            name  TEXT NOT NULL,
            dir   TEXT NOT NULL,
            size  INTEGER NOT NULL,
            mtime INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS packages (
            id         INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            root       TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS collections (
            id         INTEGER PRIMARY KEY,
            name       TEXT UNIQUE NOT NULL COLLATE NOCASE,
            created_at INTEGER NOT NULL,
            -- Null means shared with everyone. A shop wants both: the
            -- library's own shelves, and somebody's personal queue.
            owner_id   INTEGER
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
        // Provenance is guessed from the path at scan time and freely
        // editable afterwards. A guess you can correct beats a blank field
        // that never gets filled in.
        ("creator", "TEXT NOT NULL DEFAULT ''"),
        ("source", "TEXT NOT NULL DEFAULT ''"),
        // Nullable on purpose: a file belongs to a package only if someone
        // decided it does. Scanning never assumes a folder is one model.
        ("package_id", "INTEGER"),
        // Denormalised so image matching is a join rather than string work in
        // Rust over every row.
        ("dir", "TEXT NOT NULL DEFAULT ''"),
        ("parent_dir", "TEXT NOT NULL DEFAULT ''"),
        // A creator's own render, when the folder shipped one.
        ("image_id", "INTEGER"),
        ("variant", "TEXT NOT NULL DEFAULT ''"),
        ("watertight", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !has_column(conn, "models", column) {
            conn.execute_batch(&format!(
                "ALTER TABLE models ADD COLUMN {} {};",
                column, decl
            ))?;
        }
    }

    // These tables predate the columns, so existing libraries need them added
    // in place exactly as the models table did.
    for (table, column, decl) in [
        ("prints", "user_id", "INTEGER"),
        ("collections", "owner_id", "INTEGER"),
    ] {
        if !has_column(conn, table, column) {
            conn.execute_batch(&format!(
                "ALTER TABLE {} ADD COLUMN {} {};",
                table, column, decl
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
        CREATE INDEX IF NOT EXISTS idx_models_package ON models(package_id);
        CREATE INDEX IF NOT EXISTS idx_models_creator ON models(creator);
        CREATE INDEX IF NOT EXISTS idx_models_dir ON models(dir);
        CREATE INDEX IF NOT EXISTS idx_images_dir ON images(dir);
        CREATE INDEX IF NOT EXISTS idx_prints_model ON prints(model_id);
        CREATE INDEX IF NOT EXISTS idx_slices_model ON slices(model_id);
        CREATE INDEX IF NOT EXISTS idx_slices_package ON slices(package_id);
        CREATE INDEX IF NOT EXISTS idx_slices_dir ON slices(dir);
        CREATE INDEX IF NOT EXISTS idx_feedback_status ON feedback(status);
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
    provenance: (&str, &str),
    variant: &str,
    watertight: bool,
) -> Result<(i64, Upsert)> {
    let dir = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent_dir = Path::new(&dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
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
            // Provenance is not overwritten on rescan: it may have been
            // corrected by hand since, and a guess must not clobber that.
            "UPDATE models SET name=?2, size=?3, hash=?4, triangles=?5,
                    dim_x=?6, dim_y=?7, dim_z=?8, mtime=?9, volume=?10,
                    variant=?11, watertight=?12
             WHERE id=?1",
            params![
                id, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime, volume,
                variant, watertight
            ],
        )?;
        return Ok((id, Upsert::Updated));
    }

    conn.execute(
        "INSERT INTO models (path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime,
                             volume, creator, source, dir, parent_dir, variant, watertight)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            path, name, size, hash, triangles, dims[0], dims[1], dims[2], mtime, volume,
            provenance.0, provenance.1, dir, parent_dir, variant, watertight
        ],
    )?;
    Ok((conn.last_insert_rowid(), Upsert::Added))
}

pub fn list_models(conn: &Connection, q: &LibraryQuery) -> Result<Vec<ModelRecord>> {
    let mut sql = String::from(
        "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb,
                volume, creator, source, package_id,
                (SELECT path FROM images WHERE id = models.image_id),
                (SELECT COUNT(*) FROM prints WHERE prints.model_id = models.id),
                (SELECT MAX(printed_at) FROM prints WHERE prints.model_id = models.id),
                variant, watertight,
                (SELECT COUNT(*) FROM slices WHERE slices.model_id = models.id
                    OR (models.package_id IS NOT NULL
                        AND slices.package_id = models.package_id))
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

    if let Some(variant) = q.variant.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
        sql.push_str(" AND variant = ?");
        binds.push(Box::new(variant.to_string()));
    }

    if let Some(printed) = q.printed {
        if printed {
            sql.push_str(" AND EXISTS (SELECT 1 FROM prints WHERE prints.model_id = models.id)");
        } else {
            sql.push_str(" AND NOT EXISTS (SELECT 1 FROM prints WHERE prints.model_id = models.id)");
        }
    }

    if let Some(creator) = q.creator.as_ref().map(|c| c.trim()).filter(|c| !c.is_empty()) {
        sql.push_str(" AND creator = ?");
        binds.push(Box::new(creator.to_string()));
    }

    if let Some(package_id) = q.package {
        sql.push_str(" AND package_id = ?");
        binds.push(Box::new(package_id));
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
            creator: row.get(12)?,
            source: row.get(13)?,
            package_id: row.get(14)?,
            render: row.get(15)?,
            print_count: row.get(16)?,
            last_printed: row.get(17)?,
            variant: row.get(18)?,
            watertight: row.get::<_, i64>(19)? != 0,
            slice_count: row.get(20)?,
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
            "SELECT id, path, name, size, hash, triangles, dim_x, dim_y, dim_z, mtime, has_thumb,
                    volume, creator, source, package_id,
                    (SELECT path FROM images WHERE id = models.image_id),
                    (SELECT COUNT(*) FROM prints WHERE prints.model_id = models.id),
                    (SELECT MAX(printed_at) FROM prints WHERE prints.model_id = models.id),
                    variant, watertight,
                    (SELECT COUNT(*) FROM slices WHERE slices.model_id = models.id
                        OR (models.package_id IS NOT NULL
                            AND slices.package_id = models.package_id))
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
                    creator: row.get(12)?,
                    source: row.get(13)?,
                    package_id: row.get(14)?,
                    render: row.get(15)?,
                    print_count: row.get(16)?,
                    last_printed: row.get(17)?,
                    variant: row.get(18)?,
                    watertight: row.get::<_, i64>(19)? != 0,
            slice_count: row.get(20)?,
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
    conn.execute("DELETE FROM prints WHERE model_id = ?1", params![model_id])?;
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
    let printed: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT model_id) FROM prints",
        [],
        |r| r.get(0),
    )?;
    Ok(crate::core::models::LibraryStats {
        models,
        total_bytes,
        tags,
        missing: 0,
        without_thumb,
        printed,
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

pub fn create_collection(
    conn: &Connection,
    name: &str,
    owner_id: Option<i64>,
) -> std::result::Result<Collection, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A collection needs a name".into());
    }
    if name.len() > 120 {
        return Err("That name is too long".into());
    }
    conn.execute(
        "INSERT INTO collections (name, created_at, owner_id) VALUES (?1, ?2, ?3)",
        params![name, now(), owner_id],
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
        owner_id,
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
pub fn list_collections(conn: &Connection, viewer_id: Option<i64>) -> Result<Vec<Collection>> {
    // Shared shelves plus this person's own. Someone else's private queue is
    // not hidden for secrecy -- it is simply not theirs to see in a list.
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, COUNT(cm.model_id), c.owner_id
         FROM collections c
         LEFT JOIN collection_models cm ON cm.collection_id = c.id
         WHERE c.owner_id IS NULL OR c.owner_id = ?1
         GROUP BY c.id
         ORDER BY c.owner_id IS NOT NULL, c.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![viewer_id], |r| {
        Ok(Collection {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
            owner_id: r.get(3)?,
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
            ("", ""),
            "",
            true,
        )
        .unwrap()
        .0
    }

    fn names(conn: &Connection, q: &LibraryQuery) -> Vec<String> {
        let mut out: Vec<String> = list_models(conn, q).unwrap().into_iter().map(|m| m.name).collect();
        out.sort();
        out
    }

    fn create_collection_shared(
        conn: &Connection,
        name: &str,
    ) -> std::result::Result<Collection, String> {
        create_collection(conn, name, None)
    }

    fn model_at(conn: &Connection, path: &str, size: i64, hash: &str) -> i64 {
        let name = std::path::Path::new(path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        upsert_model(
            conn, path, &name, size, hash, 12, [1.0, 2.0, 3.0], 100.0, 1000, ("", ""), "", true,
        )
        .unwrap()
        .0
    }

    // ------------------------------------------------------- packages ----

    /// Four parts under a variant folder are one model; the lone file is not.
    #[test]
    fn suggestions_group_parts_and_leave_single_files_alone() {
        let conn = db();
        model_at(&conn, "/lib/Creator/Queen Bee.stl", 10, "a");
        for part in ["Body", "ARM", "LWing", "RWing"] {
            model_at(
                &conn,
                &format!("/lib/Gods/Throk_130mm/Throk_130mm_Unsupported/Throk_{}.stl", part),
                10,
                part,
            );
        }

        let suggestions = suggest_packages(&conn).unwrap();
        assert_eq!(suggestions.len(), 1, "got {:?}", suggestions);
        assert_eq!(suggestions[0].parts, 4);
        // Unsupported/ collapsed away, so the package is named for the model.
        assert_eq!(suggestions[0].name, "Throk 130mm");
    }

    /// Supported and unsupported copies of one model are one model.
    /// A file manager's duplicate folder is still the same model.
    #[test]
    fn a_duplicated_variant_folder_still_names_the_model() {
        let conn = db();
        model_at(&conn, "/lib/T/Throk_130mm_Unsupported 2/a.stl", 10, "a");
        model_at(&conn, "/lib/T/Throk_130mm_Unsupported 2/b.stl", 10, "b");
        let s = suggest_packages(&conn).unwrap();
        assert_eq!(s[0].name, "Throk 130mm", "got {:?}", s[0].name);
    }

    /// But a model genuinely called "Dragon 2" keeps its number.
    #[test]
    fn a_trailing_number_that_is_part_of_the_name_is_kept() {
        let conn = db();
        model_at(&conn, "/lib/Dragon 2/a.stl", 10, "a");
        model_at(&conn, "/lib/Dragon 2/b.stl", 10, "b");
        let s = suggest_packages(&conn).unwrap();
        assert_eq!(s[0].name, "Dragon 2", "got {:?}", s[0].name);
    }

    #[test]
    fn variant_folders_collapse_into_the_same_suggestion() {
        let conn = db();
        model_at(&conn, "/lib/Dragon/Supported/wing.stl", 10, "a");
        model_at(&conn, "/lib/Dragon/Unsupported/wing.stl", 10, "b");
        model_at(&conn, "/lib/Dragon/Presupported/body.stl", 10, "c");

        let s = suggest_packages(&conn).unwrap();
        assert_eq!(s.len(), 1, "got {:?}", s);
        assert_eq!(s[0].parts, 3);
        assert_eq!(s[0].name, "Dragon");
    }

    #[test]
    fn numeric_folders_collapse_too() {
        let conn = db();
        model_at(&conn, "/lib/Ogre/1/a.stl", 10, "a");
        model_at(&conn, "/lib/Ogre/2/b.stl", 10, "b");
        let s = suggest_packages(&conn).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Ogre");
    }

    /// Nothing is grouped without someone saying so, so a scan must leave
    /// every file unpackaged.
    #[test]
    fn scanning_alone_packages_nothing() {
        let conn = db();
        let a = model_at(&conn, "/lib/Thing/Unsupported/a.stl", 10, "a");
        model_at(&conn, "/lib/Thing/Unsupported/b.stl", 10, "b");
        assert!(get_model(&conn, a).unwrap().unwrap().package_id.is_none());
        assert!(list_packages(&conn).unwrap().is_empty());
    }

    #[test]
    fn assigning_and_unassigning_never_touches_the_models() {
        let conn = db();
        let a = model_at(&conn, "/lib/T/Unsupported/a.stl", 10, "a");
        let b = model_at(&conn, "/lib/T/Unsupported/b.stl", 10, "b");
        let pkg = create_package(&conn, "Throk", "/lib/T").unwrap();

        assert_eq!(assign_to_package(&conn, pkg.id, &[a, b]).unwrap(), 2);
        assert_eq!(list_packages(&conn).unwrap()[0].parts, 2);
        assert_eq!(get_model(&conn, a).unwrap().unwrap().package_id, Some(pkg.id));

        // Already-packaged files stop being suggested.
        assert!(suggest_packages(&conn).unwrap().is_empty());

        assert_eq!(unassign_from_package(&conn, &[a]).unwrap(), 1);
        assert_eq!(list_packages(&conn).unwrap()[0].parts, 1);
        assert!(get_model(&conn, a).unwrap().unwrap().package_id.is_none());
    }

    #[test]
    fn deleting_a_package_keeps_its_files() {
        let conn = db();
        let a = model_at(&conn, "/lib/T/a.stl", 10, "a");
        let pkg = create_package(&conn, "Throk", "/lib/T").unwrap();
        assign_to_package(&conn, pkg.id, &[a]).unwrap();

        delete_package(&conn, pkg.id).unwrap();
        assert!(list_packages(&conn).unwrap().is_empty());
        let still = get_model(&conn, a).unwrap().expect("model survives");
        assert!(still.package_id.is_none());
    }

    // ------------------------------------------------------- feedback ----

    fn review(rating: Option<i64>, body: &str, publish: bool) -> crate::core::models::NewFeedback {
        crate::core::models::NewFeedback {
            kind: "review".into(),
            rating,
            body: body.into(),
            author: "Sam".into(),
            printer: "Saturn 4".into(),
            may_publish: publish,
            ..Default::default()
        }
    }

    /// The whole point: a review is not public because it exists, or because
    /// it is glowing. A person has to approve it *and* the author has to have
    /// agreed to it being shown.
    #[test]
    fn a_review_is_invisible_until_approved_and_permitted() {
        let conn = db();
        let id = add_feedback(&conn, &review(Some(5), "Found a model I forgot I owned.", true)).unwrap();
        assert!(public_reviews(&conn).unwrap().is_empty(), "pending is not public");

        moderate_feedback(&conn, id, "approved").unwrap();
        assert_eq!(public_reviews(&conn).unwrap().len(), 1);

        moderate_feedback(&conn, id, "rejected").unwrap();
        assert!(public_reviews(&conn).unwrap().is_empty(), "rejected goes back out of sight");
    }

    #[test]
    fn an_approved_review_the_author_did_not_release_stays_private() {
        let conn = db();
        let id = add_feedback(&conn, &review(Some(5), "Genuinely good, but keep it to yourself.", false)).unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();
        assert!(public_reviews(&conn).unwrap().is_empty());
        // It is still in the queue for the person running the server.
        assert_eq!(list_feedback(&conn, "approved").unwrap().len(), 1);
    }

    #[test]
    fn a_problem_report_is_never_a_review() {
        let conn = db();
        let mut report = review(None, "Scanning crashed on a 4 GB file.", true);
        report.kind = "problem".into();
        let id = add_feedback(&conn, &report).unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();

        assert!(public_reviews(&conn).unwrap().is_empty(), "a bug report is not a testimonial");
        assert_eq!(list_feedback(&conn, "all").unwrap()[0].kind, "problem");
    }

    #[test]
    fn an_idea_keeps_its_own_bucket_and_is_never_a_review() {
        let conn = db();
        let mut wish = review(Some(5), "It should talk to my slicer.", true);
        wish.kind = "idea".into();
        let id = add_feedback(&conn, &wish).unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();

        // Kept apart from problem reports, because a wish and a fault are not
        // the same queue -- but still not a testimonial, however it is worded.
        assert_eq!(list_feedback(&conn, "all").unwrap()[0].kind, "idea");
        assert!(public_reviews(&conn).unwrap().is_empty());
    }

    #[test]
    fn an_unrecognised_kind_is_filed_rather_than_refused() {
        let conn = db();
        let mut odd = review(None, "Sent by something that speaks its own dialect.", true);
        odd.kind = "compliment".into();
        let id = add_feedback(&conn, &odd).unwrap();
        assert_eq!(list_feedback(&conn, "all").unwrap()[0].kind, "problem");
        moderate_feedback(&conn, id, "approved").unwrap();
        assert!(public_reviews(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_review_without_a_usable_rating_is_refused() {
        let conn = db();
        assert!(add_feedback(&conn, &review(None, "Some words here.", true)).is_err());
        assert!(add_feedback(&conn, &review(Some(0), "Some words here.", true)).is_err());
        assert!(add_feedback(&conn, &review(Some(9), "Some words here.", true)).is_err());
        assert!(list_feedback(&conn, "all").unwrap().is_empty());
    }

    #[test]
    fn an_empty_submission_is_refused() {
        let conn = db();
        assert!(add_feedback(&conn, &review(Some(5), "", true)).is_err());
        assert!(add_feedback(&conn, &review(Some(5), "   \n  ", true)).is_err());
    }

    /// It arrives over the network from a stranger, so it is cleaned on the
    /// way in rather than trusted on the way out.
    #[test]
    fn control_characters_are_stripped_but_paragraphs_survive() {
        let conn = db();
        let id = add_feedback(
            &conn,
            &review(Some(4), "First line.\n\nSecond line.\u{0}\u{7}\u{1b}[31m", true),
        )
        .unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();
        let body = &public_reviews(&conn).unwrap()[0].body;

        assert!(body.contains("First line.\n\nSecond line."), "got {:?}", body);
        assert!(!body.contains('\u{0}'));
        assert!(!body.contains('\u{1b}'), "no escape sequences reach a reader");
    }

    #[test]
    fn a_very_long_submission_is_capped_rather_than_refused() {
        let conn = db();
        let huge = "word ".repeat(4000);
        let id = add_feedback(&conn, &review(Some(5), &huge, true)).unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();
        assert!(public_reviews(&conn).unwrap()[0].body.len() <= 4000);
    }

    #[test]
    fn names_and_printers_cannot_run_away_either() {
        let conn = db();
        let mut long = review(Some(5), "Fine tool.", true);
        long.author = "A".repeat(500);
        long.printer = "B".repeat(500);
        let id = add_feedback(&conn, &long).unwrap();
        moderate_feedback(&conn, id, "approved").unwrap();
        let r = &public_reviews(&conn).unwrap()[0];
        assert!(r.author.len() <= 60 && r.printer.len() <= 60);
    }

    #[test]
    fn the_queue_counts_what_is_waiting() {
        let conn = db();
        let a = add_feedback(&conn, &review(Some(5), "One.", true)).unwrap();
        add_feedback(&conn, &review(Some(3), "Two.", true)).unwrap();
        assert_eq!(pending_feedback_count(&conn), 2);

        moderate_feedback(&conn, a, "approved").unwrap();
        assert_eq!(pending_feedback_count(&conn), 1);
        assert_eq!(list_feedback(&conn, "pending").unwrap().len(), 1);
    }

    #[test]
    fn an_invented_moderation_status_is_refused() {
        let conn = db();
        let id = add_feedback(&conn, &review(Some(5), "Fine tool.", true)).unwrap();
        assert!(moderate_feedback(&conn, id, "published").is_err());
        assert!(moderate_feedback(&conn, 9999, "approved").is_err());
    }

    // ----------------------------------------------------- multi-user ----

    fn a_user(conn: &Connection, name: &str, role: &str) -> i64 {
        crate::core::auth::create_user(conn, name, "longenoughpassword", role)
            .unwrap()
            .id
    }

    #[test]
    fn a_print_records_who_ran_it() {
        let conn = db();
        let sam = a_user(&conn, "sam", "user");
        let id = model(&conn, "dragon");
        add_print(&conn, id, &a_print("success"), Some(sam)).unwrap();

        assert_eq!(list_prints(&conn, id).unwrap()[0].by, "sam");
    }

    /// A shop's account of what came off which printer outlives anyone's
    /// account, so removing a person must not remove their history.
    #[test]
    fn removing_a_person_keeps_their_prints() {
        let conn = db();
        a_user(&conn, "dan", "admin");
        let sam = a_user(&conn, "sam", "user");
        let id = model(&conn, "dragon");
        add_print(&conn, id, &a_print("success"), Some(sam)).unwrap();

        crate::core::auth::delete_user(&conn, sam).unwrap();
        let history = list_prints(&conn, id).unwrap();
        assert_eq!(history.len(), 1, "the record survives");
        assert_eq!(history[0].by, "", "but is no longer attributed");
        assert_eq!(get_model(&conn, id).unwrap().unwrap().print_count, 1);
    }

    #[test]
    fn a_shared_collection_is_visible_to_everyone() {
        let conn = db();
        let dan = a_user(&conn, "dan", "admin");
        let sam = a_user(&conn, "sam", "user");
        create_collection(&conn, "Shop shelf", None).unwrap();

        assert_eq!(list_collections(&conn, Some(dan)).unwrap().len(), 1);
        assert_eq!(list_collections(&conn, Some(sam)).unwrap().len(), 1);
    }

    #[test]
    fn a_personal_collection_is_only_listed_for_its_owner() {
        let conn = db();
        let dan = a_user(&conn, "dan", "admin");
        let sam = a_user(&conn, "sam", "user");
        create_collection(&conn, "Shop shelf", None).unwrap();
        create_collection(&conn, "Sams queue", Some(sam)).unwrap();

        let for_sam = list_collections(&conn, Some(sam)).unwrap();
        assert_eq!(for_sam.len(), 2);

        let for_dan = list_collections(&conn, Some(dan)).unwrap();
        assert_eq!(for_dan.len(), 1, "someone else's queue is not their business");
        assert_eq!(for_dan[0].name, "Shop shelf");
    }

    /// The desktop app has one person and no accounts, so everything is shared
    /// and a viewer of None must still see it.
    #[test]
    fn with_no_accounts_at_all_shared_collections_still_list() {
        let conn = db();
        create_collection(&conn, "Dragons", None).unwrap();
        assert_eq!(list_collections(&conn, None).unwrap().len(), 1);
    }

    // ---------------------------------------------------------- slices ----

    fn add_slice(conn: &Connection, path: &str) {
        upsert_slice(conn, path, 1024, 0, &crate::core::sliced::SliceMeta::default()).unwrap();
    }

    #[test]
    fn a_slice_named_after_a_model_attaches_to_it() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/knight.stl", 10, "a");
        model_at(&conn, "/lib/Hero/goblin.stl", 10, "b");
        add_slice(&conn, "/lib/Hero/knight.gcode");

        assert_eq!(match_slices(&conn).unwrap(), 1);
        let slices = slices_for_model(&conn, id).unwrap();
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].format, "gcode");
    }

    #[test]
    fn a_slice_alone_beside_one_model_attaches_to_it() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/knight.stl", 10, "a");
        add_slice(&conn, "/lib/Hero/plate_final_v2.ctb");

        assert_eq!(match_slices(&conn).unwrap(), 1);
        assert_eq!(slices_for_model(&conn, id).unwrap().len(), 1);
    }

    /// The case worth refusing: several models, no name match. Guessing here
    /// attaches the wrong output to the wrong model, which is worse than
    /// leaving it for someone to say.
    #[test]
    fn an_ambiguous_slice_is_left_unattached() {
        let conn = db();
        model_at(&conn, "/lib/Pack/knight.stl", 10, "a");
        model_at(&conn, "/lib/Pack/goblin.stl", 10, "b");
        model_at(&conn, "/lib/Pack/ogre.stl", 10, "c");
        add_slice(&conn, "/lib/Pack/plate_1.gcode");

        assert_eq!(match_slices(&conn).unwrap(), 0);
        assert_eq!(unattached_slices(&conn).unwrap().len(), 1);
    }

    /// A plate sliced from a package belongs to the package, not to whichever
    /// part happens to sort first.
    #[test]
    fn a_plate_in_a_package_folder_attaches_to_the_package() {
        let conn = db();
        let a = model_at(&conn, "/lib/Throk/body.stl", 10, "a");
        let b = model_at(&conn, "/lib/Throk/arm.stl", 10, "b");
        let pkg = create_package(&conn, "Throk", "/lib/Throk").unwrap();
        assign_to_package(&conn, pkg.id, &[a, b]).unwrap();
        add_slice(&conn, "/lib/Throk/whole_plate.ctb");

        assert_eq!(match_slices(&conn).unwrap(), 1);
        // Reachable from either part, because it is the model's output.
        assert_eq!(slices_for_model(&conn, a).unwrap().len(), 1);
        assert_eq!(slices_for_model(&conn, b).unwrap().len(), 1);
        assert!(unattached_slices(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_slice_one_folder_up_still_finds_its_model() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/Unsupported/knight.stl", 10, "a");
        add_slice(&conn, "/lib/Hero/knight.gcode");
        assert_eq!(match_slices(&conn).unwrap(), 1);
        assert_eq!(slices_for_model(&conn, id).unwrap().len(), 1);
    }

    #[test]
    fn attaching_by_hand_overrides_and_detaching_works() {
        let conn = db();
        let knight = model_at(&conn, "/lib/Pack/knight.stl", 10, "a");
        model_at(&conn, "/lib/Pack/goblin.stl", 10, "b");
        add_slice(&conn, "/lib/Pack/plate.gcode");
        let orphan = unattached_slices(&conn).unwrap();
        assert_eq!(orphan.len(), 1);

        set_slice_owner(&conn, orphan[0].id, Some(knight), None).unwrap();
        assert_eq!(slices_for_model(&conn, knight).unwrap().len(), 1);
        assert_eq!(get_model(&conn, knight).unwrap().unwrap().slice_count, 1);

        set_slice_owner(&conn, orphan[0].id, None, None).unwrap();
        assert!(slices_for_model(&conn, knight).unwrap().is_empty());
        assert_eq!(unattached_slices(&conn).unwrap().len(), 1);
    }

    #[test]
    fn rescanning_a_slice_updates_it_rather_than_duplicating() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/knight.stl", 10, "a");
        add_slice(&conn, "/lib/Hero/knight.gcode");
        match_slices(&conn).unwrap();

        upsert_slice(
            &conn,
            "/lib/Hero/knight.gcode",
            2048,
            99,
            &crate::core::sliced::SliceMeta {
                layer_height: Some(0.1),
                print_time_secs: Some(3600),
                material_grams: Some(14.0),
                printer: Some("MK4".into()),
            },
        )
        .unwrap();

        let slices = slices_for_model(&conn, id).unwrap();
        assert_eq!(slices.len(), 1, "re-slicing must not create a second row");
        assert_eq!(slices[0].size, 2048);
        assert_eq!(slices[0].layer_height, Some(0.1));
        assert_eq!(slices[0].printer, "MK4");
        // The attachment survives a re-slice.
        assert_eq!(slices[0].model_id, Some(id));
    }

    /// The payoff: a log entry that knows which file was run.
    #[test]
    fn a_print_can_record_which_slice_was_used() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/knight.stl", 10, "a");
        add_slice(&conn, "/lib/Hero/knight.gcode");
        match_slices(&conn).unwrap();
        let slice = slices_for_model(&conn, id).unwrap()[0].id;

        let mut entry = a_print("success");
        entry.slice_id = Some(slice);
        add_print(&conn, id, &entry, None).unwrap();

        assert_eq!(list_prints(&conn, id).unwrap()[0].slice_id, Some(slice));
    }

    // ---------------------------------------------------------- prints ----

    fn a_print(outcome: &str) -> crate::core::models::NewPrint {
        crate::core::models::NewPrint {
            outcome: outcome.into(),
            printer: "Mars 4 Ultra".into(),
            material: "ABS-like grey".into(),
            settings: "0.05mm, 2.4s exposure".into(),
            notes: "supports on the base".into(),
            ..Default::default()
        }
    }

    #[test]
    fn logging_a_print_shows_up_on_the_model() {
        let conn = db();
        let id = model(&conn, "dragon");
        assert_eq!(get_model(&conn, id).unwrap().unwrap().print_count, 0);

        add_print(&conn, id, &a_print("success"), None).unwrap();
        let m = get_model(&conn, id).unwrap().unwrap();
        assert_eq!(m.print_count, 1);
        assert!(m.last_printed.is_some());

        let history = list_prints(&conn, id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].printer, "Mars 4 Ultra");
        assert_eq!(history[0].outcome, crate::core::models::PrintOutcome::Success);
    }

    #[test]
    fn a_failed_attempt_is_still_a_record() {
        let conn = db();
        let id = model(&conn, "dragon");
        add_print(&conn, id, &a_print("failed"), None).unwrap();
        add_print(&conn, id, &a_print("success"), None).unwrap();

        let history = list_prints(&conn, id).unwrap();
        assert_eq!(history.len(), 2);
        // Both count: how many attempts a model took is the interesting part.
        assert_eq!(get_model(&conn, id).unwrap().unwrap().print_count, 2);
    }

    #[test]
    fn an_unknown_outcome_is_refused() {
        let conn = db();
        let id = model(&conn, "dragon");
        assert!(add_print(&conn, id, &a_print("maybe"), None).is_err());
        assert!(add_print(&conn, id, &a_print(""), None).is_err());
        assert_eq!(list_prints(&conn, id).unwrap().len(), 0);
    }

    #[test]
    fn a_print_needs_a_model_that_exists() {
        let conn = db();
        assert!(add_print(&conn, 9999, &a_print("success"), None).is_err());
    }

    /// The question a pile of downloads cannot answer.
    #[test]
    fn never_printed_is_filterable() {
        let conn = db();
        let printed = model(&conn, "printed");
        model(&conn, "untouched");
        add_print(&conn, printed, &a_print("success"), None).unwrap();

        assert_eq!(
            names(&conn, &LibraryQuery { printed: Some(true), ..Default::default() }),
            vec!["printed"]
        );
        assert_eq!(
            names(&conn, &LibraryQuery { printed: Some(false), ..Default::default() }),
            vec!["untouched"]
        );
        // Unset means no filter at all, not "false".
        assert_eq!(names(&conn, &LibraryQuery::default()).len(), 2);
    }

    #[test]
    fn deleting_a_print_leaves_the_rest() {
        let conn = db();
        let id = model(&conn, "dragon");
        let first = add_print(&conn, id, &a_print("failed"), None).unwrap();
        add_print(&conn, id, &a_print("success"), None).unwrap();

        delete_print(&conn, first).unwrap();
        assert_eq!(get_model(&conn, id).unwrap().unwrap().print_count, 1);
        assert_eq!(list_prints(&conn, id).unwrap()[0].outcome,
                   crate::core::models::PrintOutcome::Success);
    }

    /// Orphaned history would inflate the printed count forever.
    #[test]
    fn deleting_a_model_takes_its_print_history() {
        let conn = db();
        let id = model(&conn, "dragon");
        add_print(&conn, id, &a_print("success"), None).unwrap();
        assert_eq!(library_stats(&conn).unwrap().printed, 1);

        delete_model(&conn, id).unwrap();
        assert_eq!(library_stats(&conn).unwrap().printed, 0);
        assert_eq!(list_prints(&conn, id).unwrap().len(), 0);
    }

    // --------------------------------------------------------- renders ----

    fn add_image(conn: &Connection, path: &str, size: i64) {
        let name = std::path::Path::new(path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        upsert_image(conn, path, &name, size, 0).unwrap();
    }

    #[test]
    fn a_render_beside_a_model_is_matched_to_it() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/hero.stl", 10, "a");
        add_image(&conn, "/lib/Hero/hero.jpg", 500_000);

        assert_eq!(match_images(&conn).unwrap(), 1);
        assert!(get_model(&conn, id).unwrap().unwrap().render.is_some());
    }

    #[test]
    fn the_render_named_after_the_model_wins_over_a_bigger_one() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/hero.stl", 10, "a");
        add_image(&conn, "/lib/Hero/group shot.jpg", 9_000_000);
        add_image(&conn, "/lib/Hero/hero.jpg", 100_000);

        match_images(&conn).unwrap();
        let render = get_model(&conn, id).unwrap().unwrap().render.unwrap();
        assert!(render.ends_with("hero.jpg"), "picked {}", render);
    }

    #[test]
    fn without_a_name_match_the_largest_render_wins() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/body.stl", 10, "a");
        add_image(&conn, "/lib/Hero/icon.png", 20_000);
        add_image(&conn, "/lib/Hero/hero shot.jpg", 4_000_000);

        match_images(&conn).unwrap();
        let render = get_model(&conn, id).unwrap().unwrap().render.unwrap();
        assert!(render.ends_with("hero shot.jpg"), "picked {}", render);
    }

    /// Creators often keep one render beside the folder of parts.
    #[test]
    fn a_render_one_level_up_is_used_when_the_folder_has_none() {
        let conn = db();
        let id = model_at(&conn, "/lib/Throk/Unsupported/body.stl", 10, "a");
        add_image(&conn, "/lib/Throk/promo.jpg", 500_000);

        assert_eq!(match_images(&conn).unwrap(), 1);
        assert!(get_model(&conn, id).unwrap().unwrap().render.is_some());
    }

    #[test]
    fn the_models_own_folder_beats_the_one_above_it() {
        let conn = db();
        let id = model_at(&conn, "/lib/Throk/Unsupported/body.stl", 10, "a");
        add_image(&conn, "/lib/Throk/promo.jpg", 9_000_000);
        add_image(&conn, "/lib/Throk/Unsupported/closer.jpg", 50_000);

        match_images(&conn).unwrap();
        let render = get_model(&conn, id).unwrap().unwrap().render.unwrap();
        assert!(render.ends_with("closer.jpg"), "picked {}", render);
    }

    #[test]
    fn a_model_with_no_render_nearby_keeps_none() {
        let conn = db();
        let id = model_at(&conn, "/lib/Alone/a.stl", 10, "a");
        add_image(&conn, "/lib/Elsewhere/pic.jpg", 500_000);

        assert_eq!(match_images(&conn).unwrap(), 0);
        assert!(get_model(&conn, id).unwrap().unwrap().render.is_none());
    }

    /// A choice made by hand must survive the next scan.
    #[test]
    fn matching_does_not_override_a_chosen_render() {
        let conn = db();
        let id = model_at(&conn, "/lib/Hero/hero.stl", 10, "a");
        add_image(&conn, "/lib/Hero/hero.jpg", 500_000);
        add_image(&conn, "/lib/Hero/other.jpg", 900_000);
        let others = images_near_model(&conn, id).unwrap();
        let chosen = others.iter().find(|(_, p)| p.ends_with("other.jpg")).unwrap().0;

        set_model_image(&conn, id, Some(chosen)).unwrap();
        assert_eq!(match_images(&conn).unwrap(), 0, "already set, so nothing to do");
        let render = get_model(&conn, id).unwrap().unwrap().render.unwrap();
        assert!(render.ends_with("other.jpg"), "picked {}", render);
    }

    // ----------------------------------------------------- duplicates ----

    /// Their library has Throk twice. Same bytes, different folders.
    #[test]
    fn identical_files_are_reported_once_with_the_space_they_waste() {
        let conn = db();
        model_at(&conn, "/lib/Gods/Throk/body.stl", 50_000_000, "samehash");
        model_at(&conn, "/lib/2/body.stl", 50_000_000, "samehash");
        model_at(&conn, "/lib/Other/unique.stl", 10, "different");

        let dupes = duplicate_groups(&conn).unwrap();
        assert_eq!(dupes.len(), 1, "only the repeated hash");
        assert_eq!(dupes[0].copies, 2);
        assert_eq!(dupes[0].models.len(), 2);
        // Keeping one copy returns the other.
        assert_eq!(dupes[0].wasted_bytes, 50_000_000);
    }

    #[test]
    fn a_library_with_no_repeats_reports_none() {
        let conn = db();
        model_at(&conn, "/lib/a.stl", 10, "a");
        model_at(&conn, "/lib/b.stl", 10, "b");
        assert!(duplicate_groups(&conn).unwrap().is_empty());
    }

    // ----------------------------------------------------- provenance ----

    #[test]
    fn provenance_is_stored_and_editable() {
        let conn = db();
        let id = upsert_model(
            &conn, "/lib/Creator/x.stl", "x", 10, "h", 12, [1.0, 1.0, 1.0], 1.0, 100,
            ("Homonculus Creations", "Patreon 0121"), "", true,
        )
        .unwrap()
        .0;
        let m = get_model(&conn, id).unwrap().unwrap();
        assert_eq!(m.creator, "Homonculus Creations");
        assert_eq!(m.source, "Patreon 0121");

        set_provenance(&conn, id, "Homonculus", "Patreon Jan 2021").unwrap();
        let m = get_model(&conn, id).unwrap().unwrap();
        assert_eq!(m.creator, "Homonculus");
        assert_eq!(list_creators(&conn).unwrap()[0].name, "Homonculus");
    }

    /// A correction must survive a rescan. Re-guessing would quietly undo
    /// every fix the moment the folder is scanned again.
    #[test]
    fn a_rescan_does_not_overwrite_corrected_provenance() {
        let conn = db();
        let id = upsert_model(
            &conn, "/lib/Gods/x.stl", "x", 10, "h1", 12, [1.0, 1.0, 1.0], 1.0, 100,
            ("Gods", "Patreon 0121"), "", true,
        )
        .unwrap()
        .0;
        set_provenance(&conn, id, "Dark Gods Studio", "Patreon Jan 2021").unwrap();

        // The file changed, so this is an update rather than a no-op.
        upsert_model(
            &conn, "/lib/Gods/x.stl", "x", 99, "h2", 12, [1.0, 1.0, 1.0], 1.0, 200,
            ("Gods", "Patreon 0121"), "", true,
        )
        .unwrap();

        let m = get_model(&conn, id).unwrap().unwrap();
        assert_eq!(m.creator, "Dark Gods Studio", "the correction was clobbered");
        assert_eq!(m.size, 99, "but the file facts did update");
    }

    #[test]
    fn collections_are_created_listed_and_counted() {
        let conn = db();
        let dragons = create_collection_shared(&conn, "Dragons").unwrap();
        create_collection_shared(&conn, "Terrain").unwrap();

        // An empty collection still appears -- you make one before you fill it.
        let listed = list_collections(&conn, None).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Dragons");
        assert_eq!(listed[0].count, 0);

        let a = model(&conn, "smaug");
        let b = model(&conn, "wyvern");
        assert_eq!(add_to_collection(&conn, dragons.id, &[a, b]).unwrap(), 2);

        let listed = list_collections(&conn, None).unwrap();
        assert_eq!(listed[0].count, 2);
        assert_eq!(listed[1].count, 0);
    }

    #[test]
    fn adding_the_same_model_twice_does_not_duplicate_it() {
        let conn = db();
        let c = create_collection_shared(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");

        assert_eq!(add_to_collection(&conn, c.id, &[a]).unwrap(), 1);
        // Second add reports nothing changed rather than creating a second row.
        assert_eq!(add_to_collection(&conn, c.id, &[a]).unwrap(), 0);
        assert_eq!(list_collections(&conn, None).unwrap()[0].count, 1);
    }

    #[test]
    fn duplicate_names_are_refused_regardless_of_case() {
        let conn = db();
        create_collection_shared(&conn, "Dragons").unwrap();
        // COLLATE NOCASE: "dragons" and "Dragons" are the same collection, and
        // silently making two would be worse than refusing.
        let err = create_collection_shared(&conn, "dragons").unwrap_err();
        assert!(err.contains("already exists"), "got: {}", err);
        assert!(create_collection_shared(&conn, "  ").is_err());
        assert_eq!(list_collections(&conn, None).unwrap().len(), 1);
    }

    #[test]
    fn filtering_by_collection_narrows_the_library() {
        let conn = db();
        let c = create_collection_shared(&conn, "Dragons").unwrap();
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
        let c = create_collection_shared(&conn, "Dragons").unwrap();
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
        let c = create_collection_shared(&conn, "Dragons").unwrap();
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
        let c = create_collection_shared(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        assert_eq!(remove_from_collection(&conn, c.id, &[a]).unwrap(), 1);
        assert_eq!(list_collections(&conn, None).unwrap()[0].count, 0);
        // The model itself survives.
        assert!(get_model(&conn, a).unwrap().is_some());
    }

    #[test]
    fn deleting_a_collection_does_not_delete_its_models() {
        let conn = db();
        let c = create_collection_shared(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        delete_collection(&conn, c.id).unwrap();
        assert!(list_collections(&conn, None).unwrap().is_empty());
        assert!(get_model(&conn, a).unwrap().is_some());
        assert_eq!(get_model(&conn, a).unwrap().unwrap().collections.len(), 0);
    }

    /// The reason delete_model touches collection_models: otherwise every
    /// collection that held the model reports a count that can never be right.
    #[test]
    fn deleting_a_model_removes_it_from_collections() {
        let conn = db();
        let c = create_collection_shared(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        let b = model(&conn, "wyvern");
        add_to_collection(&conn, c.id, &[a, b]).unwrap();

        delete_model(&conn, a).unwrap();
        assert_eq!(list_collections(&conn, None).unwrap()[0].count, 1);
    }

    #[test]
    fn renaming_a_collection_keeps_its_members() {
        let conn = db();
        let c = create_collection_shared(&conn, "Dragons").unwrap();
        let a = model(&conn, "smaug");
        add_to_collection(&conn, c.id, &[a]).unwrap();

        rename_collection(&conn, c.id, "Big Lizards").unwrap();
        let listed = list_collections(&conn, None).unwrap();
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

// -------------------------------------------------------------- packages ----

/// Folder names that describe a *variant* of a model rather than a model.
///
/// `Throk_130mm_Unsupported/` is not a thing you printed, it is one rendering
/// of a thing. Collapsing these is what lets the supported and unsupported
/// copies of one model be recognised as the same model.
fn is_variant_folder(name: &str) -> bool {
    let flat: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    matches!(
        flat.as_str(),
        "supported"
            | "unsupported"
            | "presupported"
            | "stl"
            | "stls"
            | "files"
            | "printfiles"
            | "parts"
            | "mesh"
            | "meshes"
            | "lys"
            | "lychee"
            | "chitubox"
            | "ctb"
    ) || (!name.is_empty() && name.chars().all(|c| c.is_ascii_digit()))
}

/// Walk up out of variant folders to the directory that names the model.
fn collapse_dir(dir: &Path) -> std::path::PathBuf {
    let mut current = dir.to_path_buf();
    // Bounded: nested variant folders are rare and runaway recursion toward
    // the filesystem root would turn unrelated models into one package.
    for _ in 0..3 {
        let name = match current.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => break,
        };
        if !is_variant_folder(&name) {
            break;
        }
        match current.parent() {
            Some(parent) if parent.file_name().is_some() => current = parent.to_path_buf(),
            _ => break,
        }
    }
    current
}

/// Strip a trailing variant word from a folder name.
///
/// `Throk_130mm_Unsupported` is the Throk, not a thing called
/// "Throk 130mm Unsupported". The folder itself is not collapsed away -- that
/// would risk merging unrelated models that happen to sit side by side -- but
/// the package it suggests is named for the model.
///
/// Order matters: "unsupported" has to be tested before "supported", or every
/// unsupported folder gets renamed to end in "un".
/// Strip a trailing "2", "3" or "copy" that a file manager added when it
/// duplicated a folder.
///
/// Applied only as a step toward recognising a variant folder, and reverted if
/// that fails -- otherwise a model legitimately called "Dragon 2" would lose
/// its number.
fn strip_copy_marker(name: &str) -> &str {
    let trimmed = name.trim_end();
    if let Some(idx) = trimmed.rfind(' ') {
        let tail = &trimmed[idx + 1..];
        let is_copy_number = !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit());
        if is_copy_number || tail.eq_ignore_ascii_case("copy") {
            return trimmed[..idx].trim_end();
        }
    }
    trimmed
}

fn strip_variant_suffix(name: &str) -> &str {
    const TOKENS: [&str; 8] = [
        "unsupported",
        "presupported",
        "supported",
        "printfiles",
        "stls",
        "stl",
        "files",
        "parts",
    ];
    let lower = name.to_ascii_lowercase();
    for token in TOKENS {
        for sep in ['_', '-', ' '] {
            let suffix = format!("{}{}", sep, token);
            if lower.ends_with(&suffix) && name.len() > suffix.len() {
                return &name[..name.len() - suffix.len()];
            }
        }
    }
    name
}

fn tidy_name(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn create_package(conn: &Connection, name: &str, root: &str) -> std::result::Result<Package, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A package needs a name".into());
    }
    conn.execute(
        "INSERT INTO packages (name, root, created_at) VALUES (?1, ?2, ?3)",
        params![name, root, now()],
    )
    .map_err(|e| format!("Could not create package: {}", e))?;
    Ok(Package {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        root: root.to_string(),
        parts: 0,
        total_bytes: 0,
    })
}

pub fn list_packages(conn: &Connection) -> Result<Vec<Package>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, COALESCE(p.root, ''), COUNT(m.id), COALESCE(SUM(m.size), 0)
         FROM packages p LEFT JOIN models m ON m.package_id = p.id
         GROUP BY p.id
         ORDER BY p.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Package {
            id: r.get(0)?,
            name: r.get(1)?,
            root: r.get(2)?,
            parts: r.get(3)?,
            total_bytes: r.get(4)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn assign_to_package(conn: &Connection, package_id: i64, model_ids: &[i64]) -> Result<usize> {
    if model_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare("UPDATE models SET package_id = ?1 WHERE id = ?2")?;
        for id in model_ids {
            changed += stmt.execute(params![package_id, id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// Take files back out of whatever package they were in. The files are
/// untouched; only the grouping is undone.
pub fn unassign_from_package(conn: &Connection, model_ids: &[i64]) -> Result<usize> {
    if model_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    {
        let mut stmt = tx.prepare("UPDATE models SET package_id = NULL WHERE id = ?1")?;
        for id in model_ids {
            changed += stmt.execute(params![id])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

pub fn delete_package(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE models SET package_id = NULL WHERE package_id = ?1", params![id])?;
    conn.execute("DELETE FROM packages WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn rename_package(conn: &Connection, id: i64, name: &str) -> std::result::Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A package needs a name".into());
    }
    conn.execute("UPDATE packages SET name = ?2 WHERE id = ?1", params![id, name])
        .map_err(|e| format!("Could not rename package: {}", e))?;
    Ok(())
}

/// Groupings that look like one model in parts -- offered, never applied.
///
/// A folder of STLs is as often a folder of unrelated models as it is one
/// model in pieces, so this proposes and the person decides. Only files that
/// are not already in a package are considered.
pub fn suggest_packages(conn: &Connection) -> Result<Vec<PackageSuggestion>> {
    let mut stmt =
        conn.prepare("SELECT id, path FROM models WHERE package_id IS NULL ORDER BY path")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut groups: HashMap<String, Vec<i64>> = HashMap::new();
    for (id, path) in rows.flatten() {
        let parent = match Path::new(&path).parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let key = collapse_dir(&parent).to_string_lossy().to_string();
        groups.entry(key).or_default().push(id);
    }

    let mut out: Vec<PackageSuggestion> = groups
        .into_iter()
        // One file in a folder is not a model in parts; it is just a file.
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(root, ids)| {
            let name = Path::new(&root)
                .file_name()
                .map(|s| {
                    let raw = s.to_string_lossy();
                    // Try dropping a duplicate-folder marker first, but keep it
                    // unless doing so actually reveals a variant folder.
                    let without_copy = strip_copy_marker(&raw);
                    let stripped = strip_variant_suffix(without_copy);
                    if stripped.len() < without_copy.len() {
                        tidy_name(stripped)
                    } else {
                        tidy_name(strip_variant_suffix(&raw))
                    }
                })
                .unwrap_or_else(|| "Package".to_string());
            PackageSuggestion {
                parts: ids.len() as i64,
                name,
                root,
                model_ids: ids,
            }
        })
        .collect();
    out.sort_by(|a, b| b.parts.cmp(&a.parts).then(a.name.cmp(&b.name)));
    Ok(out)
}

// ------------------------------------------------------------ duplicates ----

/// The same bytes stored more than once.
///
/// Content hashes are already computed for every file, so this costs one
/// grouped query and tells you exactly how much disk a tidy-up would return.
pub fn duplicate_groups(conn: &Connection) -> Result<Vec<DuplicateGroup>> {
    let mut stmt = conn.prepare(
        "SELECT hash, COUNT(*), MIN(size), MIN(name)
         FROM models
         GROUP BY hash
         HAVING COUNT(*) > 1
         ORDER BY (COUNT(*) - 1) * MIN(size) DESC",
    )?;
    let heads = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .flatten()
        .collect::<Vec<_>>();

    let mut out = Vec::new();
    for (hash, copies, size, name) in heads {
        let mut models = models_with_hash(conn, &hash)?;
        attach_tags(conn, &mut models)?;
        out.push(DuplicateGroup {
            hash,
            name,
            copies,
            // Keeping one copy returns the rest.
            wasted_bytes: size * (copies - 1),
            models,
        });
    }
    Ok(out)
}

fn models_with_hash(conn: &Connection, hash: &str) -> Result<Vec<ModelRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM models WHERE hash = ?1 ORDER BY path",
    )?;
    let ids: Vec<i64> = stmt.query_map(params![hash], |r| r.get(0))?.flatten().collect();
    let mut out = Vec::new();
    for id in ids {
        if let Some(m) = get_model(conn, id)? {
            out.push(m);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------ provenance ----

pub fn set_provenance(
    conn: &Connection,
    model_id: i64,
    creator: &str,
    source: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE models SET creator = ?2, source = ?3 WHERE id = ?1",
        params![model_id, creator.trim(), source.trim()],
    )?;
    Ok(())
}

/// Every creator seen in the library, with how many models each has.
pub fn list_creators(conn: &Connection) -> Result<Vec<TagCount>> {
    let mut stmt = conn.prepare(
        "SELECT creator, COUNT(*) FROM models
         WHERE creator <> ''
         GROUP BY creator
         ORDER BY COUNT(*) DESC, creator COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TagCount {
            name: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

// ----------------------------------------------------------------- images ----

pub fn upsert_image(
    conn: &Connection,
    path: &str,
    name: &str,
    size: i64,
    mtime: i64,
) -> Result<()> {
    let dir = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO images (path, name, dir, size, mtime) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET size = ?4, mtime = ?5",
        params![path, name, dir, size, mtime],
    )?;
    Ok(())
}

/// Point every model that does not have one at the best render beside it.
///
/// Creators ship promo images in the same folder as the STLs, and one of those
/// beats anything generated from the mesh -- it is posed, lit, often painted.
///
/// Two passes, because renders sometimes sit one level up from the parts: the
/// model's own folder wins, and only then the folder above it.
pub fn match_images(conn: &Connection) -> Result<usize> {
    // Ranked in Rust, not in SQL.
    //
    // The natural query correlates the subquery's ORDER BY against the outer
    // row -- "prefer the image whose name equals this model's name" -- and
    // SQLite will not resolve an outer column there: it fails with "no such
    // column: m.dir". Two SELECTs and a HashMap are clearer than fighting it,
    // and this runs once per scan over a set that fits in memory comfortably.
    let mut by_dir: HashMap<String, Vec<(i64, String, i64)>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, dir, name, size FROM images")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for (id, dir, name, size) in rows.flatten() {
            by_dir.entry(dir).or_default().push((id, name, size));
        }
    }
    if by_dir.is_empty() {
        return Ok(0);
    }

    // A render named after the model is the one the creator meant; failing
    // that the largest file is the hero shot rather than an icon.
    let best = |candidates: &[(i64, String, i64)], model_name: &str| -> Option<i64> {
        candidates
            .iter()
            .max_by_key(|(_, name, size)| {
                let named_for_model = name.eq_ignore_ascii_case(model_name);
                (named_for_model, *size)
            })
            .map(|(id, _, _)| *id)
    };

    let wanted: Vec<(i64, String, String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, dir, parent_dir, hash FROM models WHERE image_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.flatten().collect()
    };

    let tx = conn.unchecked_transaction()?;
    let mut matched = 0usize;
    let mut newly_rendered: Vec<String> = Vec::new();
    {
        let mut set = tx.prepare("UPDATE models SET image_id = ?2 WHERE id = ?1")?;
        for (model_id, name, dir, parent_dir, hash) in wanted {
            // The model's own folder wins; only then the folder above it,
            // where creators often keep one render for a set of parts.
            let chosen = by_dir
                .get(&dir)
                .and_then(|c| best(c, &name))
                .or_else(|| by_dir.get(&parent_dir).and_then(|c| best(c, &name)));
            if let Some(image_id) = chosen {
                set.execute(params![model_id, image_id])?;
                newly_rendered.push(hash);
                matched += 1;
            }
        }
    }
    tx.commit()?;

    // Previews are keyed by content hash, and gaining a render does not change
    // a single byte of the mesh -- so nothing else would ever invalidate them.
    // Without this a model whose bytes were already previewed keeps the
    // generated picture forever, even once the creator's own render is sitting
    // next to it.
    for hash in newly_rendered {
        crate::core::thumbs::forget(&hash);
    }
    Ok(matched)
}

/// The render chosen for a model, if any.
pub fn image_for_model(conn: &Connection, model_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT i.path FROM models m JOIN images i ON i.id = m.image_id WHERE m.id = ?1",
        params![model_id],
        |r| r.get(0),
    )
    .optional()
}

pub fn image_for_hash(conn: &Connection, hash: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT i.path FROM models m JOIN images i ON i.id = m.image_id
         WHERE m.hash = ?1 AND m.image_id IS NOT NULL LIMIT 1",
        params![hash],
        |r| r.get(0),
    )
    .optional()
}

/// Every render sitting beside a model, so the inspector can offer a choice.
pub fn images_near_model(conn: &Connection, model_id: i64) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.path FROM images i, models m
         WHERE m.id = ?1 AND (i.dir = m.dir OR i.dir = m.parent_dir)
         ORDER BY i.size DESC",
    )?;
    let rows = stmt.query_map(params![model_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.flatten().collect())
}

/// Choose a render by hand, or pass None to go back to the generated preview.
pub fn set_model_image(conn: &Connection, model_id: i64, image_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE models SET image_id = ?2 WHERE id = ?1",
        params![model_id, image_id],
    )?;
    Ok(())
}

// ----------------------------------------------------------------- prints ----

pub fn add_print(
    conn: &Connection,
    model_id: i64,
    entry: &crate::core::models::NewPrint,
    user_id: Option<i64>,
) -> std::result::Result<i64, String> {
    let outcome = crate::core::models::PrintOutcome::parse(&entry.outcome)
        .ok_or_else(|| "Outcome must be success, partial or failed".to_string())?;
    // A print with no model is a note about nothing.
    if get_model(conn, model_id).map_err(|e| e.to_string())?.is_none() {
        return Err("No such model".into());
    }
    conn.execute(
        "INSERT INTO prints
            (model_id, printed_at, printer, material, settings, outcome, notes, photo,
             slice_id, user_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            model_id,
            entry.printed_at.unwrap_or_else(now),
            entry.printer.trim(),
            entry.material.trim(),
            entry.settings.trim(),
            outcome.as_str(),
            entry.notes.trim(),
            entry.photo,
            entry.slice_id,
            user_id,
        ],
    )
    .map_err(|e| format!("Could not save print: {}", e))?;
    Ok(conn.last_insert_rowid())
}

pub fn list_prints(conn: &Connection, model_id: i64) -> Result<Vec<PrintRecord>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.model_id, p.printed_at, p.printer, p.material, p.settings, p.outcome,
                p.notes, p.photo, p.slice_id,
                COALESCE((SELECT username FROM users WHERE users.id = p.user_id), '')
         FROM prints p WHERE p.model_id = ?1
         ORDER BY p.printed_at DESC, p.id DESC",
    )?;
    let rows = stmt.query_map(params![model_id], |r| {
        Ok(PrintRecord {
            id: r.get(0)?,
            model_id: r.get(1)?,
            printed_at: r.get(2)?,
            printer: r.get(3)?,
            material: r.get(4)?,
            settings: r.get(5)?,
            outcome: crate::core::models::PrintOutcome::parse(&r.get::<_, String>(6)?)
                .unwrap_or(crate::core::models::PrintOutcome::Success),
            notes: r.get(7)?,
            photo: r.get(8)?,
            slice_id: r.get(9)?,
            by: r.get(10)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn delete_print(conn: &Connection, print_id: i64) -> Result<()> {
    conn.execute("DELETE FROM prints WHERE id = ?1", params![print_id])?;
    Ok(())
}

/// Deleting a model takes its print history with it -- the history is about
/// that file, and leaving orphans behind would inflate every count forever.
pub fn delete_prints_for_model(conn: &Connection, model_id: i64) -> Result<()> {
    conn.execute("DELETE FROM prints WHERE model_id = ?1", params![model_id])?;
    Ok(())
}

/// Point a model row at a file that has moved.
///
/// Only the organiser calls this, and only after the move succeeded. The
/// denormalised directories have to move with it or image matching and package
/// suggestions would keep describing where the file used to be.
pub fn relocate_model(conn: &Connection, model_id: i64, new_path: &str) -> Result<()> {
    let dir = Path::new(new_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent_dir = Path::new(&dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    conn.execute(
        "UPDATE models SET path = ?2, dir = ?3, parent_dir = ?4 WHERE id = ?1",
        params![model_id, new_path, dir, parent_dir],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- slices ----

fn slice_from_row(r: &rusqlite::Row) -> Result<Slice> {
    Ok(Slice {
        id: r.get(0)?,
        path: r.get(1)?,
        name: r.get(2)?,
        format: r.get(3)?,
        size: r.get(4)?,
        mtime: r.get(5)?,
        layer_height: r.get(6)?,
        print_time_secs: r.get(7)?,
        material_grams: r.get(8)?,
        printer: r.get(9)?,
        model_id: r.get(10)?,
        package_id: r.get(11)?,
    })
}

const SLICE_COLUMNS: &str = "id, path, name, format, size, mtime, layer_height,
     print_time_secs, material_grams, printer, model_id, package_id";

#[allow(clippy::too_many_arguments)]
pub fn upsert_slice(
    conn: &Connection,
    path: &str,
    size: i64,
    mtime: i64,
    meta: &crate::core::sliced::SliceMeta,
) -> Result<()> {
    let file = Path::new(path);
    let name = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent_dir = Path::new(&dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    conn.execute(
        "INSERT INTO slices
            (path, name, dir, parent_dir, format, size, mtime,
             layer_height, print_time_secs, material_grams, printer)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(path) DO UPDATE SET
            size = ?6, mtime = ?7, layer_height = ?8,
            print_time_secs = ?9, material_grams = ?10, printer = ?11",
        params![
            path,
            name,
            dir,
            parent_dir,
            crate::core::sliced::format_of(file),
            size,
            mtime,
            meta.layer_height,
            meta.print_time_secs,
            meta.material_grams,
            meta.printer.clone().unwrap_or_default(),
        ],
    )?;
    Ok(())
}

/// Attach sliced files to what they were sliced from -- but only when it is
/// obvious.
///
/// A slice named after a model, or sitting alone beside one, is unambiguous. A
/// folder holding six models and a plate file is not, and guessing there would
/// attach the wrong output to the wrong model, which is worse than leaving it
/// for someone to say. Same principle as package suggestions.
pub fn match_slices(conn: &Connection) -> Result<usize> {
    let pending: Vec<(i64, String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, dir, parent_dir FROM slices
             WHERE model_id IS NULL AND package_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.flatten().collect()
    };
    if pending.is_empty() {
        return Ok(0);
    }

    let tx = conn.unchecked_transaction()?;
    let mut attached = 0usize;
    {
        let mut nearby = tx.prepare(
            // Three relationships, all of which occur in real packs: the
            // slice beside the model, the slice one level up from a parts
            // folder, and the model one level down from where the slice sits.
            "SELECT id, name, package_id FROM models
             WHERE dir = ?1 OR dir = ?2 OR parent_dir = ?1",
        )?;
        let mut to_model = tx.prepare("UPDATE slices SET model_id = ?2 WHERE id = ?1")?;
        let mut to_package = tx.prepare("UPDATE slices SET package_id = ?2 WHERE id = ?1")?;

        for (slice_id, slice_name, dir, parent_dir) in pending {
            let candidates: Vec<(i64, String, Option<i64>)> = nearby
                .query_map(params![dir, parent_dir], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .flatten()
                .collect();

            if candidates.is_empty() {
                continue;
            }

            // A plate sliced from a package belongs to the package, not to
            // whichever part happens to sort first.
            let packages: Vec<i64> = {
                let mut ids: Vec<i64> = candidates.iter().filter_map(|(_, _, p)| *p).collect();
                ids.sort_unstable();
                ids.dedup();
                ids
            };
            if packages.len() == 1 && candidates.iter().all(|(_, _, p)| p.is_some()) {
                to_package.execute(params![slice_id, packages[0]])?;
                attached += 1;
                continue;
            }

            let named = candidates
                .iter()
                .find(|(_, name, _)| name.eq_ignore_ascii_case(&slice_name));
            let chosen = match named {
                Some((id, _, _)) => Some(*id),
                // Exactly one model nearby, so there is nothing to confuse it
                // with. More than one and no name match: leave it alone.
                None if candidates.len() == 1 => Some(candidates[0].0),
                None => None,
            };

            if let Some(model_id) = chosen {
                to_model.execute(params![slice_id, model_id])?;
                attached += 1;
            }
        }
    }
    tx.commit()?;
    Ok(attached)
}

/// Everything printable for this model, including anything sliced from the
/// package it belongs to.
pub fn slices_for_model(conn: &Connection, model_id: i64) -> Result<Vec<Slice>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SLICE_COLUMNS} FROM slices
         WHERE model_id = ?1
            OR (package_id IS NOT NULL
                AND package_id = (SELECT package_id FROM models WHERE id = ?1))
         ORDER BY mtime DESC"
    ))?;
    let rows = stmt.query_map(params![model_id], |r| slice_from_row(r))?;
    Ok(rows.flatten().collect())
}

/// Sliced files nobody could place. Offered for attaching by hand.
pub fn unattached_slices(conn: &Connection) -> Result<Vec<Slice>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SLICE_COLUMNS} FROM slices
         WHERE model_id IS NULL AND package_id IS NULL
         ORDER BY mtime DESC"
    ))?;
    let rows = stmt.query_map([], |r| slice_from_row(r))?;
    Ok(rows.flatten().collect())
}

/// Attach by hand, or pass None for both to detach.
pub fn set_slice_owner(
    conn: &Connection,
    slice_id: i64,
    model_id: Option<i64>,
    package_id: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE slices SET model_id = ?2, package_id = ?3 WHERE id = ?1",
        params![slice_id, model_id, package_id],
    )?;
    Ok(())
}

/// Rows whose sliced file is gone. Reported, never deleted -- the same reason
/// as missing models: an unplugged drive must not empty the library.
pub fn find_missing_slices(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, path FROM slices")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows
        .flatten()
        .filter(|(_, p)| !Path::new(p).exists())
        .collect())
}

/// Models whose file is no longer where the library left it.
pub fn missing_models(conn: &Connection) -> Result<Vec<ModelRecord>> {
    let mut out = Vec::new();
    for (id, _) in find_missing(conn)? {
        if let Some(m) = get_model(conn, id)? {
            out.push(m);
        }
    }
    Ok(out)
}

/// Point a model at a different file and re-measure it from scratch.
///
/// Everything derived from the bytes is recomputed rather than carried over,
/// because the new file may not be the old one: a re-export, a repaired mesh,
/// a different resolution of the same sculpt. Keeping the old triangle count
/// against a new file would be worse than having no number at all.
pub fn remeasure_model(conn: &Connection, model_id: i64, new_path: &str) -> std::result::Result<(), String> {
    let file = Path::new(new_path);
    if !file.is_file() {
        return Err("There is no file at that path".into());
    }
    // Paths are unique, so a file already in the library cannot be claimed by
    // a second row. Without this the UPDATE fails on a constraint and the
    // person is told nothing useful about why.
    let taken: Option<String> = conn
        .query_row(
            "SELECT name FROM models WHERE path = ?1 AND id <> ?2",
            params![new_path, model_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(other) = taken {
        return Err(format!("\"{}\" already points at that file", other));
    }

    let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let hash = crate::core::scanner::hash_file(file).map_err(|e| e.to_string())?;
    let geometry = crate::core::stl::read_meta(file).unwrap_or_else(|_| crate::core::stl::StlMeta::empty());
    let dims = geometry.dimensions();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent_dir = Path::new(&dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    conn.execute(
        "UPDATE models SET path=?2, dir=?3, parent_dir=?4, size=?5, hash=?6, triangles=?7,
                dim_x=?8, dim_y=?9, dim_z=?10, volume=?11, watertight=?12, mtime=?13
         WHERE id=?1",
        params![
            model_id, new_path, dir, parent_dir, meta.len() as i64, hash,
            i64::from(geometry.triangles), f64::from(dims[0]), f64::from(dims[1]),
            f64::from(dims[2]), f64::from(geometry.volume), geometry.watertight, mtime
        ],
    )
    .map_err(|e| format!("Could not update the model: {}", e))?;

    // The preview is keyed by content hash, so a different file means a
    // different key -- but the old picture would linger under the old one.
    crate::core::thumbs::forget(&hash);
    Ok(())
}

// -------------------------------------------------------------- feedback ----

/// The longest a review may be. Long enough for a considered paragraph, short
/// enough that nobody can post a novel into the moderation queue.
const FEEDBACK_MAX: usize = 4000;

/// Strip what has no business in a review, and cap the length.
///
/// Control characters are removed rather than escaped: they cannot be
/// displayed and their only use is confusing whatever reads the text later.
/// Newlines survive, because people write paragraphs.
fn tidy_submission(raw: &str, max: usize) -> String {
    raw.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

pub fn add_feedback(
    conn: &Connection,
    entry: &crate::core::models::NewFeedback,
) -> std::result::Result<i64, String> {
    let kind = match entry.kind.trim() {
        "review" => "review",
        "idea" => "idea",
        // Anything unrecognised is treated as a problem report rather than
        // refused: a person trying to tell you something should not be told no.
        _ => "problem",
    };

    let body = tidy_submission(&entry.body, FEEDBACK_MAX);
    if body.len() < 4 {
        return Err("Please say a little more than that".into());
    }

    let rating = match (kind, entry.rating) {
        ("review", Some(r)) if (1..=5).contains(&r) => Some(r),
        ("review", _) => return Err("A review needs a rating from 1 to 5".into()),
        _ => None,
    };

    conn.execute(
        "INSERT INTO feedback
            (kind, rating, body, author, printer, app_version, platform,
             may_publish, status, submitted_at, contact)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?10)",
        params![
            kind,
            rating,
            body,
            tidy_submission(&entry.author, 60),
            tidy_submission(&entry.printer, 60),
            tidy_submission(&entry.app_version, 30),
            tidy_submission(&entry.platform, 30),
            entry.may_publish,
            now(),
            tidy_submission(&entry.contact, 120),
        ],
    )
    .map_err(|e| format!("Could not save that: {}", e))?;
    Ok(conn.last_insert_rowid())
}

fn feedback_from_row(r: &rusqlite::Row) -> Result<crate::core::models::Feedback> {
    Ok(crate::core::models::Feedback {
        id: r.get(0)?,
        kind: r.get(1)?,
        rating: r.get(2)?,
        body: r.get(3)?,
        author: r.get(4)?,
        printer: r.get(5)?,
        app_version: r.get(6)?,
        platform: r.get(7)?,
        may_publish: r.get::<_, i64>(8)? != 0,
        status: r.get(9)?,
        submitted_at: r.get(10)?,
        contact: r.get(11)?,
    })
}

const FEEDBACK_COLUMNS: &str = "id, kind, rating, body, author, printer, app_version,
     platform, may_publish, status, submitted_at, contact";

/// The moderation queue. Administrators only.
pub fn list_feedback(conn: &Connection, status: &str) -> Result<Vec<crate::core::models::Feedback>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FEEDBACK_COLUMNS} FROM feedback
         WHERE (?1 = 'all' OR status = ?1)
         ORDER BY submitted_at DESC
         LIMIT 500"
    ))?;
    let rows = stmt.query_map(params![status], |r| feedback_from_row(r))?;
    Ok(rows.flatten().collect())
}

pub fn moderate_feedback(conn: &Connection, id: i64, status: &str) -> std::result::Result<(), String> {
    if !matches!(status, "approved" | "rejected" | "pending") {
        return Err("Status must be approved, rejected or pending".into());
    }
    let changed = conn
        .execute(
            "UPDATE feedback SET status = ?2, moderated_at = ?3 WHERE id = ?1",
            params![id, status, now()],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("No such submission".into());
    }
    Ok(())
}

pub fn delete_feedback(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM feedback WHERE id = ?1", params![id])?;
    Ok(())
}

/// What the public site may show.
///
/// Three conditions, all required: it is a review, a human approved it, and
/// the person who wrote it agreed to it being shown. Contact details are not
/// in the returned shape at all, so they cannot leak by forgetting to strip
/// them.
pub fn public_reviews(conn: &Connection) -> Result<Vec<crate::core::models::PublicReview>> {
    let mut stmt = conn.prepare(
        "SELECT rating, body, author, printer, submitted_at FROM feedback
         WHERE kind = 'review' AND status = 'approved' AND may_publish = 1
           AND rating IS NOT NULL
         ORDER BY submitted_at DESC
         LIMIT 60",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(crate::core::models::PublicReview {
            rating: r.get(0)?,
            body: r.get(1)?,
            author: r.get(2)?,
            printer: r.get(3)?,
            published_at: r.get(4)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn pending_feedback_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM feedback WHERE status = 'pending'",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}
