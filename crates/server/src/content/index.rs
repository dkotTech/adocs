use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::unpack::RawEntry;
use crate::paths;
use crate::render;
use crate::tree::{self, TreeNode};

/// Only the start of a markdown file is read for its title and summary, so indexing does not
/// depend on file sizes.
const HEAD_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Clone, Debug)]
pub struct DocMeta {
    pub path: String,
    pub title: String,
    /// The first paragraph of a markdown file, for indexes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content_type: String,
    pub size: i64,
    /// The ETag header value, already quoted.
    pub etag: String,
    pub updated_at: Option<DateTime<Utc>>,
}

/// The current documentation version: the in-memory index and the extracted files in the cache directory.
pub struct Index {
    pub docs: Vec<DocMeta>,
    pub tree: TreeNode,
    pub readme: Option<String>,
    pub total_size: i64,
    by_path: HashMap<String, usize>,
    version: Option<VersionDir>,
}

/// The directory holding one version's extracted files.
/// It is removed once the last request has released that version.
struct VersionDir {
    path: PathBuf,
}

impl Drop for VersionDir {
    fn drop(&mut self) {
        // Removing a large tree must not hold up the thread that served the last response.
        let path = std::mem::take(&mut self.path);
        std::thread::spawn(move || {
            let _ = fs::remove_dir_all(path);
        });
    }
}

impl Index {
    pub(super) fn empty() -> Self {
        Index {
            docs: Vec::new(),
            tree: tree::build(&[]),
            readme: None,
            total_size: 0,
            by_path: HashMap::new(),
            version: None,
        }
    }

    pub fn get(&self, path: &str) -> Option<&DocMeta> {
        self.by_path.get(path).map(|i| &self.docs[*i])
    }

    /// The file's path on disk. It is only joined for paths taken from the index,
    /// an arbitrary path from a request never reaches here.
    pub fn file_path(&self, path: &str) -> Option<PathBuf> {
        self.by_path.get(path)?;
        Some(self.version.as_ref()?.path.join(path))
    }

    /// The whole document. Only for small files: large ones are streamed.
    pub fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        Some(fs::read(self.file_path(path)?))
    }
}

impl DocMeta {
    /// One entry in the llms.txt format: `- [Title](link): summary (type, size)`.
    pub fn index_line(&self, link: &str) -> String {
        match &self.summary {
            Some(summary) => format!(
                "- [{}]({link}): {summary} ({}, {} bytes)",
                self.title, self.content_type, self.size
            ),
            None => format!(
                "- [{}]({link}): {}, {} bytes",
                self.title, self.content_type, self.size
            ),
        }
    }
}

/// The start of a markdown file as text. A cut in the middle of a character is dropped;
/// a file that is not UTF-8 gives nothing.
fn markdown_head(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    if let Err(e) = std::str::from_utf8(&buf) {
        if e.error_len().is_some() {
            return None;
        }
        buf.truncate(e.valid_up_to());
    }
    String::from_utf8(buf).ok()
}

pub(super) fn assemble(mut entries: Vec<RawEntry>, dir: PathBuf) -> Index {
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut docs = Vec::with_capacity(entries.len());
    for RawEntry {
        path,
        len,
        etag,
        mtime,
    } in entries
    {
        let content_type = render::detect_content_type(&path);
        let head = render::is_markdown(&content_type, &path)
            .then(|| markdown_head(&dir.join(&path)))
            .flatten();
        let title = head
            .as_deref()
            .and_then(render::markdown_title)
            .unwrap_or_else(|| paths::display_name(&path));
        let summary = head.as_deref().and_then(render::markdown_summary);

        docs.push(DocMeta {
            path,
            title,
            summary,
            content_type,
            size: len as i64,
            etag,
            updated_at: mtime.and_then(|t| DateTime::from_timestamp(t, 0)),
        });
    }

    let by_path = docs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.path.clone(), i))
        .collect();

    let readme = docs
        .iter()
        .find(|d| {
            !d.path.contains('/')
                && matches!(
                    d.path.to_lowercase().as_str(),
                    "readme.md" | "readme.markdown" | "readme"
                )
        })
        .map(|d| d.path.clone());

    Index {
        total_size: docs.iter().map(|d| d.size).sum(),
        tree: tree::build(&docs),
        docs,
        readme,
        by_path,
        version: Some(VersionDir { path: dir }),
    }
}
