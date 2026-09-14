use std::fmt::Write;

use crate::errors::AppError;

const MAX_PATH_LEN: usize = 1024;

/// Normalizes a document path: `a/b/c.md`.
/// Empty segments and `.` are skipped, `..` is rejected.
/// `tar -C dir .` writes names like `./a.md`, so `.` is not an error.
pub fn normalize(raw: &str) -> Result<String, AppError> {
    let raw = raw.trim().replace('\\', "/");
    let mut parts = Vec::new();
    for part in raw.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(AppError::bad_request("path must not contain .."));
        }
        if part.chars().any(|c| c.is_control()) {
            return Err(AppError::bad_request("path contains control characters"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(AppError::bad_request("empty path"));
    }
    let path = parts.join("/");
    if path.len() > MAX_PATH_LEN {
        return Err(AppError::bad_request("path is too long"));
    }
    Ok(path)
}

/// A document path for a URL: bytes outside the unreserved set are percent-encoded, slashes stay.
/// `form_urlencoded` does not fit: it turns a space into `+`, which a path reads literally.
pub fn encode_url(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// File name without the extension.
pub fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) | None => name,
        Some(i) => &name[..i],
    }
}

/// Default title: the file name without its extension and without a numeric prefix.
/// `01-intro.md` is shown as `intro` but sorted by the original name,
/// so the section order is set straight from the repository.
pub fn display_name(path: &str) -> String {
    strip_order_prefix(file_stem(path)).to_string()
}

fn strip_order_prefix(name: &str) -> &str {
    let digits = name.len() - name.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return name;
    }
    let rest = &name[digits..];
    match rest.strip_prefix(['-', '_', '.', ' ']) {
        // A fully numeric name is kept as is: there would be nothing left to show.
        Some(tail) if !tail.is_empty() => tail,
        _ => name,
    }
}
