use actix_web::http::{Method, header};
use actix_web::{HttpRequest, HttpResponse, web};
use std::io::{self, Write};
use tokio::sync::Semaphore;
use tokio::sync::mpsc;

use crate::content::Store;
use crate::errors::AppError;
use crate::pack;
use crate::paths;

/// Deflating takes a core for as long as the download runs; more than this at once wait or get 429.
static SLOTS: Semaphore = Semaphore::const_new(2);
/// What one message to the response carries; the channel holds a few of them.
const CHUNK: usize = 64 * 1024;
const QUEUE: usize = 4;

// GET /zip/{dir} - the directory and everything under it as one zip file
pub async fn zip(
    req: HttpRequest,
    store: web::Data<Store>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let dir = paths::normalize(&path)?;
    let index = store.current();
    let (files, bytes) = index
        .in_dir(&dir)
        .fold((0, 0), |(n, total), doc| (n + 1, total + doc.size));
    if files == 0 {
        return Err(AppError::NotFound);
    }
    if files > pack::MAX_FILES || bytes > pack::MAX_BYTES {
        return Err(AppError::TooLarge(
            "the directory is too large for one zip file".into(),
        ));
    }

    let name = format!("{}.zip", dir.rsplit('/').next().unwrap_or(&dir));
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-._".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut resp = HttpResponse::Ok();
    resp.content_type("application/zip")
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
                paths::encode_url(&name)
            ),
        ))
        .insert_header((header::CACHE_CONTROL, "no-cache"));

    // The length is not known before the files are deflated, so HEAD gets the headers only.
    if req.method() == Method::HEAD {
        return Ok(resp.finish());
    }

    let permit = SLOTS
        .try_acquire()
        .map_err(|_| AppError::TooManyDownloads)?;
    let (tx, rx) = mpsc::channel::<io::Result<web::Bytes>>(QUEUE);

    // The thread holds the version, so the files stay until the archive is written out. It ends
    // early when the browser goes away: the channel closes and the next write fails.
    std::thread::spawn(move || {
        let _permit = permit;
        let mut pipe = Pipe {
            tx: tx.clone(),
            buf: Vec::with_capacity(CHUNK),
        };
        if let Err(e) = pack::write(&index, &dir, &mut pipe).and_then(|_| pipe.flush())
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            tracing::warn!("zip of {dir}: {e}");
            let _ = tx.blocking_send(Err(e));
        }
    });

    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Ok(resp.streaming(body))
}

/// A `Write` that hands chunks to the response. A closed channel means the client is gone.
struct Pipe {
    tx: mpsc::Sender<io::Result<web::Bytes>>,
    buf: Vec<u8>,
}

impl Pipe {
    fn send(&mut self) -> io::Result<()> {
        let chunk = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK));
        self.tx
            .blocking_send(Ok(web::Bytes::from(chunk)))
            .map_err(|_| io::ErrorKind::BrokenPipe.into())
    }
}

impl Write for Pipe {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= CHUNK {
            self.send()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.send()?;
        }
        Ok(())
    }
}
