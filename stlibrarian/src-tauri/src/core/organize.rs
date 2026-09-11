//! Rearranging files on disk. The Pro server only.
//!
//! Everything else in this library reads. This is the one module that writes
//! to somebody's collection, which is why it is shaped the way it is:
//!
//!   * It plans first. A plan reads nothing but the database and touches no
//!     file, so you can look at every move before any of them happens.
//!   * Apply recomputes the plan rather than executing one handed to it. A
//!     plan that arrived over HTTP is a list of arbitrary source and
//!     destination paths, and honouring it would turn this into a
//!     move-any-file-anywhere endpoint.
//!   * Destinations are built from sanitised components joined to the library
//!     root, so a target cannot escape the root by construction rather than by
//!     a check someone might later remove.
//!   * Nothing is ever overwritten. A collision is reported, not resolved.
//!
//! It lives on the server because that is where the files are on a NAS, and
//! because building it once beats building it on every desktop platform.

use crate::core::db;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MoveAction {
    /// Will be moved.
    Move,
    /// Already exactly where it belongs.
    InPlace,
    /// Something else is already there; left alone.
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedMove {
    pub model_id: i64,
    pub from: String,
    pub to: String,
    pub action: MoveAction,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrganizeReport {
    pub moved: usize,
    pub in_place: usize,
    pub conflicts: usize,
    pub failed: usize,
}

/// Make one path segment safe to join to the library root.
///
/// Separators become dashes and dot-only names are replaced outright, so the
/// result can never climb out of the directory it is joined to. That is the
/// property the whole module leans on.
fn safe_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim();
    // "." and ".." carry no other characters, so this catches both without
    // mangling a name that merely contains a dot.
    let safe = if trimmed.chars().all(|c| c == '.') {
        ""
    } else {
        trimmed
    };

    let limited: String = safe.chars().take(80).collect();
    let limited = limited.trim().to_string();
    if limited.is_empty() {
        "Unsorted".to_string()
    } else {
        limited
    }
}

/// Where a model belongs: root / creator / model / variant / file
fn destination(root: &Path, model: &crate::core::models::ModelRecord, package: Option<&str>) -> PathBuf {
    let file_name = Path::new(&model.path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}.stl", model.name));

    let mut out = root.to_path_buf();
    out.push(safe_component(if model.creator.trim().is_empty() {
        "Unsorted"
    } else {
        &model.creator
    }));
    // A package is several files that are one model, so they share a folder.
    // A lone file gets a folder of its own, which is what makes a library of
    // things rather than a directory of files.
    out.push(safe_component(package.unwrap_or(&model.name)));
    if !model.variant.trim().is_empty() {
        out.push(safe_component(&model.variant));
    }
    out.push(safe_component(&file_name));
    out
}

/// What `apply` would do. Reads only.
pub fn plan(conn: &Connection, root: &Path) -> Result<Vec<PlannedMove>, String> {
    let models = db::list_models(
        conn,
        &crate::core::models::LibraryQuery {
            limit: Some(5000),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;

    let packages = db::list_packages(conn).map_err(|e| e.to_string())?;
    let package_name = |id: Option<i64>| -> Option<String> {
        id.and_then(|id| packages.iter().find(|p| p.id == id).map(|p| p.name.clone()))
    };

    let mut out = Vec::new();
    for model in models {
        let name = package_name(model.package_id);
        let to = destination(root, &model, name.as_deref());
        let from = PathBuf::from(&model.path);

        let action = if from == to {
            MoveAction::InPlace
        } else if to.exists() {
            MoveAction::Conflict
        } else {
            MoveAction::Move
        };

        out.push(PlannedMove {
            model_id: model.id,
            from: model.path.clone(),
            to: to.to_string_lossy().to_string(),
            action,
        });
    }
    Ok(out)
}

/// Carry out the plan.
///
/// Recomputed here rather than taken as an argument -- see the module note.
pub fn apply(conn: &Connection, root: &Path) -> Result<OrganizeReport, String> {
    let mut report = OrganizeReport::default();

    for step in plan(conn, root)? {
        match step.action {
            MoveAction::InPlace => {
                report.in_place += 1;
                continue;
            }
            MoveAction::Conflict => {
                report.conflicts += 1;
                continue;
            }
            MoveAction::Move => {}
        }

        let to = PathBuf::from(&step.to);
        let from = PathBuf::from(&step.from);

        // Re-checked immediately before the move: the plan was computed a
        // moment ago and something may have appeared since.
        if to.exists() {
            report.conflicts += 1;
            continue;
        }
        if let Some(parent) = to.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                report.failed += 1;
                continue;
            }
        }

        // rename is atomic within a filesystem and fails across them, which is
        // ordinary on a NAS where the library spans volumes.
        let moved = std::fs::rename(&from, &to).is_ok()
            || (std::fs::copy(&from, &to).is_ok() && std::fs::remove_file(&from).is_ok());

        if !moved {
            report.failed += 1;
            continue;
        }

        if db::relocate_model(conn, step.model_id, &step.to).is_err() {
            // The file moved but the row did not, which would leave the
            // library pointing at nothing. Put it back.
            std::fs::rename(&to, &from).ok();
            report.failed += 1;
            continue;
        }
        report.moved += 1;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_and_dots_cannot_escape_a_folder() {
        assert_eq!(safe_component(".."), "Unsorted");
        assert_eq!(safe_component("."), "Unsorted");
        assert_eq!(safe_component("   "), "Unsorted");
        assert_eq!(safe_component(""), "Unsorted");
        // Still one segment: the separators are gone, so it names a folder
        // called "..-..-etc" rather than climbing two levels.
        assert_eq!(safe_component("../../etc"), "..-..-etc");
        assert_eq!(safe_component("a/b"), "a-b");
        assert_eq!(safe_component("C:\\Windows"), "C--Windows");
        // A dot inside a real name is left alone.
        assert_eq!(safe_component("Archvillain Games v1.2"), "Archvillain Games v1.2");
    }

    #[test]
    fn a_destination_always_stays_under_the_root() {
        let root = Path::new("/library");
        for hostile in ["../../etc", "..", "/absolute", "a/../../b"] {
            let model = crate::core::models::ModelRecord {
                id: 1,
                path: format!("/elsewhere/{}.stl", hostile),
                name: hostile.to_string(),
                size: 1,
                hash: "h".into(),
                triangles: 1,
                dim_x: 1.0,
                dim_y: 1.0,
                dim_z: 1.0,
                has_thumb: false,
                volume: 0.0,
                creator: hostile.to_string(),
                source: String::new(),
                package_id: None,
                render: None,
                print_count: 0,
                last_printed: None,
                variant: hostile.to_string(),
                mtime: 0,
                tags: vec![],
                collections: vec![],
            };
            let dest = destination(root, &model, Some(hostile));
            assert!(
                dest.starts_with(root),
                "{:?} escaped the root for input {:?}",
                dest,
                hostile
            );
            assert!(
                !dest.components().any(|c| c.as_os_str() == ".."),
                "{:?} contains a parent hop",
                dest
            );
        }
    }

    #[test]
    fn a_package_puts_its_parts_in_one_folder() {
        let root = Path::new("/library");
        let mut model = crate::core::models::ModelRecord {
            id: 1,
            path: "/old/Throk_Body.stl".into(),
            name: "Throk_Body".into(),
            size: 1,
            hash: "h".into(),
            triangles: 1,
            dim_x: 1.0,
            dim_y: 1.0,
            dim_z: 1.0,
            has_thumb: false,
            volume: 0.0,
            creator: "Archvillain Games".into(),
            source: String::new(),
            package_id: Some(7),
            render: None,
            print_count: 0,
            last_printed: None,
            variant: "unsupported".into(),
            mtime: 0,
            tags: vec![],
            collections: vec![],
        };
        let a = destination(root, &model, Some("Throk 130mm"));
        model.name = "Throk_ARM".into();
        model.path = "/old/Throk_ARM.stl".into();
        let b = destination(root, &model, Some("Throk 130mm"));

        assert_eq!(a.parent(), b.parent(), "parts of one model must land together");
        assert_eq!(
            a,
            Path::new("/library/Archvillain Games/Throk 130mm/unsupported/Throk_Body.stl")
        );
    }

    #[test]
    fn a_model_with_no_creator_is_filed_under_unsorted() {
        let root = Path::new("/library");
        let model = crate::core::models::ModelRecord {
            id: 1,
            path: "/old/x.stl".into(),
            name: "x".into(),
            size: 1,
            hash: "h".into(),
            triangles: 1,
            dim_x: 1.0,
            dim_y: 1.0,
            dim_z: 1.0,
            has_thumb: false,
            volume: 0.0,
            creator: "   ".into(),
            source: String::new(),
            package_id: None,
            render: None,
            print_count: 0,
            last_printed: None,
            variant: String::new(),
            mtime: 0,
            tags: vec![],
            collections: vec![],
        };
        assert_eq!(
            destination(root, &model, None),
            Path::new("/library/Unsorted/x/x.stl")
        );
    }
}
