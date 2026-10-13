//! Prometheus metrics on `/metrics`. The process metrics (CPU, memory, file descriptors, threads)
//! come with the crate's `process` feature and are registered on the default registry by it.

use std::sync::LazyLock;
use std::time::Instant;

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{HttpResponse, web};
use prometheus::{
    HistogramVec, IntCounterVec, IntGauge, TextEncoder, register_histogram_vec,
    register_int_counter_vec, register_int_gauge,
};

use crate::content::Store;

static REQUESTS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "adocs_http_requests_total",
        "Requests by route pattern, method and status",
        &["route", "method", "status"]
    )
    .unwrap()
});

static DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        "adocs_http_request_duration_seconds",
        "Time to the last byte of the response by route pattern",
        &["route"],
        vec![0.005, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]
    )
    .unwrap()
});

static DOCUMENTS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!("adocs_documents", "Documents in the loaded archive").unwrap()
});
static CONTENT_BYTES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "adocs_content_bytes",
        "Size of the documents in the loaded archive"
    )
    .unwrap()
});
static UPDATED_AT: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "adocs_content_updated_timestamp_seconds",
        "When the loaded archive was read, 0 when none is loaded"
    )
    .unwrap()
});
static REFRESH_ERROR: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "adocs_refresh_error",
        "1 when the last refresh failed and the previous version keeps serving"
    )
    .unwrap()
});

/// Counts every request by the matched route pattern, so a document path does not become a label.
pub async fn observe(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let started = Instant::now();
    let method = req.method().to_string();
    let route = req.match_pattern().unwrap_or_else(|| "unmatched".into());
    let res = next.call(req).await;
    let status = match &res {
        Ok(res) => res.status(),
        Err(e) => e.as_response_error().status_code(),
    };
    REQUESTS
        .with_label_values(&[&route, &method, status.as_str()])
        .inc();
    DURATION
        .with_label_values(&[&route])
        .observe(started.elapsed().as_secs_f64());
    res
}

// GET /metrics - in the Prometheus text format
pub async fn metrics(store: web::Data<Store>) -> HttpResponse {
    let status = store.status();
    DOCUMENTS.set(status.documents as i64);
    CONTENT_BYTES.set(status.total_size);
    UPDATED_AT.set(status.updated_at.map_or(0, |t| t.timestamp()));
    REFRESH_ERROR.set(status.error.is_some() as i64);

    match TextEncoder::new().encode_to_string(&prometheus::gather()) {
        Ok(body) => HttpResponse::Ok()
            .content_type("text/plain; version=0.0.4; charset=utf-8")
            .body(body),
        Err(e) => {
            tracing::error!("metrics: {e}");
            HttpResponse::InternalServerError().finish()
        }
    }
}
