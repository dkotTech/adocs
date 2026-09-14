use actix_web::{HttpRequest, HttpResponse, web};
use serde::Serialize;

use super::file;
use crate::config::Config;
use crate::content::{DocMeta, Outcome, Status, Store};
use crate::errors::AppError;
use crate::paths;
use crate::render::{self, Render};

// GET /api/tree - the directory tree of the current version
pub async fn tree(store: web::Data<Store>) -> HttpResponse {
    HttpResponse::Ok().json(&store.current().tree)
}

/// Archive state plus the server settings the interface needs.
#[derive(Serialize)]
pub struct BuildInfo {
    #[serde(flatten)]
    pub status: Status,
    pub search_regex: bool,
}

fn build_info(cfg: &Config, store: &Store) -> BuildInfo {
    BuildInfo {
        status: store.status(),
        search_regex: cfg.search_regex,
    }
}

// GET /api/build - which archive is loaded and when it was refreshed
pub async fn build(cfg: web::Data<Config>, store: web::Data<Store>) -> HttpResponse {
    HttpResponse::Ok().json(build_info(&cfg, &store))
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub result: Outcome,
    pub message: String,
    pub build: BuildInfo,
}

// POST /api/refresh - re-read the archive; nothing changes when the hash matches
pub async fn refresh(
    cfg: web::Data<Config>,
    store: web::Data<Store>,
) -> Result<HttpResponse, AppError> {
    let worker = store.clone();
    let (result, message) = web::block(move || worker.refresh())
        .await
        .map_err(|_| AppError::Internal)?;
    tracing::info!("refresh on request: {result:?}, {message}");

    Ok(HttpResponse::Ok().json(RefreshResponse {
        result,
        message,
        build: build_info(&cfg, &store),
    }))
}

#[derive(Serialize)]
pub struct DocResponse {
    pub meta: DocMeta,
    pub render: Render,
}

// GET /api/docs/{path} - metadata and content prepared for display
pub async fn get(
    store: web::Data<Store>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let path = paths::normalize(&path)?;
    let index = store.current();
    let meta = index.get(&path).cloned().ok_or(AppError::NotFound)?;
    let data = if render::needs_data(&meta) {
        let bytes = web::block(move || index.read(&path))
            .await
            .map_err(|_| AppError::Internal)?
            .ok_or(AppError::NotFound)?
            .map_err(|_| AppError::Internal)?;
        Some(bytes)
    } else {
        None
    };

    Ok(HttpResponse::Ok().json(DocResponse {
        render: render::render(&meta, data.as_deref()),
        meta,
    }))
}

// GET /raw/{path} - the content as is
pub async fn raw(
    req: HttpRequest,
    store: web::Data<Store>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let path = paths::normalize(&path)?;
    let index = store.current();
    let meta = index.get(&path).cloned().ok_or(AppError::NotFound)?;
    file::serve(&req, index, &meta).await
}
