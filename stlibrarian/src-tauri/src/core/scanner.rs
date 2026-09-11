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
        if !entry.file_type().is_file() {
            continue;
        }
        if is_image(entry.path()) {
            // Indexed but not counted: a promo render is not a model, it is
            // how a model gets a better thumbnail than the mesh can give it.
            index_image(conn, entry.path());
            continue;
        }
        if !stl::is_supported_mesh(entry.path()) {
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

    // Renders are matched once the models exist, so ordering within the walk
    // does not matter.
    match crate::core::db::match_images(conn) {
        Ok(n) if n > 0 => info!("Matched {} models to creator renders", n),
        Err(e) => warn!("Could not match renders: {}", e),
        _ => {}
    }

    report.elapsed_ms = start.elapsed().as_millis() as u64;
    info!(
        "Scan finished: +{} ~{} ={} !{} in {}ms",
        report.added, report.updated, report.unchanged, report.failed, report.elapsed_ms
    );
    on_progress(&report);
    report
}

/// Which of a creator's two copies this is.
///
/// Packs ship the same model twice -- presupported for resin, bare for anyone
/// who wants to place their own -- and printing the wrong one wastes an
/// evening. The nearest name wins, so a file inside `Unsupported/` is
/// unsupported even if the pack folder above it says otherwise.
pub(crate) fn detect_variant(path: &Path) -> String {
    let mut names: Vec<String> = Vec::new();
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        names.push(stem.to_string());
    }
    for ancestor in path.ancestors().skip(1) {
        if let Some(n) = ancestor.file_name().and_then(|s| s.to_str()) {
            names.push(n.to_string());
        }
    }

    for name in names {
        let flat: String = name
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        // "unsupported" contains "supported", so it has to be tested first or
        // every unsupported file is labelled the opposite of what it is.
        if flat.contains("unsupported") {
            return "unsupported".to_string();
        }
        if flat.contains("presupported") || flat.contains("supported") {
            return "supported".to_string();
        }
    }
    String::new()
}

/// Formats a creator is likely to ship a render in.
fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            e == "jpg" || e == "jpeg" || e == "png" || e == "webp"
        })
        .unwrap_or(false)
}

fn index_image(conn: &Connection, path: &Path) {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return,
    };
    // Skip anything too small to be a promo shot -- icons, watermarks, the
    // creator's logo repeated in every folder.
    if meta.len() < 8 * 1024 {
        return;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Err(e) = crate::core::db::upsert_image(
        conn,
        &path.to_string_lossy(),
        &name,
        meta.len() as i64,
        mtime,
    ) {
        warn!("Could not index image {}: {}", path.display(), e);
    }
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
        &detect_variant(path),
        geometry.watertight,
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
    fn mesh_extension_matching_is_case_insensitive() {
        for good in [
            "/a/Benchy.stl",
            "/a/Benchy.STL",
            "/a/Benchy.Stl",
            "/a/Benchy.obj",
            "/a/B.OBJ",
            "/a/Benchy.3mf",
            "/a/B.3MF",
        ] {
            assert!(stl::is_supported_mesh(Path::new(good)), "{}", good);
        }
        // An extension is not a filename, and neither is a readme.
        for bad in ["/a/stl", "/a/3mf", "/a/notes.txt", "/a/model.gcode"] {
            assert!(!stl::is_supported_mesh(Path::new(bad)), "{}", bad);
        }
    }

    #[test]
    fn the_nearest_folder_decides_the_variant() {
        assert_eq!(detect_variant(Path::new("/l/Pack/Unsupported/a.stl")), "unsupported");
        assert_eq!(detect_variant(Path::new("/l/Pack/Presupported/a.stl")), "supported");
        assert_eq!(detect_variant(Path::new("/l/Pack/Supported/a.stl")), "supported");
        assert_eq!(detect_variant(Path::new("/l/Throk_130mm_Unsupported/a.stl")), "unsupported");
        assert_eq!(detect_variant(Path::new("/l/Pack/a_presupported.stl")), "supported");
        assert_eq!(detect_variant(Path::new("/l/Pack/plain.stl")), "");
    }

    /// The trap: "unsupported" ends with "supported".
    #[test]
    fn unsupported_is_never_mistaken_for_supported() {
        assert_eq!(detect_variant(Path::new("/l/Unsupported/x.stl")), "unsupported");
        // And the closest name wins over one further up.
        assert_eq!(
            detect_variant(Path::new("/l/Supported Pack/Unsupported/x.stl")),
            "unsupported"
        );
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
