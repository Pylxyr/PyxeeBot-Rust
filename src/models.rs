use serde::{Deserialize, Serialize};

/// A queued track. Immutable — the player never mutates a `Track` in place.
/// Resolution state (stream URL, resolved-at timestamp) lives separately on
/// `ResolvedTrack` (added in Phase 2), not on the queue entry itself.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub title: String,
    pub webpage_url: String,
    pub uploader: String,
    pub duration: i64,
    pub requester_id: u64,
    pub query: String,
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub acodec: String,
    #[serde(default)]
    pub abr: f64,
}

impl Track {
    pub fn escaped_title(&self) -> String {
        escape_markdown(&self.title)
    }

    pub fn escaped_uploader(&self) -> String {
        if self.uploader.is_empty() {
            "Unknown".to_owned()
        } else {
            escape_markdown(&self.uploader)
        }
    }

    /// "m:ss" or "h:mm:ss" — matches Python's Track.duration_label.
    pub fn duration_label(&self) -> String {
        let total = self.duration.max(0);
        let (hours, rem) = (total / 3600, total % 3600);
        let (minutes, seconds) = (rem / 60, rem % 60);
        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        }
    }
}

/// Mirrors discord.utils.escape_markdown for the characters the bot's own
/// title/uploader strings can contain.
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '*'
                | '_'
                | '`'
                | '|'
                | '~'
                | '<'
                | '>'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '+'
                | '#'
                | '-'
                | '!'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopMode {
    #[default]
    Off,
    One,
    All,
}

impl LoopMode {
    pub fn cycle(self) -> Self {
        match self {
            LoopMode::Off => LoopMode::One,
            LoopMode::One => LoopMode::All,
            LoopMode::All => LoopMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LoopMode::Off => "Off",
            LoopMode::One => "Single track",
            LoopMode::All => "Entire queue",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            LoopMode::Off => "→",
            LoopMode::One => "↻¹",
            LoopMode::All => "↻",
        }
    }
}

/// Persisted queue state for restart recovery. `version` lets future formats
/// be told apart from this one — the Python bot's snapshot format had no
/// such field, which would have made a future format change a silent
/// deserialization failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub version: u32,
    pub entries: Vec<Track>,
}

impl QueueSnapshot {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new(entries: Vec<Track>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            entries,
        }
    }
}
