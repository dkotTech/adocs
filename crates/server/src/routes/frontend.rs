use actix_web::http::StatusCode;
use actix_web::http::header::{self, HeaderValue};
use actix_web::{HttpRequest, HttpResponse, web};
use rust_embed::RustEmbed;

use super::{agents, file, read};
use crate::config::Config;
use crate::content::{DocMeta, Store};
use crate::errors::AppError;
use crate::paths;
use crate::render::escape_html;

/// The built frontend, embedded into the binary.
#[derive(RustEmbed)]
#[folder = "../../frontend/dist/"]
struct Dist;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/", read().to(home))
        .route("/docs", read().to(home))
        .route("/docs/{path:.*}", read().to(doc_page))
        .route("/assets/{file:.*}", read().to(asset));
}

/// Visually hidden, but present for clients that read HTML without running scripts.
const HIDDEN: &str = "position:absolute;width:1px;height:1px;margin:-1px;overflow:hidden;\
                      clip:rect(0 0 0 0);white-space:nowrap";

/// A browser gets the web page, any other client gets the content itself. Browsers always
/// send `text/html` in `Accept` when navigating; agents ask for markdown or send `*/*`.
fn wants_content(req: &HttpRequest) -> bool {
    let accept = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    accept.contains("text/markdown") || !accept.contains("text/html")
}

// GET / - the web page, or the llms.txt index for agents
async fn home(req: HttpRequest, cfg: web::Data<Config>, store: web::Data<Store>) -> HttpResponse {
    if wants_content(&req) {
        let base = agents::base_url(&req, &cfg);
        let mut resp = agents::generated(
            &req,
            "text/plain; charset=utf-8",
            agents::llms_index(&base, &cfg, &store),
        );
        resp.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept"));
        return resp;
    }
    let note = "<p>This documentation viewer is rendered with JavaScript. \
                An index of all documents with direct links: <a href=\"/llms.txt\">/llms.txt</a>.</p>";
    page(StatusCode::OK, &cfg, None, "", note)
}

// GET /docs/{path} - the web page for a document, or the document itself for agents
async fn doc_page(
    req: HttpRequest,
    cfg: web::Data<Config>,
    store: web::Data<Store>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let index = store.current();
    let meta = paths::normalize(&path)
        .ok()
        .and_then(|p| index.get(&p).cloned());

    if wants_content(&req) {
        let mut resp = match &meta {
            Some(meta) => file::serve(&req, index, meta).await?,
            None => HttpResponse::NotFound()
                .content_type("text/plain; charset=utf-8")
                .body(format!(
                    "Document not found: {path}\nIndex of all documents: {}/llms.txt\n",
                    agents::base_url(&req, &cfg)
                )),
        };
        resp.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept"));
        return Ok(resp);
    }

    Ok(match &meta {
        Some(meta) => {
            let raw = format!("/raw/{}", paths::encode_url(&meta.path));
            let head = format!(
                "<link rel=\"alternate\" type=\"{}\" href=\"{raw}\">",
                escape_html(&meta.content_type)
            );
            page(
                StatusCode::OK,
                &cfg,
                Some(meta),
                &head,
                &doc_note(meta, &raw),
            )
        }
        None => {
            let note = format!(
                "<p>Document not found: {}. An index of all documents: \
                 <a href=\"/llms.txt\">/llms.txt</a>.</p>",
                escape_html(&path)
            );
            page(StatusCode::NOT_FOUND, &cfg, None, "", &note)
        }
    })
}

fn doc_note(meta: &DocMeta, raw: &str) -> String {
    format!(
        "<h1>{title}</h1><p>This page is rendered with JavaScript. The file itself: \
         <a href=\"{raw}\">{raw}</a> ({ct}, {size} bytes). This address also returns the file \
         when the request has <code>text/markdown</code> or no <code>text/html</code> in \
         <code>Accept</code>. An index of all documents: <a href=\"/llms.txt\">/llms.txt</a>.</p>",
        title = escape_html(&meta.title),
        ct = escape_html(&meta.content_type),
        size = meta.size,
    )
}

/// index.html with the document title, extra head tags and a note for clients that do not
/// run scripts. The page removes the note as soon as it starts.
fn page(
    status: StatusCode,
    cfg: &Config,
    meta: Option<&DocMeta>,
    head: &str,
    note: &str,
) -> HttpResponse {
    let Some(file) = Dist::get("index.html") else {
        return HttpResponse::NotFound().finish();
    };
    let mut html = String::from_utf8_lossy(&file.data).into_owned();

    if let Some(meta) = meta
        && let (Some(open), Some(close)) = (html.find("<title>"), html.find("</title>"))
    {
        let title = format!(
            "{} — {}",
            escape_html(&meta.title),
            escape_html(&cfg.site_title)
        );
        html.replace_range(open + "<title>".len()..close, &title);
    }
    html = html.replacen("</head>", &format!("{head}</head>"), 1);
    html = html.replacen(
        "<body>",
        &format!("<body>\n<section id=\"agent-note\" style=\"{HIDDEN}\">{note}</section>"),
        1,
    );

    HttpResponse::build(status)
        .content_type("text/html; charset=utf-8")
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .insert_header((header::VARY, "Accept"))
        .body(html)
}

// File names under assets contain a content hash, so they can be cached forever.
async fn asset(file: web::Path<String>) -> HttpResponse {
    let path = format!("assets/{file}");
    match Dist::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            HttpResponse::Ok()
                .content_type(mime.as_ref())
                .insert_header((header::CACHE_CONTROL, "public, max-age=31536000, immutable"))
                .body(file.data.into_owned())
        }
        None => HttpResponse::NotFound().finish(),
    }
}
