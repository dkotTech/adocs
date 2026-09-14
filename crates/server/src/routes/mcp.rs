use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{HttpRequest, HttpResponse, web};
use futures_util::StreamExt;
use http_body_util::{BodyStream, Full};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::config::Config;
use crate::content::Store;
use crate::mcp::Docs;

pub type Service = StreamableHttpService<Docs, NeverSessionManager>;

/// The MCP service without sessions: every POST is answered on its own with plain JSON, so replicas
/// and restarts need nothing shared. rmcp's own Host check is off: it would guard only `/mcp`, while
/// DNS rebinding reaches `/raw` and `/api` just as well. The whole application checks `Host` against
/// `APP_ALLOWED_HOSTS` instead (`routes::check_host`).
pub fn service(cfg: web::Data<Config>, store: web::Data<Store>) -> Service {
    let mut config = StreamableHttpServerConfig::default().disable_allowed_hosts();
    config.legacy_session_mode = false;
    config.json_response = true;
    StreamableHttpService::new(
        move || Ok(Docs::new(cfg.clone(), store.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    )
}

// /mcp - MCP over Streamable HTTP. rmcp works on `http` 1.x types, so the request and the response
// are carried over from actix by hand; the response body stays a stream.
pub async fn handle(
    req: HttpRequest,
    body: web::Bytes,
    service: web::Data<Service>,
) -> HttpResponse {
    let mut request = http::Request::builder()
        .method(req.method().as_str())
        .uri(req.uri().to_string());
    for (name, value) in req.headers() {
        request = request.header(name.as_str(), value.as_bytes());
    }
    let Ok(request) = request.body(Full::new(body)) else {
        return HttpResponse::BadRequest().finish();
    };

    let (parts, body) = service.handle(request).await.into_parts();
    let status =
        StatusCode::from_u16(parts.status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut resp = HttpResponse::build(status);
    for (name, value) in &parts.headers {
        // actix sets the length and the transfer encoding for the stream itself.
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            resp.append_header((name, value));
        }
    }

    let frames = BodyStream::new(body).filter_map(|frame| async move {
        match frame {
            Ok(frame) => frame
                .into_data()
                .ok()
                .map(Ok::<_, std::convert::Infallible>),
            Err(never) => match never {},
        }
    });
    resp.streaming(frames)
}
