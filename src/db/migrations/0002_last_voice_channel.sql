-- Needed for restore_queue_on_restart: queue_snapshots already records
-- *what* to restore, but not *where* to reconnect. NULL means "not
-- currently connected" (or never connected).
ALTER TABLE guild_settings ADD COLUMN last_voice_channel_id INTEGER;
