// ── Scoring: generic / noise tokens ─────────────────────────────────────────

pub const SEARCH_GENERIC_TOKENS: &[&str] = &[
    "audio", "full", "hd", "hq", "lyrics", "lyric", "music", "official", "song", "ver", "version",
    "video",
];

pub const SEARCH_ANIME_SIGNAL_TOKENS: &[&str] = &[
    "op", "ed", "ost", "opening", "ending", "theme", "anime", "tv",
];

// ── Scoring: discouraged tokens → penalty multiplier ────────────────────────

pub const SEARCH_DISCOURAGED_TOKENS: &[(&str, f64)] = &[
    ("amv", 0.60),
    ("cast", 0.70),
    ("cover", 0.60),
    ("edit", 0.15),
    ("instrumental", 0.60),
    ("karaoke", 0.70),
    ("nightcore", 0.70),
    ("reaction", 0.65),
    ("reacts", 0.65),
    ("remix", 0.45),
    ("reverb", 0.22),
    ("seiyuu", 0.70),
    ("slowed", 0.45),
    ("live", 0.50),
    ("stage", 0.45),
    ("concert", 0.50),
    ("guitar", 0.50),
    ("piano", 0.50),
    ("violin", 0.45),
    ("acoustic", 0.35),
    ("fingerstyle", 0.55),
    ("ukulele", 0.55),
    ("bass", 0.45),
    ("drums", 0.45),
    ("drum", 0.40),
    ("flute", 0.45),
    ("cello", 0.45),
    ("harp", 0.45),
    ("saxophone", 0.45),
    ("lyrics", 0.80),
    ("lyric", 0.50),
    ("romaji", 0.70),
    ("subtitles", 0.35),
    ("kanji", 0.35),
    ("translation", 0.45),
];

pub const SEARCH_DISCOURAGED_PHRASES: &[(&str, f64)] = &[
    ("cast version", 0.80),
    ("cast ver", 0.75),
    ("character song", 0.65),
    ("female version", 0.40),
    ("male version", 0.40),
    ("lyric video", 0.45),
    ("lyrics video", 0.50),
    ("with lyrics", 0.50),
    ("english cover", 0.80),
    ("first take", 0.65),
    ("short ver", 0.30),
    ("short version", 0.30),
    ("sped up", 0.45),
    ("tv size", 0.22),
    ("anime size", 0.40),
    ("anime ver", 0.35),
    ("anime version", 0.35),
    ("op ver", 0.35),
    ("ed ver", 0.35),
    ("1 hour", 0.90),
    ("one hour", 0.90),
    ("10 hours", 0.90),
    ("2 hours", 0.90),
    ("3 hours", 0.90),
    ("extended mix", 0.30),
    ("full album", 0.60),
    ("compilation", 0.50),
    ("best of", 0.35),
    ("live at", 0.60),
    ("live from", 0.60),
    ("live in", 0.55),
    ("live performance", 0.65),
    ("live version", 0.55),
    ("live recording", 0.60),
    ("in concert", 0.60),
    ("on stage", 0.55),
];

// ── Scoring: preferred phrases → bonus ───────────────────────────────────────

pub const SEARCH_PREFERRED_PHRASES: &[(&str, f64)] = &[
    ("official audio", 0.30),
    ("official music video", 0.22),
    ("official mv", 0.20),
    ("official ver", 0.18),
    ("official version", 0.18),
    ("official video", 0.16),
    ("music video", 0.20),
];

// ── Scoring: preferred uploader tokens → bonus ───────────────────────────────

pub const SEARCH_PREFERRED_UPLOADER_TOKENS: &[(&str, f64)] = &[
    ("topic", 0.35),
    ("vevo", 0.28),
    ("hybe", 0.22),
    ("bighit", 0.22),
    ("smtown", 0.22),
    ("ygentertainment", 0.22),
    ("jyp", 0.18),
    ("starship", 0.16),
    ("official", 0.22),
    ("records", 0.10),
    ("music", 0.06),
    ("avex", 0.18),
    ("ponycanyon", 0.18),
    ("kingrecords", 0.18),
    ("sonymusic", 0.18),
    ("columbia", 0.15),
    ("victor", 0.15),
    ("tokyorecords", 0.15),
    ("lantis", 0.15),
    ("kicm", 0.12),
    ("universal", 0.14),
    ("warner", 0.14),
    ("atlantic", 0.14),
    ("capitol", 0.14),
    ("interscope", 0.12),
    ("republic", 0.12),
];

// ── Curation-mode extra filters ───────────────────────────────────────────────

pub const SEARCH_CURATION_EXTRA_TOKENS: &[&str] = &[
    "live", "concert", "stage", "festival", "session", "acoustic", "reaction", "reacts",
];

pub const SEARCH_CURATION_EXTRA_PHRASES: &[&str] = &[
    "at the",
    "in concert",
    "tour",
    "unplugged",
    "bbc session",
    "radio session",
    "tv performance",
];

// ── Audio (Songbird / yt-dlp) ─────────────────────────────────────────────────

/// Preferred format string passed to yt-dlp via --format.
pub const YTDLP_FORMAT: &str = "bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio/best[height<=480]";
