use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use sqlx::FromRow;

use super::{Database, CHECKPOINT_EVERY_N_WRITES, HISTORY_MAX_ROWS, TRIM_EVERY_N_WRITES};
use crate::models::Track;

#[derive(Debug, FromRow)]
pub struct PlaylistSummary {
    pub name: String,
    pub created_by: i64,
    pub created_at: String,
    pub track_count: i64,
}

#[derive(Debug, FromRow)]
pub struct PlaylistEntry {
    pub query: String,
    pub title: String,
    pub webpage_url: String,
}

#[derive(Debug, FromRow)]
struct QueueSnapshotRow {
    query: String,
    title: String,
    webpage_url: String,
    requester_id: i64,
}

#[derive(Debug, FromRow)]
pub struct TopPlayed {
    pub title: String,
    pub webpage_url: String,
    pub play_count: i64,
}

#[derive(Debug, FromRow)]
pub struct TopRequester {
    pub requester_id: i64,
    pub request_count: i64,
}

/// Minimal fields a caller needs to persist a queue entry (playlist item or
/// snapshot row) — deliberately not the full `Track`, since only these four
/// fields round-trip through storage.
#[derive(Debug, Clone)]
pub struct QueueEntryRef<'a> {
    pub query: &'a str,
    pub title: &'a str,
    pub webpage_url: &'a str,
    pub requester_id: u64,
}

impl Database {
    // ── guild settings (prefix / DJ role / stay / autoplay) ─────────────────

    pub async fn get_prefix(&self, guild_id: u64) -> Option<String> {
        if let Some(cached) = self.prefix_cache.get(&guild_id) {
            return cached.clone();
        }
        let row: Option<(String,)> =
            sqlx::query_as::<_, (String,)>("SELECT prefix FROM guild_settings WHERE guild_id = ?")
                .bind(guild_id as i64)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        let prefix = row.map(|(p,)| p);
        self.prefix_cache.insert(guild_id, prefix.clone());
        prefix
    }

    pub async fn set_prefix(&self, guild_id: u64, prefix: &str) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, prefix) VALUES (?, ?)
             ON CONFLICT(guild_id) DO UPDATE SET prefix = excluded.prefix",
        )
        .bind(guild_id as i64)
        .bind(prefix)
        .execute(&self.pool)
        .await?;
        self.prefix_cache.insert(guild_id, Some(prefix.to_owned()));
        Ok(())
    }

    pub async fn get_dj_role_id(&self, guild_id: u64) -> Option<u64> {
        if let Some(cached) = self.dj_role_cache.get(&guild_id) {
            return *cached;
        }
        let row: Option<(Option<i64>,)> = sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT dj_role_id FROM guild_settings WHERE guild_id = ?",
        )
        .bind(guild_id as i64)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let role_id = row.and_then(|(r,)| r).map(|r| r as u64);
        self.dj_role_cache.insert(guild_id, role_id);
        role_id
    }

    pub async fn set_dj_role_id(
        &self,
        guild_id: u64,
        role_id: Option<u64>,
        default_prefix: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, prefix, dj_role_id) VALUES (?, ?, ?)
             ON CONFLICT(guild_id) DO UPDATE SET dj_role_id = excluded.dj_role_id",
        )
        .bind(guild_id as i64)
        .bind(default_prefix)
        .bind(role_id.map(|r| r as i64))
        .execute(&self.pool)
        .await?;
        self.dj_role_cache.insert(guild_id, role_id);
        self.prefix_cache
            .entry(guild_id)
            .or_insert_with(|| Some(default_prefix.to_owned()));
        Ok(())
    }

    pub async fn get_stay_connected(&self, guild_id: u64) -> bool {
        if let Some(cached) = self.stay_connected_cache.get(&guild_id) {
            return *cached;
        }
        let row: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(
            "SELECT stay_connected FROM guild_settings WHERE guild_id = ?",
        )
        .bind(guild_id as i64)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let value = row.is_some_and(|(v,)| v != 0);
        self.stay_connected_cache.insert(guild_id, value);
        value
    }

    pub async fn set_stay_connected(
        &self,
        guild_id: u64,
        enabled: bool,
        default_prefix: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, prefix, stay_connected) VALUES (?, ?, ?)
             ON CONFLICT(guild_id) DO UPDATE SET stay_connected = excluded.stay_connected",
        )
        .bind(guild_id as i64)
        .bind(default_prefix)
        .bind(enabled as i64)
        .execute(&self.pool)
        .await?;
        self.stay_connected_cache.insert(guild_id, enabled);
        self.prefix_cache
            .entry(guild_id)
            .or_insert_with(|| Some(default_prefix.to_owned()));
        Ok(())
    }

    pub async fn get_autoplay(&self, guild_id: u64) -> bool {
        if let Some(cached) = self.autoplay_cache.get(&guild_id) {
            return *cached;
        }
        let row: Option<(i64,)> =
            sqlx::query_as::<_, (i64,)>("SELECT autoplay FROM guild_settings WHERE guild_id = ?")
                .bind(guild_id as i64)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        let value = row.is_some_and(|(v,)| v != 0);
        self.autoplay_cache.insert(guild_id, value);
        value
    }

    pub async fn set_autoplay(
        &self,
        guild_id: u64,
        enabled: bool,
        default_prefix: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, prefix, autoplay) VALUES (?, ?, ?)
             ON CONFLICT(guild_id) DO UPDATE SET autoplay = excluded.autoplay",
        )
        .bind(guild_id as i64)
        .bind(default_prefix)
        .bind(enabled as i64)
        .execute(&self.pool)
        .await?;
        self.autoplay_cache.insert(guild_id, enabled);
        self.prefix_cache
            .entry(guild_id)
            .or_insert_with(|| Some(default_prefix.to_owned()));
        Ok(())
    }

    pub async fn set_last_voice_channel(
        &self,
        guild_id: u64,
        channel_id: Option<u64>,
        default_prefix: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, prefix, last_voice_channel_id) VALUES (?, ?, ?)
             ON CONFLICT(guild_id) DO UPDATE SET last_voice_channel_id = excluded.last_voice_channel_id",
        )
        .bind(guild_id as i64)
        .bind(default_prefix)
        .bind(channel_id.map(|c| c as i64))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Guilds with a saved voice channel and a saved queue.
    pub async fn list_restorable_guilds(&self) -> sqlx::Result<Vec<(u64, u64)>> {
        let rows: Vec<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(
            "SELECT gs.guild_id, gs.last_voice_channel_id
             FROM guild_settings AS gs
             WHERE gs.last_voice_channel_id IS NOT NULL
               AND EXISTS (
                   SELECT 1 FROM queue_snapshots AS qs WHERE qs.guild_id = gs.guild_id
               )",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(g, c)| (g as u64, c as u64)).collect())
    }

    // ── saved playlists ──────────────────────────────────────────────────────

    pub async fn save_playlist(
        &self,
        guild_id: u64,
        name: &str,
        created_by: u64,
        entries: &[QueueEntryRef<'_>],
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO saved_playlists (guild_id, name, created_by) VALUES (?, ?, ?)
             ON CONFLICT(guild_id, name) DO UPDATE SET created_by = excluded.created_by",
        )
        .bind(guild_id as i64)
        .bind(name)
        .bind(created_by as i64)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM saved_playlist_items WHERE guild_id = ? AND playlist_name = ?")
            .bind(guild_id as i64)
            .bind(name)
            .execute(&mut *tx)
            .await?;

        for (position, entry) in entries.iter().enumerate() {
            sqlx::query(
                "INSERT INTO saved_playlist_items
                     (guild_id, playlist_name, position, query, title, webpage_url)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(guild_id as i64)
            .bind(name)
            .bind(position as i64)
            .bind(entry.query)
            .bind(entry.title)
            .bind(entry.webpage_url)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    pub async fn list_playlists(&self, guild_id: u64) -> sqlx::Result<Vec<PlaylistSummary>> {
        sqlx::query_as::<_, PlaylistSummary>(
            "SELECT p.name, p.created_by, p.created_at, COUNT(i.position) AS track_count
             FROM saved_playlists AS p
             LEFT JOIN saved_playlist_items AS i
                 ON p.guild_id = i.guild_id AND p.name = i.playlist_name
             WHERE p.guild_id = ?
             GROUP BY p.guild_id, p.name, p.created_by, p.created_at
             ORDER BY p.name COLLATE NOCASE",
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_playlist_entries(
        &self,
        guild_id: u64,
        name: &str,
    ) -> sqlx::Result<Vec<PlaylistEntry>> {
        sqlx::query_as::<_, PlaylistEntry>(
            "SELECT query, title, webpage_url FROM saved_playlist_items
             WHERE guild_id = ? AND playlist_name = ? ORDER BY position ASC",
        )
        .bind(guild_id as i64)
        .bind(name)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn delete_playlist(&self, guild_id: u64, name: &str) -> sqlx::Result<bool> {
        let result = sqlx::query("DELETE FROM saved_playlists WHERE guild_id = ? AND name = ?")
            .bind(guild_id as i64)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── queue snapshots (restart recovery) ───────────────────────────────────

    fn snapshot_hash(guild_id: u64, entries: &[QueueEntryRef<'_>]) -> u64 {
        let mut hasher = DefaultHasher::new();
        guild_id.hash(&mut hasher);
        for e in entries {
            e.query.hash(&mut hasher);
            e.title.hash(&mut hasher);
            e.webpage_url.hash(&mut hasher);
            e.requester_id.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub async fn save_queue_snapshot(
        &self,
        guild_id: u64,
        entries: &[QueueEntryRef<'_>],
    ) -> sqlx::Result<()> {
        let new_hash = Self::snapshot_hash(guild_id, entries);
        if self
            .snapshot_hashes
            .get(&guild_id)
            .is_some_and(|h| *h == new_hash)
        {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM queue_snapshots WHERE guild_id = ?")
            .bind(guild_id as i64)
            .execute(&mut *tx)
            .await?;
        for (position, entry) in entries.iter().enumerate() {
            sqlx::query(
                "INSERT INTO queue_snapshots
                     (guild_id, position, query, title, webpage_url, requester_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(guild_id as i64)
            .bind(position as i64)
            .bind(entry.query)
            .bind(entry.title)
            .bind(entry.webpage_url)
            .bind(entry.requester_id as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        if entries.is_empty() {
            self.snapshot_hashes.remove(&guild_id);
        } else {
            self.snapshot_hashes.insert(guild_id, new_hash);
        }
        Ok(())
    }

    pub async fn load_queue_snapshot(&self, guild_id: u64) -> sqlx::Result<Vec<Track>> {
        let rows: Vec<QueueSnapshotRow> = sqlx::query_as::<_, QueueSnapshotRow>(
            "SELECT query, title, webpage_url, requester_id FROM queue_snapshots
             WHERE guild_id = ? ORDER BY position ASC",
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Track {
                title: r.title,
                webpage_url: r.webpage_url,
                uploader: String::new(),
                duration: 0,
                requester_id: r.requester_id as u64,
                query: r.query,
                thumbnail_url: String::new(),
                tags: Vec::new(),
                acodec: String::new(),
                abr: 0.0,
            })
            .collect())
    }

    // ── play history ─────────────────────────────────────────────────────────

    pub async fn add_play_history(
        &self,
        guild_id: u64,
        title: &str,
        webpage_url: &str,
        requester_id: u64,
    ) -> sqlx::Result<()> {
        let count = self.next_write_count();

        sqlx::query(
            "INSERT INTO play_history (guild_id, title, webpage_url, requester_id)
             VALUES (?, ?, ?, ?)",
        )
        .bind(guild_id as i64)
        .bind(title)
        .bind(webpage_url)
        .bind(requester_id as i64)
        .execute(&self.pool)
        .await?;

        if count.is_multiple_of(TRIM_EVERY_N_WRITES) {
            sqlx::query(
                "DELETE FROM play_history
                 WHERE guild_id = ? AND id NOT IN (
                     SELECT id FROM play_history WHERE guild_id = ?
                     ORDER BY played_at DESC LIMIT ?
                 )",
            )
            .bind(guild_id as i64)
            .bind(guild_id as i64)
            .bind(HISTORY_MAX_ROWS)
            .execute(&self.pool)
            .await?;
        }

        if count.is_multiple_of(CHECKPOINT_EVERY_N_WRITES) {
            sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    pub async fn get_top_played(&self, guild_id: u64, limit: i64) -> sqlx::Result<Vec<TopPlayed>> {
        sqlx::query_as::<_, TopPlayed>(
            "SELECT title, webpage_url, COUNT(*) AS play_count
             FROM play_history WHERE guild_id = ?
             GROUP BY webpage_url
             ORDER BY play_count DESC, MAX(played_at) DESC
             LIMIT ?",
        )
        .bind(guild_id as i64)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_top_requesters(
        &self,
        guild_id: u64,
        limit: i64,
    ) -> sqlx::Result<Vec<TopRequester>> {
        sqlx::query_as::<_, TopRequester>(
            "SELECT requester_id, COUNT(*) AS request_count
             FROM play_history WHERE guild_id = ?
             GROUP BY requester_id
             ORDER BY request_count DESC
             LIMIT ?",
        )
        .bind(guild_id as i64)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
}
