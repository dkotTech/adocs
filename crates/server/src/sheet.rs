//! Spreadsheets read into plain rows: xlsx, xlsm, xls, xlsb and ods.
//!
//! The book is parsed whole, so everything here is bounded by `SheetLimits` from the config: a file
//! that is too big is not opened at all and stays a download. The result is kept in the derived
//! cache beside the version, so the same table is parsed once and not on every request.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use calamine::{Data, DataType, Reader, open_workbook_auto};
use serde::{Deserialize, Serialize};

use crate::config::SheetLimits;
use crate::content::{DocMeta, Index, derived};

#[derive(Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// Rows or columns were left out by the limits.
    pub truncated: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    /// Sheets were left out by the limit.
    pub truncated: bool,
    /// Whether the first row is a header. In a delimited file it almost always is; in a sheet of a
    /// workbook it is just the first row, and guessing would hide it and shift every row number.
    pub header: bool,
}

/// Extensions `open_workbook_auto` dispatches on. A workbook is recognised by extension and not by
/// content type, because that is exactly how the reader picks its parser.
const WORKBOOK: &[&str] = &["xlsx", "xlsm", "xlam", "xla", "xls", "xlsb", "ods"];
/// Tables that are plain text. They are read here as well, so that one table looks like any other.
const DELIMITED: &[&str] = &["csv", "tsv", "tab"];

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

pub fn is_sheet(path: &str) -> bool {
    extension(path)
        .is_some_and(|e| WORKBOOK.contains(&e.as_str()) || DELIMITED.contains(&e.as_str()))
}

fn cut(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// A whole number is shown without a fractional part: a count must not read as `128400.0`.
fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn cell(data: &Data, limit: usize) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(text) => cut(text, limit),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => number(*value),
        Data::Bool(value) => value.to_string(),
        Data::DurationIso(text) => cut(text, limit),
        Data::Error(err) => format!("#{err:?}"),
        Data::DateTime(_) | Data::DateTimeIso(_) => match data.as_datetime() {
            // A date without a time of day is a date, which is how it was written in the cell
            Some(dt) if dt.time() == chrono::NaiveTime::MIN => dt.format("%Y-%m-%d").to_string(),
            Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
            None => String::new(),
        },
    }
}

/// The separator of a delimited file. Programs export tables with a comma, a semicolon or a tab,
/// and the file itself does not say which, so the most frequent one outside quotes wins.
fn separator(text: &str, ext: &str) -> char {
    if ext == "tsv" || ext == "tab" {
        return '\t';
    }
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let count = |sep: char| {
        line.chars()
            .scan(false, |quoted, c| {
                if c == '"' {
                    *quoted = !*quoted;
                }
                Some((*quoted, c))
            })
            .filter(|&(quoted, c)| !quoted && c == sep)
            .count()
    };
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|&sep| count(sep))
        .filter(|&sep| count(sep) > 0)
        .unwrap_or(',')
}

/// One record: a line, and the lines after it while a quote is left open. Doubled quotes keep the
/// count even, so an even number of them means the record is closed. A record is not allowed to
/// grow past what the limits could ever keep, or one unmatched quote in a large file would pull
/// the whole file into memory.
fn read_record(reader: &mut impl BufRead, buf: &mut Vec<u8>, budget: usize) -> io::Result<bool> {
    buf.clear();
    loop {
        if reader.read_until(b'\n', buf)? == 0 {
            return Ok(!buf.is_empty());
        }
        if buf.iter().filter(|&&b| b == b'"').count() % 2 == 0 || buf.len() >= budget {
            return Ok(true);
        }
    }
}

/// The cells of one record, by the rules of RFC 4180: a quoted cell may hold the separator, a line
/// break and a doubled quote.
fn cells_of(text: &str, sep: char, limits: &SheetLimits, wide: &mut bool) -> Vec<String> {
    let mut row: Vec<String> = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    let mut finish = |row: &mut Vec<String>, value: &mut String| {
        if row.len() < limits.max_cols {
            row.push(cut(value, limits.max_cell_chars));
        } else {
            *wide = true;
        }
        value.clear();
    };

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    value.push('"');
                }
                '"' => quoted = false,
                _ => value.push(c),
            }
            continue;
        }
        match c {
            '"' if value.is_empty() => quoted = true,
            c if c == sep => finish(&mut row, &mut value),
            _ => value.push(c),
        }
    }
    finish(&mut row, &mut value);
    row
}

/// A delimited text table, read record by record. Only as much of the file is read as the limits
/// allow to be shown, so a dataset of hundreds of megabytes costs the first few thousand rows of
/// it and nothing more. A byte order mark from an export out of Excel is dropped.
fn read_delimited(path: &Path, ext: &str, limits: &SheetLimits) -> Result<Workbook, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let budget = limits
        .max_cols
        .saturating_mul(limits.max_cell_chars)
        .saturating_mul(4)
        .max(64 * 1024);

    let mut buf = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut wide = false;
    let mut cells = 0usize;
    let mut sep = None;
    let mut truncated = false;

    while read_record(&mut reader, &mut buf, budget).map_err(|e| e.to_string())? {
        if rows.len() >= limits.max_rows || cells >= limits.max_cells {
            truncated = true;
            break;
        }
        let mut text = std::str::from_utf8(&buf).map_err(|e| e.to_string())?;
        if rows.is_empty() {
            text = text.strip_prefix('\u{feff}').unwrap_or(text);
        }
        let text = text.trim_end_matches(['\n', '\r']);
        let sep = *sep.get_or_insert_with(|| separator(text, ext));
        let row = cells_of(text, sep, limits, &mut wide);
        cells += row.len();
        rows.push(row);
    }

    Ok(Workbook {
        sheets: vec![Sheet {
            name: String::new(),
            rows,
            truncated: truncated || wide,
        }],
        truncated: false,
        header: true,
    })
}

fn read(path: &Path, limits: &SheetLimits) -> Result<Workbook, String> {
    let mut book = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let names = book.sheet_names();
    let truncated = names.len() > limits.max_sheets;

    let mut sheets = Vec::new();
    for name in names.into_iter().take(limits.max_sheets) {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        let (height, width) = range.get_size();
        let columns = width.min(limits.max_cols);
        // Rows and columns multiply, so the row limit alone would not bound a wide sheet
        let rows_by_cells = limits.max_cells / columns.max(1);
        let keep = limits.max_rows.min(rows_by_cells);
        let rows: Vec<Vec<String>> = range
            .rows()
            .take(keep)
            .map(|row| {
                row.iter()
                    .take(columns)
                    .map(|data| cell(data, limits.max_cell_chars))
                    .collect()
            })
            .collect();
        sheets.push(Sheet {
            truncated: height > rows.len() || width > columns,
            name,
            rows,
        });
    }
    Ok(Workbook {
        sheets,
        truncated,
        header: false,
    })
}

const TEXT: &str = "sheet-text";

/// The key of a value derived from a table. The limits shape the result, so they belong in the
/// key: after they change, rows cut by the previous settings must not be served.
fn cache_key(kind: &str, path: &str, limits: &SheetLimits) -> String {
    derived::key(kind, &format!("{path}\0{}", fingerprint(limits)))
}

/// The limits as one line, for the cache key.
fn fingerprint(limits: &SheetLimits) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        limits.max_bytes,
        limits.max_rows,
        limits.max_cols,
        limits.max_cells,
        limits.max_sheets,
        limits.max_cell_chars,
    )
}

/// The table as text: one line per row with cells separated by tabs, a note before each sheet.
/// This is what an agent reads through MCP, and the same shape a search over tables would use.
pub fn to_lines(book: &Workbook) -> Vec<String> {
    let mut lines = Vec::new();
    for sheet in &book.sheets {
        if !sheet.name.is_empty() {
            lines.push(format!("# sheet: {}", sheet.name));
        }
        lines.extend(sheet.rows.iter().map(|row| row.join("\t")));
        if sheet.truncated {
            lines.push("# the sheet is shown in part".to_string());
        }
    }
    if book.truncated {
        lines.push("# some sheets were left out".to_string());
    }
    lines
}

/// One line about the table for llms.txt and the other indexes: its size and what its columns are
/// called. Without it a table is just a name and a content type, which tells an agent nothing.
pub fn summary(book: &Workbook, limits: &SheetLimits) -> Option<String> {
    let first = book.sheets.first()?;
    let mut out = match book.sheets.len() {
        1 => format!("a table of {} rows", first.rows.len()),
        n => format!(
            "a table of {n} sheets, the first \"{}\" with {} rows",
            first.name,
            first.rows.len()
        ),
    };
    let columns: Vec<&str> = first
        .rows
        .first()?
        .iter()
        .filter(|cell| !cell.is_empty())
        .map(String::as_str)
        .take(limits.summary_columns)
        .collect();
    if !columns.is_empty() {
        let label = if book.header { "columns" } else { "first row" };
        out.push_str(&format!("; {label}: {}", columns.join(", ")));
    }
    Some(out)
}

/// The parsed book from a version directory: taken from the derived cache when it is already
/// there, otherwise parsed once and put there. `None` means the file is too large or the reader
/// could not make sense of it, and the document stays a download.
pub fn workbook_at(
    version: &Path,
    path: &str,
    size: i64,
    limits: &SheetLimits,
) -> Option<Workbook> {
    if size > limits.max_bytes {
        return None;
    }
    let file = version.join(path);
    let key = cache_key("sheet", path, limits);

    if let Some(cached) = derived::get(version, &key)
        && let Ok(book) = serde_json::from_slice(&cached)
    {
        return Some(book);
    }

    let ext = extension(path).unwrap_or_default();
    let book = if DELIMITED.contains(&ext.as_str()) {
        read_delimited(&file, &ext, limits)
    } else {
        read(&file, limits)
    }
    .inspect_err(|e| tracing::warn!("failed to read the table {path}: {e}"))
    .ok()?;
    if let Ok(bytes) = serde_json::to_vec(&book) {
        derived::put(version, &key, &bytes);
    }
    // The same rows as text, for the search engine: it reads files from disk, and a table is not
    // one until it is written as lines.
    derived::put(
        version,
        &cache_key(TEXT, path, limits),
        to_lines(&book).join("\n").as_bytes(),
    );
    Some(book)
}

/// The file the search engine reads instead of the table itself. `None` when the table was never
/// parsed, which is also when there is nothing to search.
pub fn text_path(version: &Path, path: &str, limits: &SheetLimits) -> Option<PathBuf> {
    let file = derived::path(version, &cache_key(TEXT, path, limits));
    file.is_file().then_some(file)
}

pub fn workbook(index: &Index, meta: &DocMeta, limits: &SheetLimits) -> Option<Workbook> {
    workbook_at(index.version_dir()?, &meta.path, meta.size, limits)
}
