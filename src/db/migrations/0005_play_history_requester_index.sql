-- Covers !mystats' (guild_id, requester_id) filter, previously a full table scan.
CREATE INDEX IF NOT EXISTS idx_play_history_requester
    ON play_history(guild_id, requester_id, played_at);
