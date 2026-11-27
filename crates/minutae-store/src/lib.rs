//! minutae-store — SQLite persistence for Minutae v4 (docs/CROSS-PLATFORM.md §4).
//!
//! - `rusqlite` (bundled, FTS5), WAL, `busy_timeout`, foreign keys on.
//! - Versioned migrations via `PRAGMA user_version` (see [`schema`]).
//! - [`Store`] is `Clone + Send + Sync`: one writer connection plus a small pool
//!   of read-only connections (file databases), so searches never wait on writes.
//! - All repository methods are synchronous and short. From async code use
//!   [`Store::call`], which runs the closure on Tokio's blocking pool.

mod dictations;
mod import;
mod jobs;
mod meeting_content;
pub use meeting_content::PendingFileCleanup;
mod meetings;
pub mod schema;
mod search;
mod settings;
mod summaries;
mod transcripts;
mod usage;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};

pub use dictations::DictationStats;
pub use import::{ImportedMeeting, LegacyKind};
pub use jobs::{JobKind, JobRecord, JobState, NewJob};
pub use meetings::{
    DayGroup, MeetingDetail, MeetingListItem, Track as TrackFile, WeekStats, group_by_local_day,
};
pub use rusqlite;
pub use search::{
    HitKind, SearchFilters, SearchHit, SearchKind, fts_and_query, fts_or_query, search_tokens,
};
pub use summaries::{ActionChange, ActionItemRow, PublishChecklist};
pub use usage::{UsageAggregate, UsageEntry};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("background task failed: {0}")]
    Join(String),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

const READERS: usize = 4;

struct Inner {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

/// Thread-safe handle to the Minutae database. Cheap to clone.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("readers", &self.inner.readers.len())
            .finish()
    }
}

fn configure(conn: &Connection, file: bool) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if file {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}

fn lock(m: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Store {
    /// Open (or create) the database file and run pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StoreError::Invalid(format!("create {parent:?}: {e}")))?;
        }
        let mut writer = Connection::open(path)?;
        configure(&writer, true)?;
        schema::migrate(&mut writer)?;
        let mut readers = Vec::with_capacity(READERS);
        for _ in 0..READERS {
            let r = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )?;
            configure(&r, false)?;
            readers.push(Mutex::new(r));
        }
        Ok(Self::from_parts(writer, readers))
    }

    /// Open the default database at `minutae_core::paths::database_file()`.
    pub fn open_default() -> Result<Self> {
        Self::open(minutae_core::paths::database_file())
    }

    /// Private in-memory database (tests). Reads go through the writer connection.
    pub fn open_in_memory() -> Result<Self> {
        let mut writer = Connection::open_in_memory()?;
        configure(&writer, false)?;
        schema::migrate(&mut writer)?;
        Ok(Self::from_parts(writer, Vec::new()))
    }

    fn from_parts(writer: Connection, readers: Vec<Mutex<Connection>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                writer: Mutex::new(writer),
                readers,
                next: AtomicUsize::new(0),
            }),
        }
    }

    /// Run `f` with exclusive access to the writer connection.
    pub fn write<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut conn = lock(&self.inner.writer);
        f(&mut conn)
    }

    /// Run `f` inside a write transaction (committed when `f` returns Ok).
    pub fn transaction<R>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<R>,
    ) -> Result<R> {
        self.write(|conn| {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let r = f(&tx)?;
            tx.commit()?;
            Ok(r)
        })
    }

    /// Run `f` on a read connection (a pooled read-only one when available).
    pub fn read<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let readers = &self.inner.readers;
        if readers.is_empty() {
            return f(&lock(&self.inner.writer));
        }
        for r in readers {
            if let Ok(conn) = r.try_lock() {
                return f(&conn);
            }
        }
        let i = self.inner.next.fetch_add(1, Ordering::Relaxed) % readers.len();
        f(&lock(&readers[i]))
    }

    /// Run a store closure on Tokio's blocking pool.
    pub async fn call<R, F>(&self, f: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&Store) -> Result<R> + Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| StoreError::Join(e.to_string()))?
    }

    /// Current schema version (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        self.read(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
    }
}

// ---- small conversion helpers shared by the modules ----

pub(crate) fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

pub(crate) fn from_ms(v: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(v).single().unwrap_or_default()
}

pub(crate) fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn parse_uuid(s: &str) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn json_col<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Serialize a unit enum (e.g. `MeetingStatus`, `Track`) to its wire string.
pub(crate) fn enum_str<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub(crate) fn enum_from<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests;
