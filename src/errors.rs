use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("not connected to a voice channel")]
    NotInVoiceChannel,

    #[error("queue is full")]
    QueueFull,

    #[error("no playable result found for {0:?}")]
    NoResult(String),

    #[error("yt-dlp failed: {0}")]
    YtDlp(String),

    #[error("voice connection failed: {0}")]
    Voice(String),

    #[error("you must be in the same voice channel as the bot")]
    WrongVoiceChannel,

    #[error("you need the DJ role or manage-channels permission for this")]
    NotDj,
}

pub type Result<T> = std::result::Result<T, BotError>;
