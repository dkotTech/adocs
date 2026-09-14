//! Full-text search over the current version's files with the ripgrep libraries.
//! Every query scans the extracted files, so there is no index to build or to go stale.

use std::fmt::Write;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{
    BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkContextKind, SinkMatch,
};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::content::{DocMeta, Index};
use crate::paths;
use crate::render;

/// A search stops after this long and returns what it has found so far.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Limit on a compiled pattern, so a crafted regular expression cannot eat memory.
const REGEX_SIZE_LIMIT: usize = 10 * 1024 * 1024;
/// The longest line the searcher buffers. A file with a longer line is skipped,
/// so memory does not depend on the file.
const MAX_LINE_BUFFER: usize = 8 * 1024 * 1024;
/// Lines in the response are cut to this many characters around the first match.
const MAX_LINE_CHARS: usize = 400;
/// Characters kept before the first match when a line is cut.
const LEAD_CHARS: usize = 80;
/// Highlighted ranges per line.
const MAX_RANGES: usize = 50;

/// Searches running at once, each on its own thread.
static SLOTS: Semaphore = Semaphore::const_new(4);
/// How long a search waits for a free slot. Waiting is cheap, so a burst of parallel calls
/// from an agent queues up instead of being refused.
const SLOT_WAIT: Duration = Duration::from_secs(2);

pub const MIN_QUERY_CHARS: usize = 2;
const MAX_QUERY_CHARS: usize = 1000;
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const DEFAULT_PER_FILE: usize = 20;
const MAX_CONTEXT: usize = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Case {
    /// Case-insensitive unless the query has an uppercase letter, like ripgrep's `--smart-case`.
    Smart,
    Sensitive,
    Insensitive,
}

pub struct Params {
    pub query: String,
    pub regex: bool,
    pub case: Case,
    /// Only the file at this path or files inside this directory.
    pub path: Option<String>,
    pub context: usize,
    /// Matching lines in the whole response.
    pub limit: usize,
    /// Matching lines per file. The file's `count` still covers all of them.
    pub per_file: usize,
}

/// A search as it comes from a client, before checks and defaults.
pub struct Request {
    pub query: String,
    pub regex: bool,
    pub case: Option<String>,
    pub path: Option<String>,
    pub context: Option<usize>,
    pub limit: Option<usize>,
    pub per_file: Option<usize>,
}

impl Request {
    /// Checks the request and fills in defaults, the same way for the HTTP API and MCP.
    pub fn validate(self, regex_allowed: bool) -> Result<Params, String> {
        if self.query.trim().chars().count() < MIN_QUERY_CHARS {
            return Err(format!(
                "the query needs at least {MIN_QUERY_CHARS} characters"
            ));
        }
        if self.query.chars().count() > MAX_QUERY_CHARS {
            return Err(format!(
                "the query is longer than {MAX_QUERY_CHARS} characters"
            ));
        }
        if self.regex && !regex_allowed {
            return Err("regular expressions are disabled on this server".into());
        }
        let case = match self.case.as_deref() {
            None | Some("smart") => Case::Smart,
            Some("sensitive") => Case::Sensitive,
            Some("insensitive") => Case::Insensitive,
            Some(other) => {
                return Err(format!(
                    "unknown case {other:?}: use smart, sensitive or insensitive"
                ));
            }
        };
        let path = match self
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            Some(p) => Some(paths::normalize(p).map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(Params {
            query: self.query,
            regex: self.regex,
            case,
            path,
            context: self.context.unwrap_or(0).min(MAX_CONTEXT),
            limit: self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
            per_file: self
                .per_file
                .unwrap_or(DEFAULT_PER_FILE)
                .clamp(1, MAX_LIMIT),
        })
    }
}

pub enum Failure {
    /// No slot freed up in time.
    Busy,
    /// The pattern does not compile.
    Invalid(String),
    Internal,
}

/// Waits for a slot and runs the search on a blocking thread. The search holds its own version
/// of the index, so a refresh in the middle does not remove its files.
pub async fn search(index: Arc<Index>, params: Params) -> Result<Results, Failure> {
    let Ok(Ok(permit)) = tokio::time::timeout(SLOT_WAIT, SLOTS.acquire()).await else {
        return Err(Failure::Busy);
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        run(&index, &params)
    })
    .await
    .map_err(|_| Failure::Internal)?
    .map_err(Failure::Invalid)
}

/// Text files only: markdown and text types, without `application/octet-stream` (files of an
/// unknown type) and without images. A file that still turns out binary is skipped by the NUL check.
pub fn is_searchable(doc: &DocMeta) -> bool {
    let ct = doc.content_type.as_str();
    (render::is_markdown(ct, &doc.path) || render::is_text(ct))
        && !ct.starts_with("application/octet-stream")
        && !ct.starts_with("image/")
}

#[derive(Serialize)]
pub struct Results {
    pub files: Vec<FileHits>,
    /// Matching lines found, including those beyond `per_file`.
    pub total_matches: usize,
    /// Text files looked at.
    pub files_searched: usize,
    /// The limit or the timeout was reached: there may be more matches.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Serialize)]
pub struct FileHits {
    pub path: String,
    pub title: String,
    pub count: usize,
    pub matches: Vec<Hit>,
}

#[derive(Serialize)]
pub struct Hit {
    pub line: u64,
    pub text: String,
    /// Match positions in `text` as character offsets, `[start, end)`.
    pub ranges: Vec<[usize; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<Line>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<Line>,
}

#[derive(Serialize)]
pub struct Line {
    pub line: u64,
    pub text: String,
}

/// Runs a search. Blocking: async code goes through `search`.
/// Only files from the index are read, so the search sees exactly what the tree shows.
pub fn run(index: &Index, p: &Params) -> Result<Results, String> {
    let started = Instant::now();
    let deadline = started + TIMEOUT;

    let matcher = RegexMatcherBuilder::new()
        .fixed_strings(!p.regex)
        .case_smart(p.case == Case::Smart)
        .case_insensitive(p.case == Case::Insensitive)
        .line_terminator(Some(b'\n'))
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build(&p.query)
        .map_err(|e| format!("invalid pattern: {e}"))?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .before_context(p.context)
        .after_context(p.context)
        .binary_detection(BinaryDetection::quit(0))
        .heap_limit(Some(MAX_LINE_BUFFER))
        .build();

    let dir_prefix = p.path.as_ref().map(|dir| format!("{dir}/"));
    let mut results = Results {
        files: Vec::new(),
        total_matches: 0,
        files_searched: 0,
        truncated: false,
        elapsed_ms: 0,
    };
    let mut stored = 0;

    for doc in &index.docs {
        if let (Some(path), Some(prefix)) = (&p.path, &dir_prefix)
            && doc.path != *path
            && !doc.path.starts_with(prefix)
        {
            continue;
        }
        if !is_searchable(doc) {
            continue;
        }
        if Instant::now() > deadline {
            results.truncated = true;
            break;
        }
        let Some(file) = index.file_path(&doc.path) else {
            continue;
        };

        let mut sink = FileSink {
            matcher: &matcher,
            deadline,
            per_file: p.per_file,
            room: p.limit - stored,
            hits: Vec::new(),
            count: 0,
            before: Vec::new(),
            last_stored: false,
            binary: false,
            stopped: false,
        };
        results.files_searched += 1;
        // An unreadable file or a line longer than the buffer limit skips only that file.
        if searcher.search_path(&matcher, &file, &mut sink).is_err() || sink.binary {
            continue;
        }
        if sink.count > 0 {
            stored += sink.hits.len();
            results.total_matches += sink.count;
            results.files.push(FileHits {
                path: doc.path.clone(),
                title: doc.title.clone(),
                count: sink.count,
                matches: sink.hits,
            });
        }
        if sink.stopped {
            results.truncated = true;
            break;
        }
    }

    results.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(results)
}

/// The ripgrep layout: `path:line:text` for matches, `path-line-text` for context lines.
/// Lines starting with `#` are notes, not results.
pub fn to_text(r: &Results) -> String {
    let mut out = String::new();
    for file in &r.files {
        for hit in &file.matches {
            for l in &hit.before {
                let _ = writeln!(out, "{}-{}-{}", file.path, l.line, l.text);
            }
            let _ = writeln!(out, "{}:{}:{}", file.path, hit.line, hit.text);
            for l in &hit.after {
                let _ = writeln!(out, "{}-{}-{}", file.path, l.line, l.text);
            }
        }
        let hidden = file.count - file.matches.len();
        if hidden > 0 {
            let _ = writeln!(
                out,
                "# {}: {hidden} more matching lines, raise per_file to see them",
                file.path
            );
        }
    }
    let _ = writeln!(
        out,
        "# {} matching lines in {} files, {} text files searched, {} ms",
        r.total_matches,
        r.files.len(),
        r.files_searched,
        r.elapsed_ms
    );
    if r.truncated {
        let _ = writeln!(
            out,
            "# truncated: more matches may exist, narrow the query or raise limit"
        );
    }
    out
}

/// Collects the matches of one file.
struct FileSink<'a> {
    matcher: &'a RegexMatcher,
    deadline: Instant,
    per_file: usize,
    /// How many more lines the response can take.
    room: usize,
    hits: Vec<Hit>,
    count: usize,
    before: Vec<Line>,
    /// The last matching line was stored, so after-context lines belong to it.
    last_stored: bool,
    binary: bool,
    /// Stopped early because of the response limit or the timeout.
    stopped: bool,
}

impl Sink for FileSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if Instant::now() > self.deadline {
            self.stopped = true;
            return Ok(false);
        }
        let first = mat.line_number().unwrap_or(0);
        for (number, bytes) in (first..).zip(mat.lines()) {
            self.last_stored = false;
            if self.hits.len() >= self.room {
                self.stopped = true;
                return Ok(false);
            }
            self.count += 1;
            if self.hits.len() < self.per_file {
                let mut hit = hit(self.matcher, number, bytes);
                hit.before = std::mem::take(&mut self.before);
                self.hits.push(hit);
                self.last_stored = true;
            }
        }
        self.before.clear();
        Ok(true)
    }

    fn context(&mut self, _: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, io::Error> {
        let text = String::from_utf8_lossy(trim_eol(ctx.bytes())).into_owned();
        let line = Line {
            line: ctx.line_number().unwrap_or(0),
            text: cut(text, Vec::new()).0,
        };
        match ctx.kind() {
            SinkContextKind::Before => self.before.push(line),
            SinkContextKind::After if self.last_stored => {
                if let Some(hit) = self.hits.last_mut() {
                    hit.after.push(line);
                }
            }
            _ => {}
        }
        Ok(true)
    }

    // Binary files (images, PDF, archives) are not text: whatever matched so far is dropped.
    fn binary_data(&mut self, _: &Searcher, _: u64) -> Result<bool, io::Error> {
        self.binary = true;
        Ok(false)
    }
}

fn trim_eol(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

fn hit(matcher: &RegexMatcher, line: u64, bytes: &[u8]) -> Hit {
    let bytes = trim_eol(bytes);
    let mut spans = Vec::new();
    let _ = matcher.find_iter(bytes, |m| {
        spans.push((m.start(), m.end()));
        spans.len() < MAX_RANGES
    });

    let text = String::from_utf8_lossy(bytes).into_owned();
    let valid = std::str::from_utf8(bytes).is_ok();
    // Byte offsets into character offsets. Matches come in order, so one pass is enough.
    let (mut byte_pos, mut char_pos) = (0, 0);
    let mut to_char = |off: usize| {
        if !valid {
            return String::from_utf8_lossy(&bytes[..off]).chars().count();
        }
        if off > byte_pos {
            char_pos += text
                .get(byte_pos..off)
                .map_or(off - byte_pos, |s| s.chars().count());
            byte_pos = off;
        }
        char_pos
    };
    let ranges = spans
        .iter()
        .map(|&(start, end)| [to_char(start), to_char(end)])
        .collect();

    let (text, ranges) = cut(text, ranges);
    Hit {
        line,
        text,
        ranges,
        before: Vec::new(),
        after: Vec::new(),
    }
}

/// Cuts a long line to a window around the first match and shifts the ranges with it.
fn cut(text: String, ranges: Vec<[usize; 2]>) -> (String, Vec<[usize; 2]>) {
    let len = text.chars().count();
    if len <= MAX_LINE_CHARS {
        return (text, ranges);
    }
    let first = ranges.first().map_or(0, |r| r[0]);
    let start = first.saturating_sub(LEAD_CHARS).min(len - MAX_LINE_CHARS);
    let end = start + MAX_LINE_CHARS;

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(text.chars().skip(start).take(MAX_LINE_CHARS));
    if end < len {
        out.push('…');
    }

    let shift = usize::from(start > 0);
    let ranges = ranges
        .into_iter()
        .filter(|r| r[0] < end && r[1] > start)
        .map(|r| {
            [
                r[0].max(start) - start + shift,
                r[1].min(end) - start + shift,
            ]
        })
        .collect();
    (out, ranges)
}
