use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use moka::sync::Cache;
use poise::serenity_prelude as serenity;
use serenity::GatewayIntents;
use songbird::serenity::SerenityInit;
use songbird::Songbird;
use tracing_subscriber::fmt;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::db::Database;
use crate::extraction::Extractor;
use crate::lastfm::LastFmClient;
use crate::models::Track;
use crate::player::GuildPlayer;
use crate::scoring::ScoreBreakdown;

pub struct BotData {
    pub config: Arc<Config>,
    pub db: Database,
    pub songbird: Arc<Songbird>,
    pub extractor: Arc<Extractor>,
    pub lastfm: Option<LastFmClient>,
    pub http_client: reqwest::Client,
    pub players: DashMap<serenity::GuildId, Arc<GuildPlayer>>,
    /// Last search's ranked results + score breakdowns per guild, for `!why`.
    pub search_debug: Cache<serenity::GuildId, Arc<Vec<(Track, ScoreBreakdown)>>>,
}

impl BotData {
    /// Returns the existing player for this guild, or spawns a new one —
    /// loading its persisted stay_connected/autoplay settings first, so a
    /// freshly-spawned player (e.g. after a restart) honours them from the
    /// start rather than defaulting to off until someone re-toggles them.
    pub async fn player_for(&self, guild_id: serenity::GuildId) -> Arc<GuildPlayer> {
        if let Some(existing) = self.players.get(&guild_id) {
            return existing.clone();
        }
        let stay_connected = self.db.get_stay_connected(guild_id.get()).await;
        let autoplay = self.db.get_autoplay(guild_id.get()).await;
        self.players
            .entry(guild_id)
            .or_insert_with(|| {
                GuildPlayer::spawn(
                    guild_id,
                    self.songbird.clone(),
                    self.extractor.clone(),
                    self.http_client.clone(),
                    self.lastfm.clone(),
                    self.config.clone(),
                    stay_connected,
                    autoplay,
                )
            })
            .clone()
    }
}

pub type Context<'a> = poise::Context<'a, Arc<BotData>, anyhow::Error>;

pub fn setup_logging(config: &Config) -> anyhow::Result<()> {
    let filter = EnvFilter::try_new(&config.log_level).or_else(|_| EnvFilter::try_new("info"))?;

    if config.log_to_file {
        let appender = tracing_appender::rolling::never(&config.log_dir, "musicbot.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        // The guard must outlive the program to keep flushing to disk; there is
        // no natural owner for it, so it is intentionally leaked once at startup.
        Box::leak(Box::new(guard));
        fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .with_ansi(false)
            .init();
    } else {
        fmt().with_env_filter(filter).init();
    }
    Ok(())
}

pub async fn run(config: Config, db: Database) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let owners: HashSet<serenity::UserId> = config
        .bot_owners
        .iter()
        .map(|&id| serenity::UserId::new(id))
        .collect();

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    let setup_config = config.clone();
    let options = poise::FrameworkOptions {
        commands: crate::commands::all(),
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(config.default_prefix.clone()),
            dynamic_prefix: Some(|ctx| {
                Box::pin(async move {
                    let Some(guild_id) = ctx.guild_id else {
                        return Ok(None);
                    };
                    Ok(ctx.data.db.get_prefix(guild_id.get()).await)
                })
            }),
            case_insensitive_commands: true,
            ..Default::default()
        },
        owners,
        on_error: |error| Box::pin(on_error(error)),
        event_handler: |ctx, event, framework, data| {
            Box::pin(crate::events::handle_event(ctx, event, framework, data))
        },
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!(bot_user = %ready.user.name, "logged in");
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                let songbird = songbird::get(ctx)
                    .await
                    .expect("songbird manager not registered");
                let extractor = Arc::new(Extractor::new(setup_config.clone()));
                let http_client = reqwest::Client::new();
                let lastfm = setup_config
                    .lastfm_api_key
                    .clone()
                    .map(|key| LastFmClient::new(key, http_client.clone()));

                Ok(Arc::new(BotData {
                    config: setup_config,
                    db,
                    songbird,
                    extractor,
                    lastfm,
                    http_client,
                    players: DashMap::new(),
                    search_debug: Cache::builder()
                        .max_capacity(200)
                        .time_to_live(Duration::from_secs(30 * 60))
                        .build(),
                }))
            })
        })
        .options(options)
        .build();

    let mut client = serenity::ClientBuilder::new(config.token.clone(), intents)
        .framework(framework)
        .register_songbird()
        .await?;

    client.start().await.map_err(Into::into)
}

async fn on_error(error: poise::FrameworkError<'_, Arc<BotData>, anyhow::Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            tracing::error!(?error, "framework setup failed");
        }
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::warn!(command = %ctx.command().name, ?error, "command error");
            let reply = poise::CreateReply::default()
                .content(format!("Error: {error}"))
                .ephemeral(true);
            let _ = ctx.send(reply).await;
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!(?e, "error while handling a framework error");
            }
        }
    }
}
