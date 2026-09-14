use std::fs::File;
use std::io::{Read, Seek};

use super::unpack::Extractor;

pub(super) enum Format {
    Zip,
    TarGz,
    Tar,
}

/// The format is detected from the content, not the extension: CI names artifacts however it likes.
pub(super) fn detect_format(file: &mut File) -> Result<Format, String> {
    let mut head = [0u8; 262];
    let n = file.read(&mut head).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;

    if n >= 2 && head[..2] == [0x1f, 0x8b] {
        return Ok(Format::TarGz);
    }
    if n >= 4 && head[..4] == *b"PK\x03\x04" {
        return Ok(Format::Zip);
    }
    if n >= 262 && &head[257..262] == b"ustar" {
        return Ok(Format::Tar);
    }
    Err("unknown archive format: tar, tar.gz and zip are supported".into())
}

pub(super) fn read_tar(reader: impl Read, extractor: &mut Extractor) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(|e| format!("broken tar: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("broken tar: {e}"))?;
        // Regular files only: links and devices from the archive are never created.
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .map_err(|e| format!("broken name in tar: {e}"))?
            .to_string_lossy()
            .to_string();
        let mtime = entry.header().mtime().ok().map(|t| t as i64);
        extractor.add(&name, &mut entry, mtime)?;
    }
    Ok(())
}

pub(super) fn read_zip(file: File, extractor: &mut Extractor) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("broken zip: {e}"))?;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("broken zip: {e}"))?;
        if !entry.is_file() || entry.is_symlink() {
            continue;
        }
        // enclosed_name drops absolute paths and anything escaping the archive.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().to_string();
        let mtime = entry.last_modified().and_then(zip_time);
        extractor.add(&name, entry, mtime)?;
    }
    Ok(())
}

fn zip_time(t: zip::DateTime) -> Option<i64> {
    chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32)?
        .and_hms_opt(t.hour() as u32, t.minute() as u32, t.second() as u32)
        .map(|dt| dt.and_utc().timestamp())
}
