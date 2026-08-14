use pyxeebot::{bot, config::Config, db::Database};
use tracing::info;

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
