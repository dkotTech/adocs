use actix_web::http::{Method, header};
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::config::Config;
use crate::content::Store;
use crate::errors::AppError;
use crate::search::{self, Failure};

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(default)]
    regex: bool,
    case: Option<String>,
    path: Option<String>,
    context: Option<usize>,
    limit: Option<usize>,
    per_file: Option<usize>,
    format: Option<String>,
}

// GET /api/search?q=<text> - full-text search over the text files of the current version
pub async fn search(
    req: HttpRequest,
    cfg: web::Data<Config>,
    store: web::Data<Store>,
    query: web::Query<SearchQuery>,
) -> Result<HttpResponse, AppError> {
    let q = query.into_inner();
    let text = match q.format.as_deref() {
        None => prefers_text(&req),
        Some("text") => true,
        Some("json") => false,
        Some(other) => {
            return Err(AppError::bad_request(format!(
                "unknown format {other:?}: use json or text"
            )));
        }
    };
    let params = search::Request {
        query: q.q,
        regex: q.regex,
        case: q.case,
        path: q.path,
        context: q.context,
        limit: q.limit,
        per_file: q.per_file,
    }
    .validate(cfg.search_regex)
    .map_err(AppError::BadRequest)?;

    let content_type = if text {
        "text/plain; charset=utf-8"
    } else {
        "application/json"
    };
    // HEAD checks the parameters but runs no search and takes no slot.
    if req.method() == Method::HEAD {
        return Ok(HttpResponse::Ok().content_type(content_type).finish());
    }

    let results = search::search(store.current(), params)
        .await
        .map_err(|e| match e {
            Failure::Busy => AppError::TooManyRequests,
            Failure::Invalid(msg) => AppError::BadRequest(msg),
            Failure::Internal => AppError::Internal,
        })?;

    Ok(if text {
        HttpResponse::Ok()
            .content_type(content_type)
            .body(search::to_text(&results))
    } else {
        HttpResponse::Ok().json(results)
    })
}

fn prefers_text(req: &HttpRequest) -> bool {
    let accept = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    accept.contains("text/plain") && !accept.contains("application/json")
}
