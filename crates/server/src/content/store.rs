use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::cache::{INCOMING, Stat, TempPath, clean_cache, copy_hashing, stat_of};
use super::index::Index;
use super::unpack::build_version;
use crate::config::Config;

/// State shared by every user: what is loaded and when.
#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub archive: String,
    /// sha256 of the archive, used to decide whether the content changed.
    pub hash: Option<String>,
    /// Modification time of the archive in storage.
    pub archive_modified_at: Option<DateTime<Utc>>,
    /// When the current content was loaded.
    pub updated_at: Option<DateTime<Utc>>,
    /// When "Refresh" was last pressed, even if nothing changed.
    pub checked_at: Option<DateTime<Utc>>,
    pub documents: usize,
    pub total_size: i64,
    pub readme: Option<String>,
    /// The last refresh error. The previous version keeps serving in that case.
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Updated,
    Unchanged,
    Busy,
    Error,
}

/// The documentation store. The current index is swapped whole in a single step,
/// requests already in flight finish reading their own version.
pub struct Store {
    source: PathBuf,
    cache_dir: PathBuf,
    max_bytes: u64,
    current: RwLock<Arc<Index>>,
    status: RwLock<Status>,
    last_stat: Mutex<Option<Stat>>,
    refreshing: Mutex<()>,
}

impl Store {
    pub fn new(cfg: &Config) -> Result<Self, String> {
        fs::create_dir_all(&cfg.cache_dir).map_err(|e| {
            format!(
                "failed to create the cache {}: {e}",
                cfg.cache_dir.display()
            )
        })?;
        clean_cache(&cfg.cache_dir);

        Ok(Store {
            source: cfg.data_dir.join(&cfg.archive),
            cache_dir: cfg.cache_dir.clone(),
            max_bytes: cfg.max_total_bytes,
            current: RwLock::new(Arc::new(Index::empty())),
            status: RwLock::new(Status {
                archive: cfg.archive.clone(),
                ..Default::default()
            }),
            last_stat: Mutex::new(None),
            refreshing: Mutex::new(()),
        })
    }

    pub fn current(&self) -> Arc<Index> {
        self.current.read().unwrap().clone()
    }

    pub fn status(&self) -> Status {
        self.status.read().unwrap().clone()
    }

    /// Checks the archive and loads it if the content changed.
    /// A blocking operation: call it from a request handler through web::block.
    pub fn refresh(&self) -> (Outcome, String) {
        let Ok(_guard) = self.refreshing.try_lock() else {
            return (Outcome::Busy, "a refresh is already running".into());
        };

        let checked_at = Utc::now();
        let result = self.check();

        let mut status = self.status.write().unwrap();
        status.checked_at = Some(checked_at);
        match result {
            Ok(Check::SameStat) => {
                status.error = None;
                (Outcome::Unchanged, "the archive has not changed".into())
            }
            Ok(Check::SameHash { stat }) => {
                // The file was rewritten with the same content: remember the new date, leave the index alone.
                *self.last_stat.lock().unwrap() = Some(stat);
                status.archive_modified_at = stat.1.map(DateTime::<Utc>::from);
                status.error = None;
                (
                    Outcome::Unchanged,
                    "the archive content has not changed".into(),
                )
            }
            Ok(Check::New { index, hash, stat }) => {
                status.hash = Some(hash);
                status.archive_modified_at = stat.1.map(DateTime::<Utc>::from);
                status.updated_at = Some(Utc::now());
                status.documents = index.docs.len();
                status.total_size = index.total_size;
                status.readme = index.readme.clone();
                status.error = None;
                *self.last_stat.lock().unwrap() = Some(stat);
                *self.current.write().unwrap() = Arc::new(*index);
                (
                    Outcome::Updated,
                    format!("files loaded: {}", status.documents),
                )
            }
            Err(e) => {
                status.error = Some(e.clone());
                (Outcome::Error, e)
            }
        }
    }

    fn check(&self) -> Result<Check, String> {
        let stat = stat_of(&self.source)?;
        let loaded = self.status.read().unwrap().hash.is_some();
        // Fast path: same size and date, the archive is not read at all.
        if loaded && *self.last_stat.lock().unwrap() == Some(stat) {
            return Ok(Check::SameStat);
        }

        // The archive is copied into the local cache. Documents must not be read straight from the
        // mounted directory: on a file swap a FUSE S3 mount can hand back chunks of different versions.
        let copy = TempPath(self.cache_dir.join(INCOMING));
        let hash = copy_hashing(&self.source, &copy.0, self.max_bytes)?;

        if stat_of(&self.source)? != stat {
            return Err("the archive changed while being read, refresh again".into());
        }
        if self.status.read().unwrap().hash.as_deref() == Some(hash.as_str()) {
            return Ok(Check::SameHash { stat });
        }

        let index = Box::new(build_version(
            &copy.0,
            &self.cache_dir,
            &hash,
            self.max_bytes,
        )?);
        Ok(Check::New { index, hash, stat })
    }
}

enum Check {
    SameStat,
    SameHash {
        stat: Stat,
    },
    New {
        index: Box<Index>,
        hash: String,
        stat: Stat,
    },
}
