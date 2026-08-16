use pyxeebot::{bot, config::Config, db::Database};
use tracing::info;

// worker_threads = 1 is intentional: this bot runs on a 1-vCPU VPS. If you're
// deploying on a multi-core box, or expect to serve many guilds concurrently,
// bump this — with a single worker thread every guild's actor and every
// synchronous CPU-bound step (e.g. JSON-parsing a large yt-dlp playlist dump)
// shares one OS thread, so a slow parse in one guild can stall the gateway
// and every other guild's playback until it finishes.
#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    let recent_logs = bot::setup_logging(&config)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        prefix = %config.default_prefix,
        "PyxeeBot starting"
    );

    let db = Database::new(&config.db_path).await?;

    bot::run(config, db, recent_logs).await
}
