-- Needed for per-user listening-time stats; existing rows get 0, which just
-- means their minutes-listened total undercounts plays recorded before this
-- migration, not that they're wrong.
ALTER TABLE play_history ADD COLUMN duration INTEGER NOT NULL DEFAULT 0;
