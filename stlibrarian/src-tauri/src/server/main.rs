//! STLibrarian Pro -- the multi-user server.
//!
//! Same core as the desktop app, different front door. Build it with:
//!
//!     cargo build --release --no-default-features --features server \
//!         --bin stlibrarian-server
//!
//! It deliberately links no Tauri, so it runs on a headless NAS with no GTK or
//! webkit present.

use axum::body::Body;
use axum::extract::{Path, Query, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use stlibrarian_lib::core::models::{
    Collection, DuplicateGroup, LibraryQuery, LibraryStats, ModelRecord, NewPrint, Package,
    LibraryLocation, PackageSuggestion, PrintRecord, ScanReport, Slice, TagCount, User,
};
use stlibrarian_lib::core::organize::{OrganizeReport, PlannedMove};
use stlibrarian_lib::core::{auth, db, organize, paths, relink, scanner, stl, thumbs};
use tokio_util::io::ReaderStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

// ---------------------------------------------------------------- errors ----

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn server_error(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg.into())
}

/// Run a database closure on the blocking pool with its own connection.
///
/// A `rusqlite::Connection` is not `Sync`, and sharing one behind a mutex would
/// let a long scan block every reader. Separate connections plus WAL let reads
/// continue while a scan writes.
async fn with_db<T, F>(f: F) -> Result<T, ApiError>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        f(&conn)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?
    .map_err(server_error)
}

// ------------------------------------------------------------------ auth ----

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginRes {
    token: String,
    user: User,
}

async fn login_route(Json(req): Json<LoginReq>) -> Result<Json<LoginRes>, ApiError> {
    let session = tokio::task::spawn_blocking(move || {
        let conn = db::open_default().ok()?;
        auth::login(&conn, &req.username, &req.password)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;

    match session {
        Some(s) => Ok(Json(LoginRes {
            token: s.token,
            user: s.user,
        })),
        // One message for both "no such user" and "wrong password", so the
        // endpoint cannot be used to enumerate accounts.
        None => Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password".into(),
        )),
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
}

async fn require_auth(mut req: Request, next: Next) -> Result<Response, ApiError> {
    let token = bearer(req.headers())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "Missing bearer token".into()))?;

    let user = tokio::task::spawn_blocking(move || {
        let conn = db::open_default().ok()?;
        auth::verify_token(&conn, &token)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?
    .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "Session expired".into()))?;

    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

fn require_admin(user: &User) -> Result<(), ApiError> {
    if user.is_admin() {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "Administrator access required".into(),
        ))
    }
}

async fn logout_route(headers: HeaderMap) -> StatusCode {
    if let Some(token) = bearer(&headers) {
        tokio::task::spawn_blocking(move || {
            if let Ok(conn) = db::open_default() {
                auth::logout(&conn, &token);
            }
        })
        .await
        .ok();
    }
    StatusCode::NO_CONTENT
}

async fn me_route(Extension(user): Extension<User>) -> Json<User> {
    Json(user)
}

// --------------------------------------------------------------- library ----

/// Query-string form of `LibraryQuery`.
///
/// `serde_urlencoded` cannot build a `Vec<String>` out of a query string, so
/// tags arrive comma-separated and are split here rather than silently
/// failing to deserialize.
#[derive(Deserialize)]
struct ModelsParams {
    search: Option<String>,
    tags: Option<String>,
    collection: Option<i64>,
    creator: Option<String>,
    package: Option<i64>,
    printed: Option<bool>,
    variant: Option<String>,
    sort: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl From<ModelsParams> for LibraryQuery {
    fn from(p: ModelsParams) -> Self {
        LibraryQuery {
            search: p.search,
            tags: p
                .tags
                .unwrap_or_default()
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            collection: p.collection,
            printed: p.printed,
            variant: p.variant,
            creator: p.creator,
            package: p.package,
            sort: p.sort,
            limit: p.limit,
            offset: p.offset,
        }
    }
}

async fn list_models_route(
    Query(params): Query<ModelsParams>,
) -> Result<Json<Vec<ModelRecord>>, ApiError> {
    let q: LibraryQuery = params.into();
    with_db(move |conn| db::list_models(conn, &q).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn get_model_route(Path(id): Path<i64>) -> Result<Json<ModelRecord>, ApiError> {
    let found = with_db(move |conn| db::get_model(conn, id).map_err(|e| e.to_string())).await?;
    found
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "No such model".into()))
}

/// Stream the STL itself.
///
/// Addressed by id, so only indexed files are reachable -- a path in the URL
/// would let any signed-in user read arbitrary files off the server.
async fn model_file_route(Path(id): Path<i64>) -> Result<Response, ApiError> {
    let record = with_db(move |conn| db::get_model(conn, id).map_err(|e| e.to_string()))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "No such model".into()))?;

    let file = tokio::fs::File::open(&record.path)
        .await
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("File unavailable: {}", e)))?;

    // Streamed, not read into memory: these run to hundreds of megabytes.
    let body = Body::from_stream(ReaderStream::new(file));
    Ok((
        [
            (header::CONTENT_TYPE, "model/stl".to_string()),
            (header::CONTENT_LENGTH, record.size.to_string()),
        ],
        body,
    )
        .into_response())
}

#[derive(Deserialize)]
struct PreviewParams {
    max: Option<u32>,
}

/// Decimated mesh for the grid and the viewer. See the note on the desktop
/// `preview_mesh` command: the full file must never reach a browser.
async fn preview_route(
    Path(id): Path<i64>,
    Query(params): Query<PreviewParams>,
) -> Result<Response, ApiError> {
    let max = params.max.unwrap_or(120_000).clamp(1_000, 1_000_000);
    let bytes = with_db(move |conn| {
        let record = db::get_model(conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No such model".to_string())?;
        let mesh = stl::read_preview(std::path::Path::new(&record.path), max)
            .map_err(|e| e.to_string())?;
        Ok(mesh.into_bytes())
    })
    .await?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "private, max-age=600"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct SetTagsReq {
    tags: Vec<String>,
}

async fn set_tags_route(
    Path(id): Path<i64>,
    Json(req): Json<SetTagsReq>,
) -> Result<StatusCode, ApiError> {
    with_db(move |conn| db::set_tags(conn, id, &req.tags).map_err(|e| e.to_string())).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_model_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| db::delete_model(conn, id).map_err(|e| e.to_string())).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn tags_route() -> Result<Json<Vec<TagCount>>, ApiError> {
    with_db(|conn| db::list_tags(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn stats_route() -> Result<Json<LibraryStats>, ApiError> {
    with_db(|conn| {
        let mut s = db::library_stats(conn).map_err(|e| e.to_string())?;
        s.missing = db::find_missing(conn).map(|m| m.len() as i64).unwrap_or(0);
        Ok(s)
    })
    .await
    .map(Json)
}

#[derive(Deserialize)]
struct ScanReq {
    path: String,
}

async fn scan_route(
    Extension(user): Extension<User>,
    Json(req): Json<ScanReq>,
) -> Result<Json<ScanReport>, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| Ok(scanner::scan_folder(conn, &req.path)))
        .await
        .map(Json)
}

// ----------------------------------------------------------- collections ----

#[derive(Deserialize)]
struct NameReq {
    name: String,
    /// Collections only. Absent means shared.
    #[serde(default)]
    shared: Option<bool>,
}

#[derive(Deserialize)]
struct ModelIdsReq {
    model_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct BulkTagReq {
    model_ids: Vec<i64>,
    tag: String,
}

#[derive(Serialize)]
struct Changed {
    changed: usize,
}

async fn collections_route(
    Extension(user): Extension<User>,
) -> Result<Json<Vec<Collection>>, ApiError> {
    with_db(move |conn| db::list_collections(conn, Some(user.id)).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn create_collection_route(
    Extension(user): Extension<User>,
    Json(req): Json<NameReq>,
) -> Result<Json<Collection>, ApiError> {
    // A duplicate name is the user's mistake, not the server's, so it comes
    // back as 400 with the message rather than a 500.
    let made = tokio::task::spawn_blocking(move || {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        // Shared by default: a shop's shelves are the common case, and a
        // private queue is the thing you opt into.
        let owner = if req.shared.unwrap_or(true) {
            None
        } else {
            Some(user.id)
        };
        db::create_collection(&conn, &req.name, owner)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;

    made.map(Json).map_err(bad)
}

async fn rename_collection_route(
    Path(id): Path<i64>,
    Json(req): Json<NameReq>,
) -> Result<StatusCode, ApiError> {
    let done = tokio::task::spawn_blocking(move || {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        db::rename_collection(&conn, id, &req.name)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;

    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

async fn delete_collection_route(Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    with_db(move |conn| db::delete_collection(conn, id).map_err(|e| e.to_string())).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_to_collection_route(
    Path(id): Path<i64>,
    Json(req): Json<ModelIdsReq>,
) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| {
        db::add_to_collection(conn, id, &req.model_ids).map_err(|e| e.to_string())
    })
    .await
    .map(|changed| Json(Changed { changed }))
}

async fn remove_from_collection_route(
    Path(id): Path<i64>,
    Json(req): Json<ModelIdsReq>,
) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| {
        db::remove_from_collection(conn, id, &req.model_ids).map_err(|e| e.to_string())
    })
    .await
    .map(|changed| Json(Changed { changed }))
}

// ----------------------------------------------------------------- users ----

#[derive(Deserialize)]
struct NewUserReq {
    username: String,
    password: String,
    #[serde(default)]
    role: String,
}

#[derive(Deserialize)]
struct RoleReq {
    role: String,
}

#[derive(Deserialize)]
struct PasswordReq {
    password: String,
}

async fn users_route(Extension(user): Extension<User>) -> Result<Json<Vec<User>>, ApiError> {
    require_admin(&user)?;
    let list = tokio::task::spawn_blocking(move || {
        db::open_default().map(|conn| auth::list_users(&conn)).unwrap_or_default()
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    Ok(Json(list))
}

async fn create_user_route(
    Extension(user): Extension<User>,
    Json(req): Json<NewUserReq>,
) -> Result<Json<User>, ApiError> {
    require_admin(&user)?;
    let role = if req.role.is_empty() { "user".to_string() } else { req.role };
    if !auth::valid_role(&role) {
        return Err(bad("Role must be admin or user"));
    }
    let made = tokio::task::spawn_blocking(move || -> std::result::Result<User, String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        auth::create_user(&conn, &req.username, &req.password, &role)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    made.map(Json).map_err(bad)
}

async fn set_role_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
    Json(req): Json<RoleReq>,
) -> Result<StatusCode, ApiError> {
    require_admin(&user)?;
    let done = tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        auth::set_role(&conn, id, &req.role)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

/// Change a password: an administrator may change anyone's, and anyone may
/// change their own.
async fn set_password_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
    Json(req): Json<PasswordReq>,
) -> Result<StatusCode, ApiError> {
    if !user.is_admin() && user.id != id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "You can only change your own password".into(),
        ));
    }
    let done = tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        auth::set_password(&conn, id, &req.password)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

async fn delete_user_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    require_admin(&user)?;
    let done = tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        auth::delete_user(&conn, id)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

// -------------------------------------------------------------- packages ----

#[derive(Deserialize)]
struct NewPackageReq {
    name: String,
    #[serde(default)]
    root: String,
    #[serde(default)]
    model_ids: Vec<i64>,
}

async fn packages_route() -> Result<Json<Vec<Package>>, ApiError> {
    with_db(|conn| db::list_packages(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn package_suggestions_route() -> Result<Json<Vec<PackageSuggestion>>, ApiError> {
    with_db(|conn| db::suggest_packages(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn create_package_route(Json(req): Json<NewPackageReq>) -> Result<Json<Package>, ApiError> {
    let made = tokio::task::spawn_blocking(move || -> std::result::Result<Package, String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        let pkg = db::create_package(&conn, &req.name, &req.root)?;
        db::assign_to_package(&conn, pkg.id, &req.model_ids).map_err(|e| e.to_string())?;
        Ok(pkg)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    made.map(Json).map_err(bad)
}

async fn rename_package_route(
    Path(id): Path<i64>,
    Json(req): Json<NameReq>,
) -> Result<StatusCode, ApiError> {
    let done = tokio::task::spawn_blocking(move || {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        db::rename_package(&conn, id, &req.name)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

async fn delete_package_route(Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    with_db(move |conn| db::delete_package(conn, id).map_err(|e| e.to_string())).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_package_route(
    Path(id): Path<i64>,
    Json(req): Json<ModelIdsReq>,
) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| db::assign_to_package(conn, id, &req.model_ids).map_err(|e| e.to_string()))
        .await
        .map(|changed| Json(Changed { changed }))
}

async fn unassign_package_route(Json(req): Json<ModelIdsReq>) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| db::unassign_from_package(conn, &req.model_ids).map_err(|e| e.to_string()))
        .await
        .map(|changed| Json(Changed { changed }))
}

// -------------------------------------------------------- lost and found ----

#[derive(Deserialize)]
struct RootReq {
    root: String,
}

#[derive(Deserialize)]
struct RepointReq {
    path: String,
}

/// Where the library keeps itself. Admin only: it is a path on the server.
async fn location_route(
    Extension(user): Extension<User>,
) -> Result<Json<LibraryLocation>, ApiError> {
    require_admin(&user)?;
    Ok(Json(paths::location()))
}

async fn missing_route() -> Result<Json<Vec<ModelRecord>>, ApiError> {
    with_db(|conn| db::missing_models(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

/// What relinking would reconnect. Reads only.
async fn relink_plan_route(
    Extension(user): Extension<User>,
    Json(req): Json<RootReq>,
) -> Result<Json<Vec<relink::RelinkMatch>>, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| relink::plan(conn, std::path::Path::new(&req.root)))
        .await
        .map(Json)
}

async fn relink_apply_route(
    Extension(user): Extension<User>,
    Json(req): Json<RootReq>,
) -> Result<Json<relink::RelinkReport>, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| relink::apply(conn, std::path::Path::new(&req.root)))
        .await
        .map(Json)
}

/// Point one model at one file, by hand.
async fn repoint_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
    Json(req): Json<RepointReq>,
) -> Result<StatusCode, ApiError> {
    require_admin(&user)?;
    let done = tokio::task::spawn_blocking(move || -> std::result::Result<(), String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        relink::repoint(&conn, id, &req.path)
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    done.map(|_| StatusCode::NO_CONTENT).map_err(bad)
}

// -------------------------------------------------------------- organise ----

#[derive(Deserialize)]
struct OrganizeReq {
    root: String,
}

/// What organising would do. Reads only, so it is safe to call at any time.
async fn organize_plan_route(
    Extension(user): Extension<User>,
    Json(req): Json<OrganizeReq>,
) -> Result<Json<Vec<PlannedMove>>, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| organize::plan(conn, std::path::Path::new(&req.root)))
        .await
        .map(Json)
}

/// Carry it out. The only endpoint in the product that writes to the library's
/// files, which is why it is admin-only, server-only, and recomputes the plan
/// itself rather than trusting one sent by a client.
async fn organize_apply_route(
    Extension(user): Extension<User>,
    Json(req): Json<OrganizeReq>,
) -> Result<Json<OrganizeReport>, ApiError> {
    require_admin(&user)?;
    with_db(move |conn| organize::apply(conn, std::path::Path::new(&req.root)))
        .await
        .map(Json)
}

// ---------------------------------------------------------------- slices ----

#[derive(Deserialize)]
struct SliceOwnerReq {
    model_id: Option<i64>,
    package_id: Option<i64>,
}

async fn slices_route(Path(id): Path<i64>) -> Result<Json<Vec<Slice>>, ApiError> {
    with_db(move |conn| db::slices_for_model(conn, id).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn orphan_slices_route() -> Result<Json<Vec<Slice>>, ApiError> {
    with_db(|conn| db::unattached_slices(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn set_slice_owner_route(
    Path(id): Path<i64>,
    Json(req): Json<SliceOwnerReq>,
) -> Result<StatusCode, ApiError> {
    with_db(move |conn| {
        db::set_slice_owner(conn, id, req.model_id, req.package_id).map_err(|e| e.to_string())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Download the sliced file itself -- which is how a desktop client connected
/// to a remote server can hand it to a local slicer.
async fn slice_file_route(Path(id): Path<i64>) -> Result<Response, ApiError> {
    let path = with_db(move |conn| {
        conn.query_row(
            "SELECT path FROM slices WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())
    })
    .await?;

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("File unavailable: {}", e)))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "slice".into());

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", name),
            ),
        ],
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}

// ---------------------------------------------------------------- prints ----

async fn prints_route(Path(id): Path<i64>) -> Result<Json<Vec<PrintRecord>>, ApiError> {
    with_db(move |conn| db::list_prints(conn, id).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

#[derive(Serialize)]
struct Created {
    id: i64,
}

async fn log_print_route(
    Extension(user): Extension<User>,
    Path(id): Path<i64>,
    Json(entry): Json<NewPrint>,
) -> Result<Json<Created>, ApiError> {
    let made = tokio::task::spawn_blocking(move || -> std::result::Result<i64, String> {
        let conn = db::open_default().map_err(|e| e.to_string())?;
        db::add_print(&conn, id, &entry, Some(user.id))
    })
    .await
    .map_err(|e| server_error(e.to_string()))?;
    // A bad outcome value is the caller's mistake, so 400 with the reason.
    made.map(|id| Json(Created { id })).map_err(bad)
}

async fn delete_print_route(Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    with_db(move |conn| db::delete_print(conn, id).map_err(|e| e.to_string())).await?;
    Ok(StatusCode::NO_CONTENT)
}

// --------------------------------------------------------------- renders ----

#[derive(Deserialize)]
struct RenderReq {
    /// null puts the model back on its generated preview.
    image_id: Option<i64>,
}

async fn model_renders_route(Path(id): Path<i64>) -> Result<Json<Vec<(i64, String)>>, ApiError> {
    with_db(move |conn| db::images_near_model(conn, id).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn set_render_route(
    Path(id): Path<i64>,
    Json(req): Json<RenderReq>,
) -> Result<StatusCode, ApiError> {
    with_db(move |conn| {
        let model = db::get_model(conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No such model".to_string())?;
        db::set_model_image(conn, id, req.image_id).map_err(|e| e.to_string())?;
        // Cached by content hash, and the content did not change.
        thumbs::forget(&model.hash);
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------ duplicates ----

async fn duplicates_route() -> Result<Json<Vec<DuplicateGroup>>, ApiError> {
    with_db(|conn| db::duplicate_groups(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

// ------------------------------------------------------------ provenance ----

#[derive(Deserialize)]
struct ProvenanceReq {
    creator: String,
    source: String,
}

async fn set_provenance_route(
    Path(id): Path<i64>,
    Json(req): Json<ProvenanceReq>,
) -> Result<StatusCode, ApiError> {
    with_db(move |conn| {
        db::set_provenance(conn, id, &req.creator, &req.source).map_err(|e| e.to_string())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn creators_route() -> Result<Json<Vec<TagCount>>, ApiError> {
    with_db(|conn| db::list_creators(conn).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

// ------------------------------------------------------------- bulk tags ----

async fn bulk_add_tag_route(Json(req): Json<BulkTagReq>) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| {
        db::add_tag_to_models(conn, &req.model_ids, &req.tag).map_err(|e| e.to_string())
    })
    .await
    .map(|changed| Json(Changed { changed }))
}

async fn bulk_remove_tag_route(Json(req): Json<BulkTagReq>) -> Result<Json<Changed>, ApiError> {
    with_db(move |conn| {
        db::remove_tag_from_models(conn, &req.model_ids, &req.tag).map_err(|e| e.to_string())
    })
    .await
    .map(|changed| Json(Changed { changed }))
}

// ------------------------------------------------------------ thumbnails ----

async fn get_thumb_route(Path(hash): Path<String>) -> Result<Response, ApiError> {
    // Rendered here if it does not exist yet. The server does not wait for a
    // browser with WebGL to come along and make one for it.
    let bytes = with_db(move |conn| {
        if let Some(cached) = thumbs::read_png(&hash) {
            return Ok(Some(cached));
        }
        match db::path_for_hash(conn, &hash).map_err(|e| e.to_string())? {
            Some(path) => {
                let render = db::image_for_hash(conn, &hash).map_err(|e| e.to_string())?;
                thumbs::ensure(
                    &hash,
                    std::path::Path::new(&path),
                    render.as_deref().map(std::path::Path::new),
                )
                .map(Some)
            }
            None => Ok(None),
        }
    })
    .await?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "No preview".into()))?;

    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            // Content-addressed, so it can never go stale.
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct PutThumbReq {
    data_url: String,
}

async fn put_thumb_route(
    Path(hash): Path<String>,
    Json(req): Json<PutThumbReq>,
) -> Result<StatusCode, ApiError> {
    if !thumbs::is_valid_hash(&hash) {
        return Err(bad("Invalid model hash"));
    }
    with_db(move |conn| {
        thumbs::save(&hash, &req.data_url)?;
        db::set_has_thumb(conn, &hash, true).map_err(|e| e.to_string())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ main ----

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    paths::ensure_dirs().expect("could not create data directory");

    {
        let conn = db::open_default().expect("could not open library database");
        auth::purge_expired_sessions(&conn);
        if let Some((user, password)) = auth::ensure_bootstrap_admin(&conn) {
            // Printed once, never stored in plaintext, never a fixed default.
            println!("\n  STLibrarian Pro first run");
            println!("  ------------------------------------------");
            println!("  username: {}", user);
            println!("  password: {}", password);
            println!("  Save this now -- it is not shown again.\n");
        }
    }

    let protected = Router::new()
        .route("/me", get(me_route))
        .route("/models", get(list_models_route))
        .route("/models/{id}", get(get_model_route))
        .route("/models/{id}", delete(delete_model_route))
        .route("/models/{id}/file", get(model_file_route))
        .route("/models/{id}/preview", get(preview_route))
        .route("/models/{id}/tags", post(set_tags_route))
        .route("/tags", get(tags_route))
        .route("/users", get(users_route))
        .route("/users", post(create_user_route))
        .route("/users/{id}", patch(set_role_route))
        .route("/users/{id}", delete(delete_user_route))
        .route("/users/{id}/password", post(set_password_route))
        .route("/collections", get(collections_route))
        .route("/collections", post(create_collection_route))
        .route("/collections/{id}", patch(rename_collection_route))
        .route("/collections/{id}", delete(delete_collection_route))
        .route("/collections/{id}/models", post(add_to_collection_route))
        .route("/collections/{id}/models/remove", post(remove_from_collection_route))
        // Under /bulk rather than /models so a selection operation can never
        // be confused with a route addressing one model by id.
        .route("/packages", get(packages_route))
        .route("/packages", post(create_package_route))
        .route("/packages/suggestions", get(package_suggestions_route))
        .route("/packages/{id}", patch(rename_package_route))
        .route("/packages/{id}", delete(delete_package_route))
        .route("/packages/{id}/models", post(assign_package_route))
        .route("/duplicates", get(duplicates_route))
        .route("/library/location", get(location_route))
        .route("/library/missing", get(missing_route))
        .route("/library/relink/plan", post(relink_plan_route))
        .route("/library/relink/apply", post(relink_apply_route))
        .route("/models/{id}/repoint", post(repoint_route))
        .route("/organize/plan", post(organize_plan_route))
        .route("/organize/apply", post(organize_apply_route))
        .route("/models/{id}/slices", get(slices_route))
        .route("/slices/unattached", get(orphan_slices_route))
        .route("/slices/{id}/owner", post(set_slice_owner_route))
        .route("/slices/{id}/file", get(slice_file_route))
        .route("/models/{id}/prints", get(prints_route))
        .route("/models/{id}/prints", post(log_print_route))
        .route("/prints/{id}", delete(delete_print_route))
        .route("/models/{id}/renders", get(model_renders_route))
        .route("/models/{id}/render", post(set_render_route))
        .route("/creators", get(creators_route))
        .route("/models/{id}/provenance", post(set_provenance_route))
        .route("/bulk/packages/unassign", post(unassign_package_route))
        .route("/bulk/tags/add", post(bulk_add_tag_route))
        .route("/bulk/tags/remove", post(bulk_remove_tag_route))
        .route("/stats", get(stats_route))
        .route("/scan", post(scan_route))
        .route("/thumb/{hash}", get(get_thumb_route))
        .route("/thumb/{hash}", put(put_thumb_route))
        .layer(axum::middleware::from_fn(require_auth));

    let public = Router::new()
        .route("/login", post(login_route))
        .route("/logout", post(logout_route))
        .route("/health", get(|| async { "ok" }));

    // Nested so the API keeps its own 404. Without this an unmatched /api path
    // falls through to the SPA and an API client gets HTML under a 200.
    let api = Router::new()
        .merge(public)
        .merge(protected)
        .fallback(|| async {
            ApiError(StatusCode::NOT_FOUND, "Unknown API endpoint".into())
        });

    // The built React app. Unknown paths fall back to index.html so client-side
    // routing survives a refresh.
    let web_root = PathBuf::from(env_or("STLIBRARIAN_WEB_ROOT", "../dist"));
    // `fallback`, not `not_found_service`: the latter rewrites the response
    // status to 404, so a deep link would serve the app under a "missing"
    // status. `fallback` keeps index.html's own 200.
    let spa = ServeDir::new(&web_root).fallback(ServeFile::new(web_root.join("index.html")));

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        // Same-origin in production; permissive so `vite dev` on :1420 can talk
        // to the API while you work on the front end.
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let bind = env_or("STLIBRARIAN_BIND", "0.0.0.0:3000");
    let addr: SocketAddr = bind.parse().expect("STLIBRARIAN_BIND must be host:port");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("could not bind {}: {}", addr, e));

    log::info!("STLibrarian Pro listening on http://{}", addr);
    log::info!("Library at {}", paths::data_dir().display());
    log::info!("Serving web front end from {}", web_root.display());

    axum::serve(listener, app).await.expect("server failed");
}
