use pyxeebot::{bot, config::Config, db::Database};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    bot::setup_logging(&config)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        prefix = %config.default_prefix,
        "PyxeeBot starting"
    );

    let db = Database::new(&config.db_path).await?;

    bot::run(config, db).await
}
