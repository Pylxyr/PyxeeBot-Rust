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

    #[error("you already have the maximum number of tracks queued")]
    UserQueueFull,

    #[error("no playable result found for {0:?}")]
    NoResult(String),

    #[error("yt-dlp failed: {0}")]
    YtDlp(String),

    #[error("voice connection failed: {0}")]
    Voice(String),
}

pub type Result<T> = std::result::Result<T, BotError>;
