use crate::core::db::{tag_model, upsert_model, Upsert};
use crate::core::models::ScanReport;
use crate::core::stl;
use log::{info, warn};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::{Duration, Instant, UNIX_EPOCH};
use walkdir::WalkDir;

pub fn scan_folder(conn: &Connection, root: &str) -> ScanReport {
    scan_folder_with(conn, root, |_| {})
}

/// A big library takes a while and a progress-less spinner is indistinguishable
/// from a hang, so the caller gets a running tally.
pub fn scan_folder_with<F>(conn: &Connection, root: &str, mut on_progress: F) -> ScanReport
where
    F: FnMut(&ScanReport),
{
    let start = Instant::now();
    let mut last_emit = Instant::now();
    let mut report = ScanReport::default();
    info!("Scanning folder: {}", root);

    // follow_links(false) is load-bearing. The old hand-rolled recursion
    // followed directory symlinks, so a single self-referential link -- which
    // is ordinary on a NAS share -- walked forever.
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() || !is_stl(entry.path()) {
            continue;
        }
        match index_file(conn, entry.path(), Path::new(root)) {
            Ok(Upsert::Added) => report.added += 1,
            Ok(Upsert::Updated) => report.updated += 1,
            Ok(Upsert::Unchanged) => report.unchanged += 1,
            Err(e) => {
                warn!("Skipped {}: {}", entry.path().display(), e);
                report.failed += 1;
            }
        }

        // Throttled by time, not by file count. Counting meant a folder with
        // fewer than 25 STLs never reported anything at all -- and since each
        // of these files can take seconds to hash, the UI sat on "Walking
        // folders" for the entire scan looking like it had hung.
        if last_emit.elapsed() >= Duration::from_millis(200) {
            report.elapsed_ms = start.elapsed().as_millis() as u64;
            on_progress(&report);
            last_emit = Instant::now();
        }
    }

    report.elapsed_ms = start.elapsed().as_millis() as u64;
    info!(
        "Scan finished: +{} ~{} ={} !{} in {}ms",
        report.added, report.updated, report.unchanged, report.failed, report.elapsed_ms
    );
    on_progress(&report);
    report
}

/// Case-insensitive on purpose: `.STL` is what most model sites hand you.
fn is_stl(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("stl"))
        .unwrap_or(false)
}

fn index_file(conn: &Connection, path: &Path, root: &Path) -> Result<Upsert, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let size = meta.len() as i64;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let hash = hash_file(path).map_err(|e| e.to_string())?;
    let geometry = stl::read_meta(path).unwrap_or_else(|_| stl::StlMeta::empty());
    let dims = geometry.dimensions();
    let path_str = path.to_string_lossy().to_string();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.clone());

    let (creator, source) = provenance(path, root);
    let (model_id, outcome) = upsert_model(
        conn,
        &path_str,
        &name,
        size,
        &hash,
        i64::from(geometry.triangles),
        [dims[0] as f64, dims[1] as f64, dims[2] as f64],
        f64::from(geometry.volume),
        mtime,
        (&creator, &source),
    )
    .map_err(|e| e.to_string())?;

    // Only a brand-new model gets the starter tag. Re-tagging on every rescan
    // would resurrect "untagged" on models the user has since organised.
    if outcome == Upsert::Added {
        tag_model(conn, model_id, "untagged").map_err(|e| e.to_string())?;
    }

    Ok(outcome)
}

/// Guess where a model came from, from where it sits.
///
/// The scan root names the batch -- "STLs - Patreon 0121" -- and the first
/// folder inside it is usually the creator. Both are guesses and both are
/// editable; a wrong guess costs one correction, whereas an empty field stays
/// empty across nine hundred models.
///
/// Returns (creator, source). Either may be empty when the path says nothing.
fn provenance<'a>(path: &'a Path, root: &'a Path) -> (String, String) {
    // Folder names routinely carry stray whitespace -- "Archvillain Games /"
    // is real -- and it would show up in every creator list forever.
    let source = root
        .file_name()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default();

    let creator = path
        .strip_prefix(root)
        .ok()
        .and_then(|rel| {
            let parts: Vec<_> = rel.components().collect();
            // Needs at least one directory between the root and the file;
            // a loose file at the root tells us nothing about its author.
            if parts.len() >= 2 {
                parts
                    .first()
                    .map(|c| c.as_os_str().to_string_lossy().trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    (creator, source)
}

/// SHA-256 over a 64 KB window.
///
/// The previous implementation called `fs::read`, pulling the entire file into
/// memory. Multi-hundred-megabyte STLs are routine, and a library scan would
/// hold one of them at a time for no reason.
fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stl_extension_matching_is_case_insensitive() {
        assert!(is_stl(Path::new("/a/b/Benchy.stl")));
        assert!(is_stl(Path::new("/a/b/Benchy.STL")));
        assert!(is_stl(Path::new("/a/b/Benchy.Stl")));
        assert!(!is_stl(Path::new("/a/b/Benchy.3mf")));
        assert!(!is_stl(Path::new("/a/b/stl")));
        assert!(!is_stl(Path::new("/a/b/notes.txt")));
    }

    #[test]
    fn hash_is_stable_and_content_dependent() {
        let dir = std::env::temp_dir();
        let a = dir.join(format!("stlib_hash_a_{}", std::process::id()));
        let b = dir.join(format!("stlib_hash_b_{}", std::process::id()));
        std::fs::write(&a, b"hello world").unwrap();
        std::fs::write(&b, b"hello worlds").unwrap();
        let ha = hash_file(&a).unwrap();
        assert_eq!(ha, hash_file(&a).unwrap());
        assert_ne!(ha, hash_file(&b).unwrap());
        // Known SHA-256 of "hello world" -- proves the streaming loop actually
        // feeds every byte, in order, exactly once.
        assert_eq!(
            ha,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        std::fs::remove_file(a).ok();
        std::fs::remove_file(b).ok();
    }

    #[test]
    fn hashing_spans_multiple_buffer_windows() {
        // Larger than the 64 KB read window, so a loop that dropped or
        // double-counted a chunk would show up here.
        let dir = std::env::temp_dir();
        let p = dir.join(format!("stlib_hash_big_{}", std::process::id()));
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        let mut expected = Sha256::new();
        expected.update(&data);
        assert_eq!(hash_file(&p).unwrap(), format!("{:x}", expected.finalize()));
        std::fs::remove_file(p).ok();
    }
}
