CREATE TABLE IF NOT EXISTS guild_settings (
    guild_id       INTEGER PRIMARY KEY,
    prefix         TEXT    NOT NULL,
    dj_role_id     INTEGER,
    stay_connected INTEGER NOT NULL DEFAULT 0,
    autoplay       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS saved_playlists (
    guild_id   INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    created_by INTEGER NOT NULL,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (guild_id, name)
);

CREATE TABLE IF NOT EXISTS saved_playlist_items (
    guild_id      INTEGER NOT NULL,
    playlist_name TEXT    NOT NULL,
    position      INTEGER NOT NULL,
    query         TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    webpage_url   TEXT    NOT NULL,
    PRIMARY KEY (guild_id, playlist_name, position),
    FOREIGN KEY (guild_id, playlist_name)
        REFERENCES saved_playlists(guild_id, name)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS queue_snapshots (
    guild_id     INTEGER NOT NULL,
    position     INTEGER NOT NULL,
    query        TEXT    NOT NULL,
    title        TEXT    NOT NULL,
    webpage_url  TEXT    NOT NULL,
    requester_id INTEGER NOT NULL,
    PRIMARY KEY (guild_id, position)
);

CREATE TABLE IF NOT EXISTS play_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id     INTEGER NOT NULL,
    title        TEXT    NOT NULL,
    webpage_url  TEXT    NOT NULL,
    requester_id INTEGER NOT NULL,
    played_at    TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_play_history_guild ON play_history(guild_id, played_at);
CREATE INDEX IF NOT EXISTS idx_play_history_url ON play_history(guild_id, webpage_url, played_at);
