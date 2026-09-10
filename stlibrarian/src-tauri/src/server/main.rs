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
use axum::routing::{delete, get, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use stlibrarian_lib::core::models::{
    LibraryQuery, LibraryStats, ModelRecord, ScanReport, TagCount, User,
};
use stlibrarian_lib::core::{auth, db, paths, scanner, thumbs};
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

// ------------------------------------------------------------ thumbnails ----

async fn get_thumb_route(Path(hash): Path<String>) -> Result<Response, ApiError> {
    let bytes = tokio::task::spawn_blocking(move || thumbs::read_png(&hash))
        .await
        .map_err(|e| server_error(e.to_string()))?
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
        .route("/models/{id}/tags", post(set_tags_route))
        .route("/tags", get(tags_route))
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
