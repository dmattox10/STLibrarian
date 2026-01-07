use axum::{
    extract::Path,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
mod core;
use core::auth::*;
use core::db::*;
use core::models::ModelRecord;
use core::scanner::*;
use tauri::Manager;

#[derive(Deserialize)]
struct ScanReq {
    path: String,
}

#[derive(Deserialize)]
struct LoginReq {
    user: String,
    pass: String,
}

#[derive(Deserialize)]
struct SetTagsReq {
    model_id: i64,
    tags: Vec<String>,
}

async fn scan_route(Json(req): Json<ScanReq>) {
    let conn = open_db("stl.db").unwrap();
    scan_folder(&conn, &req.path);
}

async fn list_route() -> Json<Vec<ModelRecord>> {
    let conn = open_db("stl.db").unwrap();
    Json(list_models(&conn).unwrap_or_default())
}

async fn login_route(Json(req): Json<LoginReq>) -> Option<Json<AuthResponse>> {
    login(&req.user, &req.pass).map(Json)
}

async fn set_tags_route(Json(req): Json<SetTagsReq>) {
    let conn = open_db("stl.db").unwrap();
    set_tags(&conn, req.model_id, &req.tags);
}

async fn delete_model_route(Path(model_id): Path<i64>) {
    let conn = open_db("stl.db").unwrap();
    delete_model(&conn, model_id);
}

fn main() {
    // spawn axum in background
    tauri::async_runtime::spawn(async move {
        let app = Router::new()
            .route("/scan", post(scan_route))
            .route("/models", get(list_route))
            .route("/login", post(login_route))
            .route("/set_tags", post(set_tags_route))
            .route("/model/:id", delete(delete_model_route));

        axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init()) // ✅ correct
        .invoke_handler(tauri::generate_handler![
            scan,
            models,
            login_cmd,
            set_tags_cmd,
            delete_model_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
