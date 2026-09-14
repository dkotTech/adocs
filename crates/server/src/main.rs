use actix_web::{App, HttpServer, middleware, web};
use tracing_subscriber::EnvFilter;

mod config;
mod content;
mod errors;
mod mcp;
mod paths;
mod render;
mod routes;
mod search;
mod tree;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // A .env parse error is not swallowed: otherwise lines after a broken one are silently lost.
    if let Err(e) = dotenvy::dotenv()
        && !e.not_found()
    {
        fail(&format!(
            "failed to parse .env: {e}. Quote values that contain spaces"
        ));
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cfg = config::Config::from_env().unwrap_or_else(|e| fail(&e));
    let store = content::Store::new(&cfg).unwrap_or_else(|e| fail(&e));

    tracing::info!(
        "archive: {}, cache: {}",
        cfg.data_dir.join(&cfg.archive).display(),
        cfg.cache_dir.display()
    );

    tracing::info!(
        "search: regular expressions {}",
        if cfg.search_regex {
            "allowed"
        } else {
            "disabled"
        }
    );

    if cfg.base_url.is_none() {
        tracing::warn!(
            "APP_BASE_URL is not set: links in llms.txt and sitemap.xml take the address from the \
             request {}; set it in production",
            if cfg.trust_forwarded {
                "Host and X-Forwarded-* headers"
            } else {
                "Host header"
            }
        );
    }

    if cfg.allowed_hosts.is_empty() {
        tracing::info!("host check: off, any Host is answered (APP_ALLOWED_HOSTS)");
    } else {
        tracing::info!("host check: only {}", cfg.allowed_hosts.join(", "));
    }

    // An unavailable archive does not block startup: the storage may not be mounted yet.
    // Once the archive shows up, the "Refresh" button picks it up.
    let started = std::time::Instant::now();
    match store.refresh() {
        (content::Outcome::Updated, msg) => {
            tracing::info!("{msg} in {} ms", started.elapsed().as_millis())
        }
        (_, msg) => tracing::warn!("archive not loaded: {msg}"),
    }

    let host = cfg.host.clone();
    let port = cfg.port;
    let cfg = web::Data::new(cfg);
    let store = web::Data::new(store);
    let mcp = web::Data::new(routes::mcp::service(cfg.clone(), store.clone()));

    tracing::info!("Starting server on {host}:{port}");

    HttpServer::new(move || {
        App::new()
            .app_data(cfg.clone())
            .app_data(store.clone())
            .app_data(mcp.clone())
            // Malformed query parameters answer with the same JSON error as everything else.
            .app_data(
                web::QueryConfig::default()
                    .error_handler(|err, _| errors::AppError::bad_request(err.to_string()).into()),
            )
            .wrap(middleware::from_fn(routes::check_host))
            .wrap(middleware::Logger::default())
            .configure(routes::configure)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}

/// A clear startup error without a panic backtrace.
fn fail(msg: &str) -> ! {
    eprintln!("adocs: {msg}");
    std::process::exit(2);
}
