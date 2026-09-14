use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

/// Buffer size used when copying and unpacking.
pub(super) const BUF_SIZE: usize = 64 * 1024;

/// Size and modification time of the archive as read.
pub(super) type Stat = (u64, Option<SystemTime>);

/// A temporary file removed when it goes out of scope.
pub(super) struct TempPath(pub(super) PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The archive copy kept while it is checked. Refreshes run one at a time, so the name is fixed.
pub(super) const INCOMING: &str = "incoming.archive";
/// How many characters of the archive hash go into a version directory name.
pub(super) const VERSION_NAME_LEN: usize = 12;

/// A version directory name: the start of the hash, with the suffix `-2`, `-3` when it clashes
/// with a version still in use, and the `.tmp` ending while extraction is unfinished.
fn is_version_name(name: &str) -> bool {
    let name = name.strip_suffix(".tmp").unwrap_or(name);
    let (hash, suffix) = name.split_once('-').unwrap_or((name, "1"));
    hash.len() == VERSION_NAME_LEN
        && hash.chars().all(|c| c.is_ascii_hexdigit())
        && !suffix.is_empty()
        && suffix.chars().all(|c| c.is_ascii_digit())
}

/// Leftovers from previous runs. Files in the cache directory that are not ours are left alone.
pub(super) fn clean_cache(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if name == INCOMING {
            let _ = fs::remove_file(path);
        } else if is_version_name(&name) && path.is_dir() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

pub(super) fn stat_of(path: &Path) -> Result<Stat, String> {
    let meta = fs::metadata(path)
        .map_err(|e| format!("archive {} is unavailable: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    Ok((meta.len(), meta.modified().ok()))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

pub(super) fn too_big(limit: u64) -> String {
    format!(
        "the archive or its contents exceed {} MB, raise ADOCS_MAX_SIZE_MB",
        limit / 1024 / 1024
    )
}

/// Copies the archive while computing its sha256.
pub(super) fn copy_hashing(src: &Path, dst: &Path, limit: u64) -> Result<String, String> {
    let mut input =
        File::open(src).map_err(|e| format!("failed to open {}: {e}", src.display()))?;
    let mut out = BufWriter::new(
        File::create(dst).map_err(|e| format!("failed to write to the cache: {e}"))?,
    );
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];
    let mut total = 0u64;
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| format!("error reading the archive: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(too_big(limit));
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| format!("error writing to the cache: {e}"))?;
    }
    out.flush()
        .map_err(|e| format!("error writing to the cache: {e}"))?;
    Ok(hex(&hasher.finalize()))
}
