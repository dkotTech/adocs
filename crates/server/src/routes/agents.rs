use actix_web::http::header;
use actix_web::{HttpRequest, HttpResponse, web};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write;

use super::file;
use crate::config::Config;
use crate::content::{DocMeta, Store};
use crate::paths;
use crate::render::escape_html;

/// The sitemap protocol allows at most this many addresses in one file.
const MAX_SITEMAP_URLS: usize = 50_000;

/// The absolute address of the service for links in generated indexes. `APP_BASE_URL` wins.
/// Without it the address comes from `Host`; `Forwarded` and `X-Forwarded-*` are read only with
/// `APP_TRUST_FORWARDED`, since a forged header could otherwise end up in a proxy cache.
pub fn base_url(req: &HttpRequest, cfg: &Config) -> String {
    if let Some(url) = &cfg.base_url {
        return url.clone();
    }
    if cfg.trust_forwarded {
        let info = req.connection_info();
        return format!("{}://{}", info.scheme(), info.host());
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(req.app_config().host());
    let scheme = if req.app_config().secure() {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://{host}")
}

/// A generated text with an `ETag` from its own hash. The body depends on the archive, the refresh
/// time, the last error and the base URL, so hashing the result is simpler and safer than tracking
/// every input, and the bodies are small.
pub fn generated(req: &HttpRequest, content_type: &'static str, body: String) -> HttpResponse {
    let digest = Sha256::digest(body.as_bytes());
    let mut etag = String::from("\"");
    for b in &digest[..16] {
        let _ = write!(etag, "{b:02x}");
    }
    etag.push('"');

    if file::etag_matches(req, &etag) {
        return HttpResponse::NotModified()
            .insert_header((header::ETAG, etag))
            .insert_header((header::CACHE_CONTROL, "no-cache"))
            .finish();
    }
    HttpResponse::Ok()
        .content_type(content_type)
        .insert_header((header::ETAG, etag))
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .body(body)
}

// GET /llms.txt - the documentation index for AI agents
pub async fn llms_txt(
    req: HttpRequest,
    cfg: web::Data<Config>,
    store: web::Data<Store>,
) -> HttpResponse {
    let base = base_url(&req, &cfg);
    generated(
        &req,
        "text/plain; charset=utf-8",
        llms_index(&base, &cfg, &store),
    )
}

/// The index in the llms.txt format: a title, a summary, how to read, then files by directory.
pub fn llms_index(base: &str, cfg: &Config, store: &Store) -> String {
    let idx = store.current();
    let status = store.status();
    let mut out = String::new();

    let _ = writeln!(out, "# {}\n", cfg.site_title);
    let _ = writeln!(
        out,
        "> Documentation from the repository archive {}: {} files. \
         Every link below returns the file as is.\n",
        status.archive,
        idx.docs.len()
    );
    if let Some(t) = status.updated_at {
        let _ = writeln!(out, "- Updated: {}", t.format("%Y-%m-%d %H:%M UTC"));
    }
    if let Some(h) = &status.hash {
        let _ = writeln!(out, "- Archive sha256: {h}");
    }
    if let Some(e) = &status.error {
        let _ = writeln!(out, "- Last refresh error: {e}");
    }

    let _ = writeln!(out, "\nHow to read:\n");
    let _ = writeln!(
        out,
        "- `GET {base}/raw/<path>`: the file as is, with `ETag` and `Range` support"
    );
    let _ = writeln!(
        out,
        "- `GET {base}/docs/<path>`: the page a person sees; a request with `text/markdown` \
         or without `text/html` in `Accept` gets the file itself"
    );
    let _ = writeln!(
        out,
        "- `GET {base}/api/docs/<path>`: metadata and markdown rendered to HTML"
    );
    let _ = writeln!(out, "- `GET {base}/api/tree`: the directory tree as JSON");
    let _ = writeln!(
        out,
        "- `GET {base}/api/search?q=<text>`: full-text search in text files as JSON; \
         `format=text` gives `path:line:text` lines, `context=N` adds lines around each match, \
         `path=<dir>` narrows the search, `case=sensitive|insensitive` overrides smart case, \
         `limit` and `per_file` cap the output; regular expressions (`regex=true`) are {}",
        if cfg.search_regex {
            "allowed"
        } else {
            "disabled on this server"
        }
    );
    let _ = writeln!(
        out,
        "- `GET {base}/api/build`: archive state and refresh time"
    );
    let _ = writeln!(
        out,
        "- MCP over Streamable HTTP at `{base}/mcp` with the tools `list_docs`, `read_doc` and \
         `search`; connect with `claude mcp add --transport http adocs {base}/mcp`"
    );

    if idx.docs.is_empty() {
        let _ = writeln!(out, "\nNo documents are loaded.");
        return out;
    }

    // The root comes first, since the empty name sorts before any directory.
    let mut dirs: BTreeMap<&str, Vec<&DocMeta>> = BTreeMap::new();
    for doc in &idx.docs {
        let dir = doc.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        dirs.entry(dir).or_default().push(doc);
    }
    for (dir, docs) in dirs {
        let heading = if dir.is_empty() { "/" } else { dir };
        let _ = writeln!(out, "\n## {heading}\n");
        for doc in docs {
            let link = format!("{base}/raw/{}", paths::encode_url(&doc.path));
            let _ = writeln!(out, "{}", doc.index_line(&link));
        }
    }
    out
}

// GET /robots.txt - everything is allowed, and the sitemap is announced
pub async fn robots_txt(req: HttpRequest, cfg: web::Data<Config>) -> HttpResponse {
    let base = base_url(&req, &cfg);
    HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .body(format!(
            "User-agent: *\nAllow: /\n\nSitemap: {base}/sitemap.xml\n"
        ))
}

// GET /sitemap.xml - document pages for crawlers
pub async fn sitemap(
    req: HttpRequest,
    cfg: web::Data<Config>,
    store: web::Data<Store>,
) -> HttpResponse {
    let base = escape_html(&base_url(&req, &cfg));
    let idx = store.current();
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    let _ = writeln!(out, "<url><loc>{base}/</loc></url>");
    for doc in idx.docs.iter().take(MAX_SITEMAP_URLS - 1) {
        let _ = write!(
            out,
            "<url><loc>{base}/docs/{}</loc>",
            paths::encode_url(&doc.path)
        );
        if let Some(t) = doc.updated_at {
            let _ = write!(out, "<lastmod>{}</lastmod>", t.format("%Y-%m-%d"));
        }
        out.push_str("</url>\n");
    }
    out.push_str("</urlset>\n");
    generated(&req, "application/xml; charset=utf-8", out)
}
