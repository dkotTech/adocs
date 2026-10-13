//! A directory of the index as a zip file, written in one pass to a plain `Write`.
//!
//! The zip crate needs a seekable writer, so the stream to the browser would have to go through a
//! temporary file first. The format itself allows one pass: a header with the sizes and CRC
//! unknown, the deflated data, then a descriptor carrying them (flag bit 3). Each file is read
//! in chunks, so memory does not depend on the file size. Zip64 is not written: a directory above
//! `MAX_BYTES` or `MAX_FILES` is refused, and the archive itself is bounded by `ADOCS_MAX_SIZE_MB`.

use std::fs::File;
use std::io::{self, Read, Write};

use chrono::{DateTime, Datelike, Timelike, Utc};
use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};

use crate::content::Index;

/// More files than fit in the end record without zip64.
pub const MAX_FILES: usize = 65_000;
/// Above this the offsets would not fit in 32 bits; the gap to 4 GiB covers headers and the
/// slight growth of data deflate cannot compress.
pub const MAX_BYTES: i64 = 4_000_000_000;

const READ_CHUNK: usize = 64 * 1024;
/// Version 2.0: deflate and data descriptors.
const VERSION: u16 = 20;
/// Bit 3: the CRC and sizes follow the data. Bit 11: names are UTF-8.
const FLAGS: u16 = 0x0808;
const DEFLATE: u16 = 8;
/// "Made by" a Unix tool, so the permissions in the external attributes are read.
const MADE_BY: u16 = 3 << 8 | VERSION;
const PERMISSIONS: u32 = 0o100644 << 16;

struct Entry {
    name: String,
    crc: u32,
    compressed: u64,
    size: u64,
    time: u16,
    date: u16,
    offset: u64,
}

/// The files of `dir` as `<dir name>/<path inside>`, so unpacking gives a directory.
pub fn write(index: &Index, dir: &str, out: impl Write) -> io::Result<()> {
    let mut out = Counted { inner: out, pos: 0 };
    let top = dir.rsplit('/').next().unwrap_or(dir);
    let mut entries = Vec::new();

    for meta in index.in_dir(dir) {
        let name = format!("{top}/{}", &meta.path[dir.len() + 1..]);
        let (date, time) = dos_time(meta.updated_at);
        let offset = out.pos;

        out.write_all(&0x0403_4b50u32.to_le_bytes())?;
        put16(&mut out, &[VERSION, FLAGS, DEFLATE, time, date])?;
        put32(&mut out, &[0, 0, 0])?;
        put16(&mut out, &[name.len() as u16, 0])?;
        out.write_all(name.as_bytes())?;

        let path = index.file_path(&meta.path).ok_or(io::ErrorKind::NotFound)?;
        let (crc, size, compressed) = deflate(File::open(path)?, &mut out)?;

        out.write_all(&0x0807_4b50u32.to_le_bytes())?;
        put32(&mut out, &[crc, compressed as u32, size as u32])?;

        entries.push(Entry {
            name,
            crc,
            compressed,
            size,
            time,
            date,
            offset,
        });
    }

    let start = out.pos;
    for e in &entries {
        out.write_all(&0x0201_4b50u32.to_le_bytes())?;
        put16(
            &mut out,
            &[MADE_BY, VERSION, FLAGS, DEFLATE, e.time, e.date],
        )?;
        put32(&mut out, &[e.crc, e.compressed as u32, e.size as u32])?;
        put16(&mut out, &[e.name.len() as u16, 0, 0, 0, 0])?;
        put32(&mut out, &[PERMISSIONS, e.offset as u32])?;
        out.write_all(e.name.as_bytes())?;
    }
    let end = out.pos;

    out.write_all(&0x0605_4b50u32.to_le_bytes())?;
    put16(
        &mut out,
        &[0, 0, entries.len() as u16, entries.len() as u16],
    )?;
    put32(&mut out, &[(end - start) as u32, start as u32])?;
    put16(&mut out, &[0])?;
    out.flush()
}

/// Deflates the file into `out`; gives the CRC, the size and the compressed size.
fn deflate(mut file: File, out: &mut impl Write) -> io::Result<(u32, u64, u64)> {
    let mut enc = DeflateEncoder::new(out, Compression::default());
    let mut crc = Crc::new();
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        enc.write_all(&buf[..n])?;
    }
    enc.try_finish()?;
    Ok((crc.sum(), enc.total_in(), enc.total_out()))
}

/// MS-DOS date and time, the only time stamp a plain entry has. Before 1980 is the epoch of
/// the format; a missing time is shown the same way.
fn dos_time(t: Option<DateTime<Utc>>) -> (u16, u16) {
    let Some(t) = t.filter(|t| t.year() >= 1980) else {
        return (1 << 5 | 1, 0);
    };
    let date = ((t.year() - 1980) as u16) << 9 | (t.month() as u16) << 5 | t.day() as u16;
    let time = (t.hour() as u16) << 11 | (t.minute() as u16) << 5 | (t.second() as u16) / 2;
    (date, time)
}

fn put16(out: &mut impl Write, values: &[u16]) -> io::Result<()> {
    for v in values {
        out.write_all(&v.to_le_bytes())?;
    }
    Ok(())
}

fn put32(out: &mut impl Write, values: &[u32]) -> io::Result<()> {
    for v in values {
        out.write_all(&v.to_le_bytes())?;
    }
    Ok(())
}

/// Counts the bytes written: the central directory records where every entry began.
struct Counted<W: Write> {
    inner: W,
    pos: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
