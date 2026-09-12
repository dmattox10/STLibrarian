use crate::core::db;
use crate::core::models::{
    Collection, DuplicateGroup, LibraryQuery, LibraryStats, ModelRecord, NewPrint, Package,
    LibraryLocation, PackageSuggestion, PrintRecord, ScanReport, Slice, TagCount,
};
use crate::core::paths;
use crate::core::scanner;
use crate::core::thumbs;
use tauri::{command, Emitter};
use tauri_plugin_dialog::DialogExt;

fn conn() -> Result<rusqlite::Connection, String> {
    db::open_default().map_err(|e| format!("Could not open library: {}", e))
}

#[command]
fn library(query: Option<LibraryQuery>) -> Result<Vec<ModelRecord>, String> {
    let c = conn()?;
    db::list_models(&c, &query.unwrap_or_default()).map_err(|e| e.to_string())
}

#[command]
fn model(id: i64) -> Result<Option<ModelRecord>, String> {
    let c = conn()?;
    db::get_model(&c, id).map_err(|e| e.to_string())
}

#[command]
fn tags() -> Result<Vec<TagCount>, String> {
    let c = conn()?;
    db::list_tags(&c).map_err(|e| e.to_string())
}

#[command]
fn stats() -> Result<LibraryStats, String> {
    let c = conn()?;
    let mut s = db::library_stats(&c).map_err(|e| e.to_string())?;
    s.missing = db::find_missing(&c).map(|m| m.len() as i64).unwrap_or(0);
    Ok(s)
}

#[command]
async fn scan(app: tauri::AppHandle, path: String) -> Result<ScanReport, String> {
    // A library scan is minutes of blocking I/O. Running it on the async
    // runtime's worker would freeze every other command, the window included.
    tauri::async_runtime::spawn_blocking(move || {
        let c = conn()?;
        Ok(scanner::scan_folder_with(&c, &path, |progress| {
            app.emit("scan:progress", progress).ok();
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[command]
fn set_tags(model_id: i64, tags: Vec<String>) -> Result<(), String> {
    let c = conn()?;
    db::set_tags(&c, model_id, &tags).map_err(|e| e.to_string())
}

#[command]
fn delete_model(model_id: i64) -> Result<(), String> {
    let c = conn()?;
    db::delete_model(&c, model_id).map_err(|e| e.to_string())
}

// --------------------------------------------------------- collections ----

#[command]
fn collections() -> Result<Vec<Collection>, String> {
    let c = conn()?;
    db::list_collections(&c, None).map_err(|e| e.to_string())
}

#[command]
fn create_collection(name: String) -> Result<Collection, String> {
    let c = conn()?;
    db::create_collection(&c, &name, None)
}

#[command]
fn rename_collection(id: i64, name: String) -> Result<(), String> {
    let c = conn()?;
    db::rename_collection(&c, id, &name)
}

#[command]
fn delete_collection(id: i64) -> Result<(), String> {
    let c = conn()?;
    db::delete_collection(&c, id).map_err(|e| e.to_string())
}

#[command]
fn add_to_collection(collection_id: i64, model_ids: Vec<i64>) -> Result<usize, String> {
    let c = conn()?;
    db::add_to_collection(&c, collection_id, &model_ids).map_err(|e| e.to_string())
}

#[command]
fn remove_from_collection(collection_id: i64, model_ids: Vec<i64>) -> Result<usize, String> {
    let c = conn()?;
    db::remove_from_collection(&c, collection_id, &model_ids).map_err(|e| e.to_string())
}

// ------------------------------------------------------------ packages ----

#[command]
fn packages() -> Result<Vec<Package>, String> {
    let c = conn()?;
    db::list_packages(&c).map_err(|e| e.to_string())
}

#[command]
fn package_suggestions() -> Result<Vec<PackageSuggestion>, String> {
    let c = conn()?;
    db::suggest_packages(&c).map_err(|e| e.to_string())
}

#[command]
fn create_package(name: String, root: String, model_ids: Vec<i64>) -> Result<Package, String> {
    let c = conn()?;
    let pkg = db::create_package(&c, &name, &root)?;
    db::assign_to_package(&c, pkg.id, &model_ids).map_err(|e| e.to_string())?;
    Ok(pkg)
}

#[command]
fn rename_package(id: i64, name: String) -> Result<(), String> {
    let c = conn()?;
    db::rename_package(&c, id, &name)
}

#[command]
fn delete_package(id: i64) -> Result<(), String> {
    let c = conn()?;
    db::delete_package(&c, id).map_err(|e| e.to_string())
}

#[command]
fn assign_to_package(package_id: i64, model_ids: Vec<i64>) -> Result<usize, String> {
    let c = conn()?;
    db::assign_to_package(&c, package_id, &model_ids).map_err(|e| e.to_string())
}

#[command]
fn unassign_from_package(model_ids: Vec<i64>) -> Result<usize, String> {
    let c = conn()?;
    db::unassign_from_package(&c, &model_ids).map_err(|e| e.to_string())
}

/// Version and platform, so feedback arrives with enough context to act on.
#[command]
fn app_info() -> (String, String) {
    (
        env!("CARGO_PKG_VERSION").to_string(),
        std::env::consts::OS.to_string(),
    )
}

// -------------------------------------------------------- lost and found ----

/// Where the library lives, so it can be backed up.
#[command]
fn library_location() -> LibraryLocation {
    paths::location()
}

#[command]
fn reveal_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(paths::data_dir().to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("Could not open it: {}", e))
}

#[command]
fn missing_models() -> Result<Vec<ModelRecord>, String> {
    let c = conn()?;
    db::missing_models(&c).map_err(|e| e.to_string())
}

#[command]
async fn relink_plan(root: String) -> Result<Vec<crate::core::relink::RelinkMatch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = conn()?;
        crate::core::relink::plan(&c, std::path::Path::new(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[command]
async fn relink_apply(root: String) -> Result<crate::core::relink::RelinkReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = conn()?;
        crate::core::relink::apply(&c, std::path::Path::new(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[command]
fn repoint_model(model_id: i64, path: String) -> Result<(), String> {
    let c = conn()?;
    crate::core::relink::repoint(&c, model_id, &path)
}

/// A file picker for the manual fix, when searching cannot find it.
#[command]
async fn pick_file(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Models", &["stl", "obj", "3mf", "STL", "OBJ", "3MF"])
            .blocking_pick_file()
            .map(|p| p.to_string())
    })
    .await
    .ok()
    .flatten()
}

// ---------------------------------------------------------------- slices ----

#[command]
fn slices(model_id: i64) -> Result<Vec<Slice>, String> {
    let c = conn()?;
    db::slices_for_model(&c, model_id).map_err(|e| e.to_string())
}

#[command]
fn orphan_slices() -> Result<Vec<Slice>, String> {
    let c = conn()?;
    db::unattached_slices(&c).map_err(|e| e.to_string())
}

#[command]
fn set_slice_owner(
    slice_id: i64,
    model_id: Option<i64>,
    package_id: Option<i64>,
) -> Result<(), String> {
    let c = conn()?;
    db::set_slice_owner(&c, slice_id, model_id, package_id).map_err(|e| e.to_string())
}

/// Hand a sliced file to whatever opens it -- Lychee, Chitubox, a gcode viewer.
#[command]
fn open_slice(app: tauri::AppHandle, slice_id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let c = conn()?;
    let path: String = c
        .query_row(
            "SELECT path FROM slices WHERE id = ?1",
            rusqlite::params![slice_id],
            |r| r.get(0),
        )
        .map_err(|_| "No such sliced file".to_string())?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Could not open it: {}", e))
}

// ---------------------------------------------------------------- prints ----

#[command]
fn prints(model_id: i64) -> Result<Vec<PrintRecord>, String> {
    let c = conn()?;
    db::list_prints(&c, model_id).map_err(|e| e.to_string())
}

#[command]
fn log_print(model_id: i64, entry: NewPrint) -> Result<i64, String> {
    let c = conn()?;
    db::add_print(&c, model_id, &entry, None)
}

#[command]
fn delete_print(print_id: i64) -> Result<(), String> {
    let c = conn()?;
    db::delete_print(&c, print_id).map_err(|e| e.to_string())
}

// --------------------------------------------------------------- renders ----

#[command]
fn model_renders(model_id: i64) -> Result<Vec<(i64, String)>, String> {
    let c = conn()?;
    db::images_near_model(&c, model_id).map_err(|e| e.to_string())
}

/// Choose a render, or pass null to go back to the generated preview.
#[command]
fn set_model_render(model_id: i64, image_id: Option<i64>) -> Result<(), String> {
    let c = conn()?;
    let model = db::get_model(&c, model_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No such model".to_string())?;
    db::set_model_image(&c, model_id, image_id).map_err(|e| e.to_string())?;
    // The cached thumbnail is keyed by content hash and the content has not
    // changed, so nothing else would invalidate it.
    thumbs::forget(&model.hash);
    Ok(())
}

// ---------------------------------------------------------- duplicates ----

#[command]
fn duplicates() -> Result<Vec<DuplicateGroup>, String> {
    let c = conn()?;
    db::duplicate_groups(&c).map_err(|e| e.to_string())
}

// ---------------------------------------------------------- provenance ----

#[command]
fn set_provenance(model_id: i64, creator: String, source: String) -> Result<(), String> {
    let c = conn()?;
    db::set_provenance(&c, model_id, &creator, &source).map_err(|e| e.to_string())
}

#[command]
fn creators() -> Result<Vec<TagCount>, String> {
    let c = conn()?;
    db::list_creators(&c).map_err(|e| e.to_string())
}

// ----------------------------------------------------------- bulk tags ----

#[command]
fn bulk_add_tag(model_ids: Vec<i64>, tag: String) -> Result<usize, String> {
    let c = conn()?;
    db::add_tag_to_models(&c, &model_ids, &tag).map_err(|e| e.to_string())
}

#[command]
fn bulk_remove_tag(model_ids: Vec<i64>, tag: String) -> Result<usize, String> {
    let c = conn()?;
    db::remove_tag_from_models(&c, &model_ids, &tag).map_err(|e| e.to_string())
}

#[command]
async fn get_thumb(hash: String) -> Option<String> {
    // Rendered in Rust on first request and cached by content hash. The
    // webview no longer draws previews at all.
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(cached) = thumbs::load_data_url(&hash) {
            return Some(cached);
        }
        let conn = db::open_default().ok()?;
        let path = db::path_for_hash(&conn, &hash).ok().flatten()?;
        let render = db::image_for_hash(&conn, &hash).ok().flatten();
        thumbs::ensure(
            &hash,
            std::path::Path::new(&path),
            render.as_deref().map(std::path::Path::new),
        )
        .ok()?;
        thumbs::load_data_url(&hash)
    })
    .await
    .ok()
    .flatten()
}

#[command]
fn put_thumb(hash: String, data_url: String) -> Result<(), String> {
    thumbs::save(&hash, &data_url)?;
    let c = conn()?;
    db::set_has_thumb(&c, &hash, true).map_err(|e| e.to_string())
}

/// Hand the viewer the raw STL bytes.
///
/// Addressed by database id, never by a path from the webview. The renderer can
/// therefore only ever read files the scanner already indexed -- it cannot ask
/// for `~/.ssh/id_rsa`.
#[command]
fn read_model(id: i64) -> Result<tauri::ipc::Response, String> {
    let c = conn()?;
    let record = db::get_model(&c, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No such model".to_string())?;
    let bytes = std::fs::read(&record.path)
        .map_err(|e| format!("Could not read {}: {}", record.path, e))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// A decimated copy of the mesh, capped at `max_triangles`.
///
/// This is what the grid and the viewer actually load. `read_model` still
/// exists for anything that genuinely needs the original bytes, but nothing
/// in the UI should call it -- a 3M triangle model will take the webview
/// down.
#[command]
async fn preview_mesh(id: i64, max_triangles: u32) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = conn()?;
        let record = db::get_model(&c, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No such model".to_string())?;
        let mesh = crate::core::stl::read_preview(std::path::Path::new(&record.path), max_triangles)
            .map_err(|e| format!("Could not read {}: {}", record.path, e))?;
        Ok(tauri::ipc::Response::new(mesh.into_bytes()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Hand the file to whatever the system opens .stl with -- usually a slicer.
#[command]
fn open_in_slicer(app: tauri::AppHandle, model_id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let c = conn()?;
    let model = db::get_model(&c, model_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No such model".to_string())?;
    app.opener()
        .open_path(model.path, None::<&str>)
        .map_err(|e| format!("Could not open it: {}", e))
}

/// Show the file in Finder, for everything the default app cannot do.
#[command]
fn reveal_in_folder(app: tauri::AppHandle, model_id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let c = conn()?;
    let model = db::get_model(&c, model_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No such model".to_string())?;
    app.opener()
        .reveal_item_in_dir(model.path)
        .map_err(|e| format!("Could not reveal it: {}", e))
}

#[command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| p.to_string())
    })
    .await
    .ok()
    .flatten()
}

#[command]
fn prune_thumbs() -> Result<usize, String> {
    let c = conn()?;
    let live = db::all_hashes(&c).map_err(|e| e.to_string())?;
    Ok(thumbs::prune(&live))
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = paths::ensure_dirs() {
        log::error!("Could not create data directory: {}", e);
    }
    log::info!("Library at {}", paths::data_dir().display());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|_app| {
            // Surface a broken library at startup rather than on the first query.
            if let Err(e) = db::open_default() {
                log::error!("Could not open library database: {}", e);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library,
            model,
            tags,
            stats,
            scan,
            set_tags,
            delete_model,
            collections,
            create_collection,
            rename_collection,
            delete_collection,
            add_to_collection,
            remove_from_collection,
            bulk_add_tag,
            bulk_remove_tag,
            packages,
            package_suggestions,
            create_package,
            rename_package,
            delete_package,
            assign_to_package,
            unassign_from_package,
            duplicates,
            model_renders,
            set_model_render,
            prints,
            log_print,
            delete_print,
            open_in_slicer,
            reveal_in_folder,
            slices,
            orphan_slices,
            set_slice_owner,
            open_slice,
            open_remote,
            clear_remote_cache,
            app_info,
            library_location,
            reveal_data_dir,
            missing_models,
            relink_plan,
            relink_apply,
            repoint_model,
            pick_file,
            set_provenance,
            creators,
            get_thumb,
            put_thumb,
            read_model,
            preview_mesh,
            pick_folder,
            prune_thumbs,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}

// ------------------------------------------------------- remote libraries ----

/// Where files fetched from a remote library are kept.
///
/// A cache, not a library: these are copies of somebody else's server and are
/// safe to delete at any time.
fn remote_cache() -> std::path::PathBuf {
    paths::data_dir().join("remote-cache")
}

/// Reduce a filename from a remote server to one harmless path segment.
///
/// It arrives over the network and becomes a path here, so the same rule as
/// the organiser applies: separators and dot-only names cannot survive, and
/// containment is guaranteed by construction rather than by a later check.
fn safe_download_name(raw: &str) -> Option<String> {
    let mapped: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = mapped.trim();
    if trimmed.chars().all(|c| c == '.') {
        return None;
    }
    let limited: String = trimmed.chars().take(120).collect();
    let limited = limited.trim().to_string();
    if limited.is_empty() {
        None
    } else {
        Some(limited)
    }
}

/// Download a file from a remote library into the cache and return its path.
///
/// Resumable. These run to hundreds of megabytes and the machine asking is a
/// workshop laptop on wifi, so an interrupted transfer that starts again from
/// zero is the difference between a slow download and one that never finishes.
///
/// The partial download lives at `<name>.part`, with the `Last-Modified` the
/// server gave for it in `<name>.part-lastmod`. A resume sends that back as
/// `If-Range`: if the model was re-exported while we were away the server
/// sends the whole file instead of the rest of a different one, and the bytes
/// already on disk are replaced rather than spliced onto new ones. Without a
/// stored validator there is no safe resume, so it starts again -- which is
/// the direction to be wrong in.
///
/// The file only appears under its real name once it is complete, so a
/// half-downloaded model can never be handed to a slicer.
fn fetch_to_cache(url: &str, token: Option<&str>, filename: &str) -> Result<std::path::PathBuf, String> {
    fetch_into(&remote_cache(), url, token, filename)
}

/// The above, with the directory named rather than assumed, so it can be
/// pointed at a scratch directory in a test.
fn fetch_into(
    dir: &std::path::Path,
    url: &str,
    token: Option<&str>,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    let safe = safe_download_name(filename).ok_or("That file has no usable name")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let target = dir.join(&safe);
    let part = dir.join(format!("{}.part", safe));
    let stamp = dir.join(format!("{}.part-lastmod", safe));

    // Both, or neither. Bytes without the validator they were fetched against
    // are bytes we cannot prove belong to this version of the file.
    let resume = std::fs::metadata(&part)
        .map(|meta| meta.len())
        .ok()
        .filter(|len| *len > 0)
        .zip(std::fs::read_to_string(&stamp).ok())
        .map(|(len, validator)| (len, validator.trim().to_string()));

    let mut attempt = resume;
    loop {
        let resuming = attempt.is_some();
        let result = fetch_once(
            url,
            token,
            &part,
            &stamp,
            attempt.as_ref().map(|(len, v)| (*len, v.as_str())),
        );

        match result {
            Ok(()) => {
                // Renamed last: until this, nothing can mistake a partial
                // download for the model.
                std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
                std::fs::remove_file(&stamp).ok();
                return Ok(target);
            }
            Err(e) => {
                std::fs::remove_file(&part).ok();
                std::fs::remove_file(&stamp).ok();
                // A resume can fail for reasons a clean request would not: a
                // .part the file has since outgrown gets a 416, and a server
                // that does not do ranges may refuse outright. Neither should
                // leave someone unable to open their model, so try once more
                // from the beginning. A plain request that failed has nothing
                // left to try.
                if !resuming {
                    return Err(e);
                }
                attempt = None;
            }
        }
    }
}

/// One request for a file, appending to `part` when the server picks up where
/// we left off and replacing it when it does not.
fn fetch_once(
    url: &str,
    token: Option<&str>,
    part: &std::path::Path,
    stamp: &std::path::Path,
    resume: Option<(u64, &str)>,
) -> Result<(), String> {
    let mut request = ureq::get(url);
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        request = request.set("Authorization", &format!("Bearer {}", token));
    }
    if let Some((have, validator)) = resume {
        request = request
            .set("Range", &format!("bytes={}-", have))
            .set("If-Range", validator);
    }

    let response = request
        .call()
        .map_err(|e| format!("Could not reach the library: {}", e))?;

    // 206 is the only answer that means "carry on". A 200 -- what the server
    // sends when the If-Range no longer matches -- means the file changed
    // underneath us, and whatever is already on disk describes something else.
    let appending = response.status() == 206;
    let modified = response.header("Last-Modified").map(|v| v.to_string());

    let mut file = if appending {
        std::fs::OpenOptions::new().append(true).open(part)
    } else {
        std::fs::File::create(part)
    }
    .map_err(|e| e.to_string())?;

    // Written before the body, so an interrupted transfer still knows what it
    // was fetching. A server that sends no validator leaves nothing to resume
    // against, and the next attempt starts over.
    match &modified {
        Some(value) => {
            std::fs::write(stamp, value).ok();
        }
        None => {
            std::fs::remove_file(stamp).ok();
        }
    }

    let mut reader = response.into_reader();
    // Streamed: the whole point of doing this in Rust is not holding the file.
    std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    Ok(())
}

/// Fetch a file from a remote library and open it with whatever handles it.
///
/// This is the entire reason a desktop client connected to a remote server is
/// worth having. A browser can offer a download; it cannot put the file into
/// Lychee on this machine. The download happens in Rust because these run to
/// hundreds of megabytes and the webview should never hold one.
#[command]
async fn open_remote(
    app: tauri::AppHandle,
    url: String,
    token: Option<String>,
    filename: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let target = tauri::async_runtime::spawn_blocking(move || {
        fetch_to_cache(&url, token.as_deref(), &filename)
    })
    .await
    .map_err(|e| e.to_string())??;

    app.opener()
        .open_path(target.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("Could not open it: {}", e))
}

/// Throw away everything fetched from remote libraries.
#[command]
fn clear_remote_cache() -> Result<u64, String> {
    let dir = remote_cache();
    let mut freed = 0u64;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if std::fs::remove_file(entry.path()).is_ok() {
                    freed += meta.len();
                }
            }
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filename arriving from someone else's server becomes a path on this
    /// machine, so it gets the same treatment as anything else that does.
    #[test]
    fn a_remote_filename_cannot_escape_the_cache() {
        for hostile in ["../../etc/passwd", "..", ".", "/etc/passwd", "a/b/c", "C:\\Windows\\x"] {
            match safe_download_name(hostile) {
                None => {}
                Some(name) => {
                    assert!(!name.contains('/'), "{} -> {}", hostile, name);
                    assert!(!name.contains('\\'), "{} -> {}", hostile, name);
                    assert_ne!(name, "..", "{}", hostile);
                    let joined = std::path::Path::new("/cache").join(&name);
                    assert!(
                        !joined.components().any(|c| c.as_os_str() == ".."),
                        "{} -> {:?}",
                        hostile,
                        joined
                    );
                }
            }
        }
    }

    #[test]
    fn ordinary_filenames_survive_intact() {
        assert_eq!(
            safe_download_name("Throk_Body_130mm.stl").as_deref(),
            Some("Throk_Body_130mm.stl")
        );
        assert_eq!(
            safe_download_name("plate v2 (final).ctb").as_deref(),
            Some("plate v2 (final).ctb")
        );
    }

    #[test]
    fn a_nameless_download_is_refused() {
        assert_eq!(safe_download_name(""), None);
        assert_eq!(safe_download_name("   "), None);
        assert_eq!(safe_download_name("..."), None);
    }
}


/// The resumable download, checked against a server that answers exactly what
/// each case is about.
///
/// Deliberately a scripted stub rather than a real range server: what matters
/// here is how the client reacts to each answer, and in particular that a 200
/// replaces the partial file instead of appending to it. The real server's
/// range handling -- 206, suffix ranges, 416, If-Range -- is covered end to
/// end by tools/remote-client.sh.
#[cfg(test)]
mod download_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};

    const LM: &str = "Wed, 21 Oct 2026 07:28:00 GMT";
    const LM_NEWER: &str = "Thu, 22 Oct 2026 09:00:00 GMT";

    /// Answers each request with the next reply in the list and records the
    /// request head, so a test can assert what was actually asked for.
    fn scripted(replies: Vec<(u16, Option<&'static str>, &'static [u8])>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);

        std::thread::spawn(move || {
            for (status, modified, body) in replies {
                let Ok((mut sock, _)) = listener.accept() else { return };

                let mut reader = BufReader::new(sock.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) if line == "\r\n" => break,
                        Ok(_) => head.push_str(&line),
                        Err(_) => break,
                    }
                }
                log.lock().unwrap().push(head);

                let reason = match status {
                    206 => "Partial Content",
                    416 => "Range Not Satisfiable",
                    _ => "OK",
                };
                let mut response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n",
                    status,
                    reason,
                    body.len()
                );
                if let Some(value) = modified {
                    response.push_str(&format!("Last-Modified: {}\r\n", value));
                }
                response.push_str("Connection: close\r\n\r\n");
                let _ = sock.write_all(response.as_bytes());
                let _ = sock.write_all(body);
                let _ = sock.flush();
            }
        });

        (url, seen)
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stlib_dl_{}_{}_{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_fresh_download_keeps_the_validator_it_will_need_to_resume() {
        let dir = scratch("fresh");
        let (url, seen) = scripted(vec![(200, Some(LM), b"HELLOWORLD")]);

        let target = fetch_into(&dir, &url, None, "body.stl").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"HELLOWORLD");
        // Complete, so the working files are gone and only the model is left.
        assert!(!dir.join("body.stl.part").exists());
        assert!(!dir.join("body.stl.part-lastmod").exists());

        let head = seen.lock().unwrap()[0].clone();
        assert!(!head.contains("Range:"), "asked for a range on a fresh download:\n{}", head);
    }

    #[test]
    fn a_resume_asks_for_the_rest_and_appends_it() {
        let dir = scratch("resume");
        std::fs::write(dir.join("body.stl.part"), b"HELLO").unwrap();
        std::fs::write(dir.join("body.stl.part-lastmod"), LM).unwrap();

        let (url, seen) = scripted(vec![(206, Some(LM), b"WORLD")]);
        let target = fetch_into(&dir, &url, None, "body.stl").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"HELLOWORLD");

        let head = seen.lock().unwrap()[0].clone();
        assert!(head.contains("Range: bytes=5-"), "no range asked for:\n{}", head);
        assert!(head.contains(&format!("If-Range: {}", LM)), "no validator sent:\n{}", head);
    }

    /// The case the whole design exists for. The model was re-exported while
    /// the download was interrupted; the server answers 200 rather than 206;
    /// the bytes already on disk describe the old file and must be thrown
    /// away, not prepended to the new one.
    #[test]
    fn a_changed_file_replaces_the_partial_instead_of_splicing_onto_it() {
        let dir = scratch("changed");
        std::fs::write(dir.join("body.stl.part"), b"OLDOLDOLD").unwrap();
        std::fs::write(dir.join("body.stl.part-lastmod"), LM).unwrap();

        let (url, _seen) = scripted(vec![(200, Some(LM_NEWER), b"NEWFILE")]);
        let target = fetch_into(&dir, &url, None, "body.stl").unwrap();

        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"NEWFILE",
            "the old partial bytes survived into the finished file"
        );
    }

    /// Bytes with no validator beside them cannot be proven to belong to this
    /// version of the file, so they are not resumed from.
    #[test]
    fn a_partial_with_no_validator_starts_again() {
        let dir = scratch("novalidator");
        std::fs::write(dir.join("body.stl.part"), b"ORPHANED").unwrap();

        let (url, seen) = scripted(vec![(200, Some(LM), b"WHOLE")]);
        let target = fetch_into(&dir, &url, None, "body.stl").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"WHOLE");
        let head = seen.lock().unwrap()[0].clone();
        assert!(!head.contains("Range:"), "resumed without a validator:\n{}", head);
    }

    /// A .part the file has since outgrown gets a 416, and a server with no
    /// range support may refuse outright. Neither may leave someone unable to
    /// open their model.
    #[test]
    fn a_refused_resume_falls_back_to_the_whole_file() {
        let dir = scratch("refused");
        std::fs::write(dir.join("body.stl.part"), b"STALEBYTES").unwrap();
        std::fs::write(dir.join("body.stl.part-lastmod"), LM).unwrap();

        let (url, seen) = scripted(vec![
            (416, Some(LM), b""),
            (200, Some(LM_NEWER), b"WHOLEFILE"),
        ]);
        let target = fetch_into(&dir, &url, None, "body.stl").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"WHOLEFILE");
        assert_eq!(seen.lock().unwrap().len(), 2, "did not retry after the refusal");
    }

    /// A server that offers no validator leaves nothing to resume against, so
    /// nothing is kept that would invite one.
    #[test]
    fn no_validator_from_the_server_means_no_stamp_left_behind() {
        let dir = scratch("nostamp");
        let part = dir.join("body.stl.part");
        let stamp = dir.join("body.stl.part-lastmod");
        std::fs::write(&stamp, LM).unwrap();

        let (url, _seen) = scripted(vec![(200, None, b"ANON")]);
        fetch_once(&url, None, &part, &stamp, None).unwrap();

        assert!(!stamp.exists(), "kept a validator the server never gave");
        assert_eq!(std::fs::read(&part).unwrap(), b"ANON");
    }
}
