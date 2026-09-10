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
