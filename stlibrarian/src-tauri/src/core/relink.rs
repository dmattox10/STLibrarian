//! Finding files the library has lost track of.
//!
//! Every row stores an absolute path, which is fine until the day it is not:
//! a library moved to a bigger disk, a NAS share remounted somewhere else, or
//! a database restored from backup onto a machine that keeps its models in a
//! different place. On that day every path is wrong at once and the library
//! looks empty even though nothing was lost.
//!
//! Relinking matches by *content*, so it does not care what anything is called
//! or where it sits now. It only hashes files whose size already matches
//! something missing, which is what keeps it from re-reading a whole library
//! to find a handful of strays.

use crate::core::{db, scanner, sliced, stl};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelinkMatch {
    pub model_id: i64,
    pub name: String,
    /// Where the library last saw it.
    pub was: String,
    /// Where it is now.
    pub found: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelinkReport {
    pub relinked: usize,
    pub still_missing: usize,
    /// Files opened and hashed. Everything else was ruled out on size alone.
    pub examined: usize,
}

/// What relinking would reconnect, without touching the database.
pub fn plan(conn: &Connection, root: &Path) -> Result<Vec<RelinkMatch>, String> {
    let missing = db::missing_models(conn).map_err(|e| e.to_string())?;
    if missing.is_empty() {
        return Ok(Vec::new());
    }

    // Size is a free filter: a candidate whose length differs cannot be the
    // same bytes, so it is never opened.
    let mut by_size: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, m) in missing.iter().enumerate() {
        by_size.entry(m.size).or_default().push(i);
    }

    let mut taken = vec![false; missing.len()];
    let mut out = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !stl::is_supported_mesh(path) && !sliced::is_sliced(path) {
            continue;
        }
        let len = match entry.metadata() {
            Ok(m) => m.len() as i64,
            Err(_) => continue,
        };
        let candidates = match by_size.get(&len) {
            Some(c) if c.iter().any(|&i| !taken[i]) => c.clone(),
            _ => continue,
        };

        let hash = match scanner::hash_file(path) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let here = path.to_string_lossy().to_string();
        let file_name = path.file_name().map(|s| s.to_string_lossy().to_string());

        // Several missing models can share a hash -- a library that held the
        // same model twice. Prefer the one that also had this filename, so the
        // two copies land back where they each were.
        let mut best: Option<usize> = None;
        for &i in &candidates {
            if taken[i] || missing[i].hash != hash {
                continue;
            }
            let same_name = Path::new(&missing[i].path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                == file_name;
            if same_name {
                best = Some(i);
                break;
            }
            if best.is_none() {
                best = Some(i);
            }
        }

        if let Some(i) = best {
            taken[i] = true;
            out.push(RelinkMatch {
                model_id: missing[i].id,
                name: missing[i].name.clone(),
                was: missing[i].path.clone(),
                found: here,
            });
        }
    }

    Ok(out)
}

/// Reconnect everything `plan` can find.
///
/// The plan is recomputed here rather than accepted as an argument, for the
/// same reason the organiser recomputes its own: a list of paths arriving from
/// a client is a list of arbitrary paths.
pub fn apply(conn: &Connection, root: &Path) -> Result<RelinkReport, String> {
    let matches = plan(conn, root)?;
    let mut report = RelinkReport {
        examined: matches.len(),
        ..Default::default()
    };

    for m in matches {
        // Only the location changed -- the bytes are identical by definition,
        // since that is how the match was made.
        if db::relocate_model(conn, m.model_id, &m.found).is_ok() {
            report.relinked += 1;
        }
    }
    report.still_missing = db::find_missing(conn).map(|v| v.len()).unwrap_or(0);
    Ok(report)
}

/// Point one model at one file, by hand.
///
/// For what searching cannot reach: a model re-exported at a different
/// resolution, a mesh someone repaired, a file renamed and edited. The row is
/// re-measured from the new file rather than assuming it is the old one.
pub fn repoint(conn: &Connection, model_id: i64, new_path: &str) -> Result<(), String> {
    db::remeasure_model(conn, model_id, new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    /// A real directory with real files: relinking is about the filesystem, so
    /// a fake one would test nothing.
    struct Bench {
        conn: Connection,
        dir: std::path::PathBuf,
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn cube_bytes(side: f32) -> Vec<u8> {
        let s = side;
        let p = [
            [0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0],
            [0.0, 0.0, s], [s, 0.0, s], [s, s, s], [0.0, s, s],
        ];
        let faces = [
            (0, 1, 2), (0, 2, 3), (4, 6, 5), (4, 7, 6), (0, 5, 1), (0, 4, 5),
            (1, 6, 2), (1, 5, 6), (2, 7, 3), (2, 6, 7), (3, 4, 0), (3, 7, 4),
        ];
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(faces.len() as u32).to_le_bytes());
        for (a, b, c) in faces {
            out.extend_from_slice(&[0u8; 12]);
            for v in [p[a], p[b], p[c]] {
                for coord in v {
                    out.extend_from_slice(&coord.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    fn bench() -> Bench {
        let dir = std::env::temp_dir().join(format!(
            "stlib_relink_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(dir.join("library")).unwrap();
        let conn = db::open_db(&dir.join("stl.db")).unwrap();
        Bench { conn, dir }
    }

    fn write(b: &Bench, rel: &str, side: f32) -> std::path::PathBuf {
        let p = b.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, cube_bytes(side)).unwrap();
        p
    }

    fn scan(b: &Bench, rel: &str) {
        scanner::scan_folder(&b.conn, &b.dir.join(rel).to_string_lossy());
    }

    #[test]
    fn a_file_that_moved_is_found_again() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        assert_eq!(db::count_models(&b.conn).unwrap(), 1);
        assert!(db::find_missing(&b.conn).unwrap().is_empty());

        std::fs::create_dir_all(b.dir.join("elsewhere")).unwrap();
        std::fs::rename(b.dir.join("library/knight.stl"), b.dir.join("elsewhere/knight.stl")).unwrap();
        assert_eq!(db::find_missing(&b.conn).unwrap().len(), 1, "it should now be lost");

        let found = plan(&b.conn, &b.dir.join("elsewhere")).unwrap();
        assert_eq!(found.len(), 1);
        // Compared as a path, not as text: on Windows the separator is a
        // backslash and a string comparison against "elsewhere/knight.stl"
        // never matches. Path::ends_with matches whole components, and
        // takes "/" in the pattern on either platform.
        assert!(std::path::Path::new(&found[0].found).ends_with("elsewhere/knight.stl"));

        let report = apply(&b.conn, &b.dir.join("elsewhere")).unwrap();
        assert_eq!(report.relinked, 1);
        assert_eq!(report.still_missing, 0);
    }

    /// The restore case: the database comes back but the models live somewhere
    /// else entirely, so every path is wrong at once.
    #[test]
    fn a_whole_library_that_moved_is_reconnected() {
        let b = bench();
        for (name, side) in [("a.stl", 10.0), ("b.stl", 14.0), ("deep/c.stl", 18.0)] {
            write(&b, &format!("library/{}", name), side);
        }
        scan(&b, "library");
        assert_eq!(db::count_models(&b.conn).unwrap(), 3);

        std::fs::rename(b.dir.join("library"), b.dir.join("restored")).unwrap();
        assert_eq!(db::find_missing(&b.conn).unwrap().len(), 3);

        let report = apply(&b.conn, &b.dir.join("restored")).unwrap();
        assert_eq!(report.relinked, 3);
        assert_eq!(report.still_missing, 0);
        // And nothing was duplicated in the process.
        assert_eq!(db::count_models(&b.conn).unwrap(), 3);
    }

    /// Matching is by content, so a rename is no obstacle.
    #[test]
    fn a_renamed_file_is_still_recognised() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        std::fs::rename(b.dir.join("library/knight.stl"), b.dir.join("library/totally-different.stl")).unwrap();

        let report = apply(&b.conn, &b.dir.join("library")).unwrap();
        assert_eq!(report.relinked, 1);
        let m = db::list_models(&b.conn, &Default::default()).unwrap();
        assert!(m[0].path.ends_with("totally-different.stl"));
    }

    /// Two copies of one model must land back on their own files rather than
    /// both claiming whichever turns up first.
    #[test]
    fn duplicate_models_are_matched_to_the_right_copies() {
        let b = bench();
        write(&b, "library/one/dragon.stl", 12.0);
        write(&b, "library/two/dragon.stl", 12.0);
        scan(&b, "library");
        assert_eq!(db::count_models(&b.conn).unwrap(), 2, "same bytes, two rows");

        std::fs::rename(b.dir.join("library"), b.dir.join("moved")).unwrap();
        let report = apply(&b.conn, &b.dir.join("moved")).unwrap();
        assert_eq!(report.relinked, 2);
        assert_eq!(report.still_missing, 0);

        let paths: Vec<String> = db::list_models(&b.conn, &Default::default())
            .unwrap()
            .into_iter()
            .map(|m| m.path)
            .collect();
        // Each row got its own file, not the same one twice.
        assert_eq!(paths.len(), 2);
        assert_ne!(paths[0], paths[1]);
    }

    #[test]
    fn a_file_that_is_really_gone_stays_missing() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        std::fs::remove_file(b.dir.join("library/knight.stl")).unwrap();

        let report = apply(&b.conn, &b.dir.join("library")).unwrap();
        assert_eq!(report.relinked, 0);
        assert_eq!(report.still_missing, 1, "reported, not quietly deleted");
        assert_eq!(db::count_models(&b.conn).unwrap(), 1, "the row survives");
    }

    #[test]
    fn planning_changes_nothing() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        std::fs::create_dir_all(b.dir.join("elsewhere")).unwrap();
        std::fs::rename(b.dir.join("library/knight.stl"), b.dir.join("elsewhere/knight.stl")).unwrap();

        let before = db::list_models(&b.conn, &Default::default()).unwrap()[0].path.clone();
        plan(&b.conn, &b.dir.join("elsewhere")).unwrap();
        let after = db::list_models(&b.conn, &Default::default()).unwrap()[0].path.clone();
        assert_eq!(before, after, "a plan is a look, not a change");
    }

    /// The manual escape hatch, for a file searching cannot match: a re-export,
    /// a repaired mesh. The row is re-measured rather than assumed.
    #[test]
    fn pointing_a_model_at_a_different_file_re_measures_it() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        let id = db::list_models(&b.conn, &Default::default()).unwrap()[0].id;
        let before = db::get_model(&b.conn, id).unwrap().unwrap();

        // A bigger cube: same shape, different everything.
        let replacement = write(&b, "spare/knight-v2.stl", 30.0);
        repoint(&b.conn, id, &replacement.to_string_lossy()).unwrap();

        let after = db::get_model(&b.conn, id).unwrap().unwrap();
        assert!(after.path.ends_with("knight-v2.stl"));
        assert_ne!(after.hash, before.hash, "it is a different file");
        assert!(after.dim_x > before.dim_x, "and it was measured, not copied");
        assert_eq!(db::count_models(&b.conn).unwrap(), 1, "still one model");
    }

    /// Two rows cannot share one file, and being told why beats a constraint
    /// error from the database.
    #[test]
    fn pointing_at_a_file_another_model_already_uses_is_refused() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        write(&b, "library/ogre.stl", 22.0);
        scan(&b, "library");
        let models = db::list_models(&b.conn, &Default::default()).unwrap();
        let knight = models.iter().find(|m| m.name == "knight").unwrap();
        let ogre = models.iter().find(|m| m.name == "ogre").unwrap();

        let err = repoint(&b.conn, knight.id, &ogre.path).unwrap_err();
        assert!(err.contains("ogre"), "should name the model in the way: {}", err);
        // Neither row moved.
        assert_eq!(db::get_model(&b.conn, knight.id).unwrap().unwrap().path, knight.path);
        assert_eq!(db::get_model(&b.conn, ogre.id).unwrap().unwrap().path, ogre.path);
    }

    #[test]
    fn pointing_at_nothing_is_refused() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        let id = db::list_models(&b.conn, &Default::default()).unwrap()[0].id;
        assert!(repoint(&b.conn, id, "/no/such/file.stl").is_err());
        // The row still points where it did.
        assert!(db::get_model(&b.conn, id).unwrap().unwrap().path.ends_with("knight.stl"));
    }

    #[test]
    fn relinking_a_library_with_nothing_missing_is_a_no_op() {
        let b = bench();
        write(&b, "library/knight.stl", 10.0);
        scan(&b, "library");
        let report = apply(&b.conn, &b.dir.join("library")).unwrap();
        assert_eq!(report.relinked, 0);
        assert_eq!(report.still_missing, 0);
    }
}
