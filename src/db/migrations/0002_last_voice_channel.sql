-- NULL means not currently connected.
ALTER TABLE guild_settings ADD COLUMN last_voice_channel_id INTEGER;
