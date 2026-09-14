use actix_web::body::SizedStream;
use actix_web::http::{Method, StatusCode, header};
use actix_web::{HttpRequest, HttpResponse, web};
use std::io;
use std::sync::Arc;

use crate::content::{DocMeta, Index};
use crate::errors::AppError;

/// Chunk size when streaming: per-request memory does not depend on the file size.
const CHUNK: u64 = 256 * 1024;

/// Serves a file from the index as is: `ETag` and 304, a single byte range,
/// and HEAD without reading the file.
pub async fn serve(
    req: &HttpRequest,
    index: Arc<Index>,
    meta: &DocMeta,
) -> Result<HttpResponse, AppError> {
    let len = meta.size as u64;

    // The ETag is a hash of the file content, so a match means the very same version.
    if etag_matches(req, &meta.etag) {
        return Ok(HttpResponse::NotModified()
            .insert_header((header::ETAG, meta.etag.clone()))
            .finish());
    }

    // If-Range with another ETag means the client holds a different version: send the whole file.
    let range = match header_str(req, header::IF_RANGE) {
        Some(tag) if tag.trim() != meta.etag => None,
        _ => header_str(req, header::RANGE),
    };
    let (status, start, end) = match parse_range(range, len) {
        Range::Full => (StatusCode::OK, 0, len),
        Range::Part(start, end) => (StatusCode::PARTIAL_CONTENT, start, end),
        Range::Unsatisfiable => {
            return Ok(HttpResponse::RangeNotSatisfiable()
                .insert_header((header::CONTENT_RANGE, format!("bytes */{len}")))
                .insert_header((header::ACCEPT_RANGES, "bytes"))
                .finish());
        }
    };
    let count = end - start;

    let mut content_type = meta.content_type.clone();
    if content_type.starts_with("text/") && !content_type.contains("charset") {
        content_type.push_str("; charset=utf-8");
    }

    let mut resp = HttpResponse::build(status);
    resp.content_type(content_type)
        .insert_header((header::ETAG, meta.etag.clone()))
        .insert_header((header::ACCEPT_RANGES, "bytes"))
        .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .insert_header((header::CACHE_CONTROL, "no-cache"));
    if status == StatusCode::PARTIAL_CONTENT {
        resp.insert_header((
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", end - 1),
        ));
    }

    // HTML and SVG run in an opaque origin: their scripts get no access to the site.
    if meta.content_type.starts_with("text/html") || meta.content_type.starts_with("image/svg") {
        resp.insert_header((
            header::CONTENT_SECURITY_POLICY,
            "sandbox allow-scripts allow-forms allow-popups allow-modals",
        ));
    }

    // HEAD gets the same headers, including the length, and the file is not read.
    if req.method() == Method::HEAD {
        let empty = futures_util::stream::empty::<io::Result<web::Bytes>>();
        return Ok(resp.body(SizedStream::new(count, empty)));
    }

    let file_path = index.file_path(&meta.path).ok_or(AppError::NotFound)?;
    let file = web::block(move || std::fs::File::open(file_path))
        .await
        .map_err(|_| AppError::Internal)?
        .map_err(|_| AppError::Internal)?;

    Ok(resp.body(SizedStream::new(
        count,
        stream_file(index, file, start, count),
    )))
}

/// Whether `If-None-Match` names this ETag, so the client already holds the same content.
pub fn etag_matches(req: &HttpRequest, etag: &str) -> bool {
    header_str(req, header::IF_NONE_MATCH).is_some_and(|sent| {
        sent.split(',')
            .any(|tag| tag.trim() == etag || tag.trim() == "*")
    })
}

fn header_str(req: &HttpRequest, name: header::HeaderName) -> Option<&str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

#[derive(Debug, PartialEq)]
enum Range {
    Full,
    /// From the first byte inclusive to the second exclusive.
    Part(u64, u64),
    Unsatisfiable,
}

/// A single range `bytes=a-b`, `bytes=a-` or `bytes=-n`. Several ranges at once, other units
/// and a malformed header are ignored and the whole file is sent, which the standard allows.
fn parse_range(value: Option<&str>, len: u64) -> Range {
    let Some(spec) = value.and_then(|v| v.trim().strip_prefix("bytes=")) else {
        return Range::Full;
    };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return Range::Full;
    };
    let (first, last) = (first.trim(), last.trim());

    if first.is_empty() {
        // A suffix: the last n bytes.
        return match last.parse::<u64>() {
            Ok(0) => Range::Unsatisfiable,
            Ok(_) if len == 0 => Range::Unsatisfiable,
            Ok(n) => Range::Part(len.saturating_sub(n), len),
            Err(_) => Range::Full,
        };
    }

    let Ok(start) = first.parse::<u64>() else {
        return Range::Full;
    };
    let end = if last.is_empty() {
        len
    } else {
        match last.parse::<u64>() {
            Ok(last) if last >= start => last.saturating_add(1).min(len),
            _ => return Range::Full,
        }
    };
    if start >= len {
        Range::Unsatisfiable
    } else {
        Range::Part(start, end)
    }
}

/// Streams `count` bytes of a file starting at `start`. The stream holds its own version
/// of the index, so refreshing the archive mid-download will not remove the file until
/// the transfer is done.
fn stream_file(
    index: Arc<Index>,
    file: std::fs::File,
    start: u64,
    count: u64,
) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = io::Result<web::Bytes>>>> {
    use std::os::unix::fs::FileExt;
    let file = Arc::new(file);
    Box::pin(futures_util::stream::unfold(
        (index, file, start, count),
        |(index, file, pos, left)| async move {
            if left == 0 {
                return None;
            }
            let n = left.min(CHUNK);
            let reader = file.clone();
            let item = match web::block(move || {
                let mut buf = vec![0u8; n as usize];
                reader.read_exact_at(&mut buf, pos).map(|_| buf)
            })
            .await
            {
                Ok(Ok(buf)) => Ok(web::Bytes::from(buf)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(io::Error::other("read interrupted")),
            };
            // The stream ends after an error.
            let rest = if item.is_ok() { left - n } else { 0 };
            Some((item, (index, file, pos + n, rest)))
        },
    ))
}
