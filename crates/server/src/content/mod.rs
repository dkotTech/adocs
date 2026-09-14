//! Documentation from an archive: loading, unpacking into the cache and access to the current version.
//!
//! - `store`: the store, refresh on demand and the status shown in the interface;
//! - `index`: the current version in memory and building the index from unpacked files;
//! - `unpack`: extracting the archive into a version directory;
//! - `archive`: format detection and reading tar, tar.gz, zip;
//! - `cache`: the archive copy, cache cleanup and small helpers.

mod archive;
mod cache;
mod index;
mod store;
mod unpack;

pub use index::{DocMeta, Index};
pub use store::{Outcome, Status, Store};
