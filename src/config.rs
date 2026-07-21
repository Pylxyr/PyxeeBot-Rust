use std::path::PathBuf;

use anyhow::{bail, Context as _, Result};

pub struct Config {
    pub token: String,
    pub default_prefix: String,
    pub bot_owners: Vec<u64>,
    pub log_level: String,
    pub db_path: PathBuf,
    pub log_to_file: bool,
    pub log_dir: PathBuf,
    pub max_queue_size: usize,
    pub max_queue_size_per_user: usize,
    pub max_playlist_size: usize,
    pub idle_timeout_secs: u64,
    pub empty_channel_timeout_secs: u64,
    pub ytdlp_cookies_file: Option<PathBuf>,
    pub ytdlp_cache_dir: PathBuf,
    pub ytdlp_js_runtime_path: Option<String>,
    pub ytdlp_pot_provider_base_url: Option<String>,
    pub ytdlp_socket_timeout: u32,
    pub ytdlp_prefetch_count: usize,
    pub ytdlp_concurrent_extracts: usize,
    pub ytdlp_curation_concurrency: usize,
    pub near_end_prefetch_secs: u64,
    pub opus_bitrate_kbps: u32,
    pub ytdlp_search_results: usize,
    pub ytdlp_resolve_cache_size: u64,
    pub ytdlp_resolve_cache_ttl_secs: u64,
    pub ytdlp_extract_timeout_secs: u64,
    pub np_auto_refresh: bool,
    pub np_auto_refresh_interval: u32,
    pub error_announce: bool,
    pub lastfm_api_key: Option<String>,
    pub restore_queue_on_restart: bool,
    pub bot_activity_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let base_dir = std::env::current_dir()?;
        let data_dir = base_dir.join("data");
        std::fs::create_dir_all(&data_dir)?;

        // yt-dlp's default ~/.cache/yt-dlp is blocked by ProtectHome=read-only,
        // silently disabling its signature-cache. Point it here instead.
        let ytdlp_cache_dir = data_dir.join("ytdlp-cache");
        std::fs::create_dir_all(&ytdlp_cache_dir)?;

        let token = env_str("DISCORD_TOKEN");
        if token.is_empty() {
            bail!("DISCORD_TOKEN is not set. Add it to .env before starting the bot.");
        }

        let default_prefix = {
            let p = env_str("DEFAULT_PREFIX");
            let p = if p.is_empty() { "!".to_owned() } else { p };
            if p.contains(' ') {
                bail!("DEFAULT_PREFIX cannot contain spaces.");
            }
            p
        };

        let log_dir = base_dir.join(env_str_or("LOG_DIR", "logs"));
        std::fs::create_dir_all(&log_dir)?;

        let ytdlp_cookies_file = {
            let raw = env_str("YTDLP_COOKIES_FILE");
            if raw.is_empty() {
                None
            } else {
                let p = PathBuf::from(&raw);
                Some(if p.is_absolute() { p } else { base_dir.join(p) })
            }
        };

        let ytdlp_js_runtime_path = {
            let s = env_str("YTDLP_JS_RUNTIME_PATH");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };

        let ytdlp_pot_provider_base_url = {
            let s = env_str("YTDLP_POT_PROVIDER_BASE_URL");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };

        let lastfm_api_key = {
            let s = env_str("LASTFM_API_KEY");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };

        let bot_activity_url = {
            let s = env_str_or("BOT_ACTIVITY_URL", "pylxyr.github.io/PyxeeBot-Page/");
            if s.is_empty() {
                "pylxyr.github.io/PyxeeBot-Page/".to_owned()
            } else {
                s
            }
        };

        Ok(Self {
            token,
            default_prefix,
            bot_owners: parse_owner_ids(&env_str("BOT_OWNERS"))?,
            log_level: env_str_or("LOG_LEVEL", "INFO").to_uppercase(),
            db_path: data_dir.join("musicbot.sqlite3"),
            log_to_file: bool_env("LOG_TO_FILE", true),
            log_dir,
            max_queue_size: int_env("MAX_QUEUE_SIZE", 100).max(1) as usize,
            max_queue_size_per_user: int_env("MAX_QUEUE_SIZE_PER_USER", 0).max(0) as usize,
            max_playlist_size: int_env("MAX_PLAYLIST_SIZE", 25).max(1) as usize,
            idle_timeout_secs: int_env("IDLE_TIMEOUT_SECONDS", 180).max(30) as u64,
            empty_channel_timeout_secs: int_env("EMPTY_CHANNEL_TIMEOUT_SECONDS", 60).max(15) as u64,
            ytdlp_cookies_file,
            ytdlp_cache_dir,
            ytdlp_js_runtime_path,
            ytdlp_pot_provider_base_url,
            ytdlp_socket_timeout: int_env("YTDLP_SOCKET_TIMEOUT", 15).max(5) as u32,
            ytdlp_prefetch_count: int_env("YTDLP_PREFETCH_COUNT", 1).max(0) as usize,
            ytdlp_concurrent_extracts: int_env("YTDLP_CONCURRENT_EXTRACTS", 1).max(1) as usize,
            ytdlp_curation_concurrency: int_env("YTDLP_CURATION_CONCURRENCY", 3).clamp(1, 6)
                as usize,
            near_end_prefetch_secs: int_env("NEAR_END_PREFETCH_SECONDS", 30).max(0) as u64,
            opus_bitrate_kbps: int_env("OPUS_BITRATE_KBPS", 64).clamp(64, 256) as u32,
            ytdlp_search_results: int_env("YTDLP_SEARCH_RESULTS", 5).clamp(1, 10) as usize,
            ytdlp_resolve_cache_size: int_env("YTDLP_RESOLVE_CACHE_SIZE", 128).max(16) as u64,
            ytdlp_resolve_cache_ttl_secs: int_env("YTDLP_RESOLVE_CACHE_TTL_SECONDS", 1800).max(60)
                as u64,
            ytdlp_extract_timeout_secs: int_env("YTDLP_EXTRACT_TIMEOUT_SECONDS", 45).max(5) as u64,
            np_auto_refresh: bool_env("NP_AUTO_REFRESH", false),
            np_auto_refresh_interval: int_env("NP_AUTO_REFRESH_INTERVAL", 30).max(15) as u32,
            error_announce: bool_env("ERROR_ANNOUNCE", true),
            lastfm_api_key,
            restore_queue_on_restart: bool_env("RESTORE_QUEUE_ON_RESTART", false),
            bot_activity_url,
        })
    }
}

fn env_str(name: &str) -> String {
    std::env::var(name).unwrap_or_default().trim().to_owned()
}

fn env_str_or(name: &str, default: &str) -> String {
    let v = std::env::var(name).unwrap_or_default();
    let v = v.trim();
    if v.is_empty() {
        default.to_owned()
    } else {
        v.to_owned()
    }
}

fn int_env(name: &str, default: i64) -> i64 {
    let raw = std::env::var(name).unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return default;
    }
    raw.parse().unwrap_or(default)
}

fn bool_env(name: &str, default: bool) -> bool {
    let val = std::env::var(name).unwrap_or_default();
    let val = val.trim().to_lowercase();
    match val.as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn parse_owner_ids(raw: &str) -> Result<Vec<u64>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<u64>()
                .with_context(|| format!("BOT_OWNERS: invalid user ID {s:?}"))
        })
        .collect()
}
