use std::path::PathBuf;

/// Where the library lives.
///
/// The previous code opened `"stl.db"` -- a *relative* path -- so the database
/// landed wherever the process happened to be launched from. Double-click the
/// app and you got one library; run the server from a shell and you got a
/// different, empty one. Both halves must agree on a single absolute location.
pub fn data_dir() -> PathBuf {
    // An explicit override comes first: a NAS needs the library on the big
    // disk, not in the service account's home directory.
    if let Ok(dir) = std::env::var("STLIBRARIAN_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("STLibrarian")
}

pub fn db_path() -> PathBuf {
    data_dir().join("stl.db")
}

/// Rendered previews, one PNG per model hash.
pub fn thumb_dir() -> PathBuf {
    data_dir().join("thumbnails")
}

pub fn thumb_path(hash: &str) -> PathBuf {
    thumb_dir().join(format!("{}.png", hash))
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(thumb_dir())?;
    Ok(())
}

fn size_of(path: &std::path::Path) -> i64 {
    std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
}

/// Everything needed to answer "what do I back up?"
pub fn location() -> crate::core::models::LibraryLocation {
    let db = db_path();
    // WAL and shared-memory sidecars hold writes that have not been folded
    // into the main file yet; a backup without them can be a moment stale.
    let db_bytes = size_of(&db)
        + size_of(&db.with_extension("db-wal"))
        + size_of(&db.with_extension("db-shm"));

    let thumbs = thumb_dir();
    let thumbs_bytes = std::fs::read_dir(&thumbs)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len() as i64)
                .sum()
        })
        .unwrap_or(0);

    crate::core::models::LibraryLocation {
        data_dir: data_dir().to_string_lossy().to_string(),
        db_path: db.to_string_lossy().to_string(),
        thumbs_dir: thumbs.to_string_lossy().to_string(),
        db_bytes,
        thumbs_bytes,
        overridden: std::env::var("STLIBRARIAN_DATA_DIR")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
    }
}
