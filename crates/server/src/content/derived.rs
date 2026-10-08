//! Results computed from a document and kept beside the version they belong to.
//!
//! The directory lives inside the version directory, so it dies together with that version and
//! nothing in it can go stale: a new archive means a new version and an empty cache. The name
//! starts with a dot, and the extractor writes no hidden entries, so no document from an archive
//! can ever collide with it or be served from it.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::cache::hex;

const DIR: &str = ".derived";
/// How many characters of the key hash name the file.
const KEY_LEN: usize = 24;

/// The file name for a key. The key may be any text, the name is always a safe one.
pub fn key(kind: &str, value: &str) -> String {
    let digest = Sha256::digest(format!("{kind}\0{value}").as_bytes());
    hex(&digest)[..KEY_LEN].to_string()
}

/// Where a value lies. Public because a value can be large enough that it is better searched on
/// disk than read into memory.
pub fn path(version: &Path, key: &str) -> PathBuf {
    version.join(DIR).join(key)
}

pub fn get(version: &Path, key: &str) -> Option<Vec<u8>> {
    fs::read(path(version, key)).ok()
}

/// Written through a temporary file and a rename: a reader never sees a half-written value.
/// A failure only means the work is done again next time, so errors are not worth reporting.
pub fn put(version: &Path, key: &str, value: &[u8]) {
    let path = path(version, key);
    let Some(dir) = path.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, value).is_ok() && fs::rename(&tmp, &path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}
