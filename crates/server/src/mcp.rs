//! MCP tools for agents: list the files, read one by lines, search the text.
//! The HTTP transport is in `routes/mcp.rs`.

use std::fmt::Write;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use actix_web::web;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::Config;
use crate::content::Store;
use crate::paths;
use crate::search::{self, Failure};

/// Entries in one `list_docs` answer.
const MAX_LIST: usize = 2000;
const DEFAULT_READ_LINES: usize = 500;
const MAX_READ_LINES: usize = 2000;
/// Bytes kept from one line; the rest of a longer line is skipped without being stored.
const MAX_LINE_BYTES: usize = 2000;
/// Text in one `read_doc` answer.
const MAX_READ_BYTES: usize = 256 * 1024;
/// A NUL byte this close to the start marks a binary file.
const BINARY_PROBE: usize = 8192;

#[derive(Clone)]
pub struct Docs {
    cfg: web::Data<Config>,
    store: web::Data<Store>,
}

impl Docs {
    pub fn new(cfg: web::Data<Config>, store: web::Data<Store>) -> Self {
        Docs { cfg, store }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListDocsParams {
    /// Directory relative to the repository root, for example `guide`. All files when omitted.
    pub dir: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadDocParams {
    /// File path relative to the repository root, as `list_docs` and `search` show it.
    pub path: String,
    /// Line to start from, counting from 1. Default 1.
    pub offset: Option<u64>,
    /// How many lines to return. Default 500, at most 2000.
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Text to find, at least 2 characters. A plain string with smart case: lowercase matches any case.
    pub q: String,
    /// Only this file or directory.
    pub path: Option<String>,
    /// Lines to show before and after each match, up to 5.
    pub context: Option<usize>,
    /// Matching lines in the answer. Default 100, at most 1000.
    pub limit: Option<usize>,
}

fn text(s: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::text(s)]))
}

/// A tool error the agent reads and acts on, rather than a protocol error.
fn failed(s: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(s)]))
}

#[tool_router]
impl Docs {
    #[tool(
        description = "List documentation files as `- [Title](path): summary (type, size)`. \
                       Pass `dir` to list one directory.",
        annotations(read_only_hint = true)
    )]
    async fn list_docs(
        &self,
        Parameters(p): Parameters<ListDocsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let index = self.store.current();
        let dir = match p
            .dir
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty() && *d != "/")
        {
            Some(d) => match paths::normalize(d) {
                Ok(d) => Some(d),
                Err(e) => return failed(e.to_string()),
            },
            None => None,
        };
        let prefix = dir.as_ref().map(|d| format!("{d}/"));
        let docs: Vec<_> = index
            .docs
            .iter()
            .filter(|doc| prefix.as_ref().is_none_or(|p| doc.path.starts_with(p)))
            .collect();
        if docs.is_empty() {
            return failed(match &dir {
                Some(d) => format!("no files under {d}"),
                None => "no documents are loaded".into(),
            });
        }

        let mut out = String::new();
        match &dir {
            Some(d) => writeln!(out, "{} files under {d}", docs.len()),
            None => writeln!(out, "{} files", docs.len()),
        }
        .ok();
        for doc in docs.iter().take(MAX_LIST) {
            let _ = writeln!(out, "{}", doc.index_line(&doc.path));
        }
        if docs.len() > MAX_LIST {
            let _ = writeln!(
                out,
                "... and {} more, pass dir to list a smaller part",
                docs.len() - MAX_LIST
            );
        }
        text(out)
    }

    #[tool(
        description = "Read a text file by lines, numbered. For a long file pass `offset` (the line \
                       to start from) and `limit`; the answer ends with where to continue. Line \
                       numbers match those in search results.",
        annotations(read_only_hint = true)
    )]
    async fn read_doc(
        &self,
        Parameters(p): Parameters<ReadDocParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let index = self.store.current();
        let path = match paths::normalize(&p.path) {
            Ok(path) => path,
            Err(e) => return failed(e.to_string()),
        };
        let (Some(meta), Some(file)) = (index.get(&path), index.file_path(&path)) else {
            return failed(format!(
                "{path} is not found; list_docs and search show the existing paths"
            ));
        };
        let binary = format!(
            "{path} is a binary file; its content is at /raw/{}",
            paths::encode_url(&path)
        );
        // Types known to be binary (PDF, images) are refused before reading: their text form, such
        // as PDF objects, would cost tokens and look like content. SVG stays readable as markup, and a
        // file of unknown type (Dockerfile, LICENSE) is left to the NUL check.
        let ct = meta.content_type.as_str();
        if !(search::is_searchable(meta)
            || ct.starts_with("image/svg")
            || ct.starts_with("application/octet-stream"))
        {
            return failed(binary);
        }
        let offset = p.offset.unwrap_or(1).max(1);
        let limit = p
            .limit
            .unwrap_or(DEFAULT_READ_LINES)
            .clamp(1, MAX_READ_LINES);

        // The read holds its version, so a refresh in the middle cannot remove the file.
        let read = tokio::task::spawn_blocking(move || {
            let _index = index;
            read_lines(&file, offset, limit)
        })
        .await;
        match read {
            Ok(Ok(Some(out))) => text(out),
            Ok(Ok(None)) => failed(binary),
            Ok(Err(e)) => failed(format!("failed to read {path}: {e}")),
            Err(_) => Err(ErrorData::internal_error("read interrupted", None)),
        }
    }

    #[tool(
        description = "Full-text search in the text files, like ripgrep. Returns `path:line:text` \
                       for matches and `path-line-text` for context lines; lines starting with `#` \
                       are notes.",
        annotations(read_only_hint = true)
    )]
    async fn search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = search::Request {
            query: p.q,
            regex: false,
            case: None,
            path: p.path,
            context: p.context,
            limit: p.limit,
            per_file: None,
        };
        let params = match request.validate(self.cfg.search_regex) {
            Ok(params) => params,
            Err(e) => return failed(e),
        };
        match search::search(self.store.current(), params).await {
            Ok(results) => text(search::to_text(&results)),
            Err(Failure::Busy) => failed("too many searches at once, try again in a moment"),
            Err(Failure::Invalid(msg)) => failed(msg),
            Err(Failure::Internal) => Err(ErrorData::internal_error("search interrupted", None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for Docs {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("adocs", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "{}: documentation of a repository. Find files with search or list_docs, then \
                 read them with read_doc; line numbers in search results are read_doc offsets.",
                self.cfg.site_title
            ))
    }
}

/// Lines `offset..offset + limit` of a text file, numbered like `cat -n`, with a note on where to
/// continue. None for a binary file. Memory stays within the limits whatever the file size.
fn read_lines(file: &Path, offset: u64, limit: usize) -> io::Result<Option<String>> {
    let mut reader = BufReader::new(File::open(file)?);
    if reader
        .fill_buf()?
        .iter()
        .take(BINARY_PROBE)
        .any(|&b| b == 0)
    {
        return Ok(None);
    }

    // Lines before the offset are skipped without being kept.
    let mut line_no = 0u64;
    while line_no + 1 < offset && read_line(&mut reader, 0)?.is_some() {
        line_no += 1;
    }

    let mut out = String::new();
    let mut shown = 0;
    while shown < limit && out.len() < MAX_READ_BYTES {
        let Some((bytes, cut)) = read_line(&mut reader, MAX_LINE_BYTES)? else {
            break;
        };
        line_no += 1;
        shown += 1;
        let bytes = if cut {
            &bytes[..]
        } else {
            bytes.strip_suffix(b"\r").unwrap_or(&bytes)
        };
        let note = if cut { " [line cut]" } else { "" };
        let _ = writeln!(
            out,
            "{line_no:>6}\t{}{note}",
            String::from_utf8_lossy(bytes)
        );
    }

    if shown == 0 {
        return Ok(Some(match line_no {
            0 => "[the file is empty]".to_string(),
            n => format!("[the file has {n} lines; offset {offset} is past the end]"),
        }));
    }
    let first = line_no + 1 - shown as u64;
    if read_line(&mut reader, 0)?.is_some() {
        let _ = write!(
            out,
            "[lines {first}-{line_no}; more follows, continue with offset={}]",
            line_no + 1
        );
    } else {
        let _ = write!(out, "[lines {first}-{line_no}, end of file]");
    }
    Ok(Some(out))
}

/// Reads one line and keeps at most `max` bytes of it; the rest is skipped without being stored.
/// Returns None at the end of the file, otherwise the kept bytes and whether the line was cut.
fn read_line(reader: &mut impl BufRead, max: usize) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut kept = Vec::new();
    let mut cut = false;
    let mut started = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(started.then_some((kept, cut)));
        }
        started = true;
        let newline = buf.iter().position(|&b| b == b'\n');
        let part = &buf[..newline.unwrap_or(buf.len())];
        let room = max.saturating_sub(kept.len());
        cut |= part.len() > room;
        kept.extend_from_slice(&part[..part.len().min(room)]);
        let used = newline.map_or(buf.len(), |i| i + 1);
        reader.consume(used);
        if newline.is_some() {
            return Ok(Some((kept, cut)));
        }
    }
}
