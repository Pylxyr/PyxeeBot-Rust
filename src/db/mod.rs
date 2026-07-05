mod queries;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

pub use queries::{PlaylistEntry, PlaylistSummary, QueueEntryRef, TopPlayed, TopRequester};

/// Rows of play_history kept per guild — trimmed every `TRIM_EVERY_N_WRITES`
/// history inserts, matching the Python bot's write-amplification fix.
const HISTORY_MAX_ROWS: i64 = 5000;
const TRIM_EVERY_N_WRITES: u64 = 50;
const CHECKPOINT_EVERY_N_WRITES: u64 = 100;

pub struct Database {
    pool: SqlitePool,
    write_count: AtomicU64,
    prefix_cache: DashMap<u64, Option<String>>,
    dj_role_cache: DashMap<u64, Option<u64>>,
    stay_connected_cache: DashMap<u64, bool>,
    autoplay_cache: DashMap<u64, bool>,
    snapshot_hashes: DashMap<u64, u64>,
}

impl Database {
    pub async fn new(path: &Path) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("synchronous", "NORMAL");

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;

        // IF NOT EXISTS DDL — safe to run against a pre-existing Python-created
        // database. sqlx records the migration as applied without touching
        // existing tables or data.
        sqlx::migrate!("./src/db/migrations").run(&pool).await?;

        Ok(Self {
            pool,
            write_count: AtomicU64::new(0),
            prefix_cache: DashMap::new(),
            dj_role_cache: DashMap::new(),
            stay_connected_cache: DashMap::new(),
            autoplay_cache: DashMap::new(),
            snapshot_hashes: DashMap::new(),
        })
    }

    fn next_write_count(&self) -> u64 {
        self.write_count.fetch_add(1, Ordering::Relaxed) + 1
    }
}
