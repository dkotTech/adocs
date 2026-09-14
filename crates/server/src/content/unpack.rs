use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::archive::{Format, detect_format, read_tar, read_zip};
use super::cache::{BUF_SIZE, VERSION_NAME_LEN, hex, too_big};
use super::index::{Index, assemble};
use crate::paths;

/// An extracted file, before the index is built.
pub(super) struct RawEntry {
    pub(super) path: String,
    pub(super) len: u64,
    pub(super) etag: String,
    pub(super) mtime: Option<i64>,
}

/// Extracts the archive into a version directory and builds the index.
/// Everything is written to `<hash>.tmp` first, and the finished directory gets its name by a single rename.
pub(super) fn build_version(
    archive: &Path,
    cache: &Path,
    hash: &str,
    limit: u64,
) -> Result<Index, String> {
    let short = &hash[..VERSION_NAME_LEN];
    let staging = cache.join(format!("{short}.tmp"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("failed to write to the cache: {e}"))?;

    let result = extract(archive, &staging, limit).and_then(|entries| {
        let final_dir = free_version_dir(cache, short);
        let entries = move_into_place(entries, &staging, &final_dir)?;
        Ok(assemble(entries, final_dir))
    });
    let _ = fs::remove_dir_all(&staging);
    result
}

fn extract(archive: &Path, staging: &Path, limit: u64) -> Result<Vec<RawEntry>, String> {
    let mut file = File::open(archive).map_err(|e| e.to_string())?;
    let mut extractor = Extractor::new(staging, limit);
    match detect_format(&mut file)? {
        Format::Zip => read_zip(file, &mut extractor)?,
        Format::TarGz => read_tar(flate2::read::GzDecoder::new(file), &mut extractor)?,
        Format::Tar => read_tar(file, &mut extractor)?,
    }
    Ok(extractor.entries.into_values().collect())
}

/// A free name for a version directory. A clash is possible when an archive is rolled back
/// while an earlier copy of it is still held by an unfinished download.
fn free_version_dir(cache: &Path, short: &str) -> PathBuf {
    let mut dir = cache.join(short);
    let mut n = 2;
    while dir.exists() {
        dir = cache.join(format!("{short}-{n}"));
        n += 1;
    }
    dir
}

/// `git archive --prefix=repo/` and `tar czf repo.tar.gz repo/` put everything into a single directory.
/// When that is the case it becomes the version directory itself, so paths look like they do in the repository.
fn move_into_place(
    mut entries: Vec<RawEntry>,
    staging: &Path,
    final_dir: &Path,
) -> Result<Vec<RawEntry>, String> {
    let common = entries
        .first()
        .and_then(|e| e.path.split_once('/'))
        .map(|(root, _)| root.to_string())
        .filter(|root| {
            entries
                .iter()
                .all(|e| e.path.split_once('/').is_some_and(|(r, _)| r == root))
        });

    let source = match &common {
        Some(root) => staging.join(root),
        None => staging.to_path_buf(),
    };
    fs::rename(&source, final_dir).map_err(|e| format!("failed to write to the cache: {e}"))?;

    if let Some(root) = common {
        for entry in &mut entries {
            entry.path = entry.path[root.len() + 1..].to_string();
        }
    }
    Ok(entries)
}

/// Writes the archive files to disk. The names are untrusted, so each one is normalized,
/// and files are only created inside a fresh directory that holds no symbolic links at all.
pub(super) struct Extractor<'a> {
    root: &'a Path,
    left: u64,
    limit: u64,
    buf: Vec<u8>,
    entries: HashMap<String, RawEntry>,
}

impl<'a> Extractor<'a> {
    fn new(root: &'a Path, limit: u64) -> Self {
        Extractor {
            root,
            left: limit,
            limit,
            buf: vec![0u8; BUF_SIZE],
            entries: HashMap::new(),
        }
    }

    pub(super) fn add(
        &mut self,
        raw_name: &str,
        mut reader: impl Read,
        mtime: Option<i64>,
    ) -> Result<(), String> {
        // Discarded entries are neither written to disk nor counted against the limit.
        let Some(path) = archive_path(raw_name) else {
            return Ok(());
        };
        if path.split('/').any(|seg| seg.starts_with('.')) {
            return Ok(());
        }

        let target = self.root.join(&path);
        // A clash like "a is a file but a/b needs a directory" spoils one entry, not the whole archive.
        let created = target
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| File::create(&target));
        let file = match created {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("skipped {path}: {e}");
                return Ok(());
            }
        };

        let mut out = BufWriter::new(file);
        let mut hasher = Sha256::new();
        let mut len = 0u64;
        loop {
            // The size declared in the archive is not trusted: the bytes actually read are counted.
            let n = reader
                .read(&mut self.buf)
                .map_err(|e| format!("error reading {path}: {e}"))?;
            if n == 0 {
                break;
            }
            if n as u64 > self.left {
                return Err(too_big(self.limit));
            }
            self.left -= n as u64;
            hasher.update(&self.buf[..n]);
            out.write_all(&self.buf[..n])
                .map_err(|e| format!("error writing to the cache: {e}"))?;
            len += n as u64;
        }
        out.flush()
            .map_err(|e| format!("error writing to the cache: {e}"))?;

        // A tar can carry the same name several times: the last one wins on disk and in the index.
        self.entries.insert(
            path.clone(),
            RawEntry {
                path,
                len,
                etag: format!("\"{}\"", hex(&hasher.finalize())),
                mtime,
            },
        );
        Ok(())
    }
}

/// A path from the archive. Absolute paths and anything escaping upwards via `..` are dropped entirely.
fn archive_path(raw: &str) -> Option<String> {
    if raw.starts_with('/') || raw.starts_with('\\') {
        return None;
    }
    paths::normalize(raw).ok()
}
