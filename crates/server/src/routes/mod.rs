use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{Route, guard, http::header, web};

use crate::config::{self, Config};
use crate::errors::AppError;

mod agents;
mod docs;
mod file;
mod frontend;
mod health;
pub mod mcp;
mod search;

/// Answers only to the host names in `APP_ALLOWED_HOSTS`, when the list is set. Behind access control,
/// a malicious page could otherwise read the documentation through DNS rebinding in an employee's
/// browser: the request would carry the attacker's host name. The liveness check stays open for
/// probes that come by IP address.
pub async fn check_host(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let allowed = req
        .app_data::<web::Data<Config>>()
        .map(|cfg| cfg.allowed_hosts.as_slice())
        .unwrap_or_default();
    if !allowed.is_empty() && req.path() != "/pub/health" {
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|h| config::host_name(h.trim()).to_ascii_lowercase());
        if !host.is_some_and(|h| allowed.contains(&h)) {
            return Err(AppError::UnknownHost.into());
        }
    }
    next.call(req).await
}

/// GET and HEAD: some crawlers and agents probe an address with HEAD first.
fn read() -> Route {
    web::route().guard(guard::Any(guard::Get()).or(guard::Head()))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/pub/health", read().to(health::health))
        // Standard entry points for AI agents and crawlers.
        .route("/llms.txt", read().to(agents::llms_txt))
        .route("/robots.txt", read().to(agents::robots_txt))
        .route("/sitemap.xml", read().to(agents::sitemap))
        // MCP over Streamable HTTP. Every method goes to rmcp, which answers GET and DELETE with 405.
        .route("/mcp", web::route().to(mcp::handle))
        // Raw document content.
        .route("/raw/{path:.*}", read().to(docs::raw))
        .service(
            web::scope("/api")
                .route("/tree", read().to(docs::tree))
                .route("/build", read().to(docs::build))
                .route("/refresh", web::post().to(docs::refresh))
                .route("/docs/{path:.*}", read().to(docs::get))
                .route("/search", read().to(search::search)),
        )
        .configure(frontend::configure);
}
