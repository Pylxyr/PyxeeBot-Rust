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
    pub db: Arc<Database>,
    pub songbird: Arc<Songbird>,
    pub extractor: Arc<Extractor>,
    pub lastfm: Option<LastFmClient>,
    pub http_client: reqwest::Client,
    pub players: DashMap<serenity::GuildId, Arc<GuildPlayer>>,
    /// Last search's ranked results + score breakdowns per guild, for `!why`.
    pub search_debug: Cache<serenity::GuildId, Arc<Vec<(Track, ScoreBreakdown)>>>,
    /// Recently `!vibe`-queued song keys per guild, oldest-first, capped at
    /// VIBE_HISTORY_CAP — lets vibe favour songs it hasn't just played.
    pub vibe_history: DashMap<serenity::GuildId, std::collections::VecDeque<String>>,
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
                    self.db.clone(),
                    stay_connected,
                    autoplay,
                )
            })
            .clone()
    }
}

pub type Context<'a> = poise::Context<'a, Arc<BotData>, anyhow::Error>;

pub fn setup_logging(config: &Config) -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    // The configured LOG_LEVEL sets the baseline for everything else (so
    // noisy dependencies like h2/rustls/tungstenite stay quiet at "info"),
    // but pyxeebot's own code plus songbird/symphonia — the crates that
    // actually matter when chasing a playback bug — are always forced to
    // at least debug. This is a floor, not a ceiling: setting LOG_LEVEL to
    // something louder (e.g. "trace") still raises these further.
    let base = if config.log_level.trim().is_empty() {
        "info"
    } else {
        config.log_level.as_str()
    };
    let directive_str =
        format!("{base},pyxeebot=debug,songbird=debug,symphonia_core=debug,symphonia=debug");
    let fallback = "info,pyxeebot=debug,songbird=debug,symphonia_core=debug,symphonia=debug";
    let filter = EnvFilter::try_new(&directive_str).or_else(|_| EnvFilter::try_new(fallback))?;

    // Always emit to stdout — under systemd (Type=simple) this is captured
    // by journalctl automatically (`journalctl -u pyxeebotr -f`), so verbose
    // diagnostics are never gated behind remembering to tail a file. If
    // LOG_TO_FILE is also set, the file becomes a second destination rather
    // than the only one.
    let stdout_layer = fmt::layer().with_ansi(false);
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer);

    if config.log_to_file {
        let appender = tracing_appender::rolling::never(&config.log_dir, "musicbot.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        // The guard must outlive the program to keep flushing to disk; there is
        // no natural owner for it, so it is intentionally leaked once at startup.
        Box::leak(Box::new(guard));
        let file_layer = fmt::layer().with_writer(writer).with_ansi(false);
        registry.with(file_layer).init();
    } else {
        registry.init();
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

                if !setup_config.bot_activity_url.is_empty() {
                    ctx.set_activity(Some(serenity::ActivityData::watching(
                        setup_config.bot_activity_url.clone(),
                    )));
                }

                let songbird = songbird::get(ctx)
                    .await
                    .expect("songbird manager not registered");
                let extractor = Arc::new(Extractor::new(setup_config.clone()));
                let http_client = reqwest::Client::new();
                let lastfm = setup_config
                    .lastfm_api_key
                    .clone()
                    .map(|key| LastFmClient::new(key, http_client.clone()));

                let data = Arc::new(BotData {
                    config: setup_config,
                    db: Arc::new(db),
                    songbird,
                    extractor,
                    lastfm,
                    http_client,
                    players: DashMap::new(),
                    search_debug: Cache::builder()
                        .max_capacity(200)
                        .time_to_live(Duration::from_secs(30 * 60))
                        .build(),
                    vibe_history: DashMap::new(),
                });

                if data.config.restore_queue_on_restart {
                    let data = data.clone();
                    tokio::spawn(async move {
                        restore_queues(data).await;
                    });
                }

                Ok(data)
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

/// Reconnects and replays each guild's saved queue on startup.
async fn restore_queues(data: Arc<BotData>) {
    let guilds = match data.db.list_restorable_guilds().await {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(error = %e, "restore_queues: failed to list restorable guilds");
            return;
        }
    };
    if guilds.is_empty() {
        return;
    }
    tracing::info!(count = guilds.len(), "restore_queues: restoring queues");

    for (guild_id, channel_id) in guilds {
        let tracks = match data.db.load_queue_snapshot(guild_id).await {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!(guild_id = guild_id, error = %e, "restore_queues: failed to load snapshot");
                continue;
            }
        };

        let guild_id = serenity::GuildId::new(guild_id);
        let channel_id = serenity::ChannelId::new(channel_id);
        let player = data.player_for(guild_id).await;

        if let Err(e) = player.connect(channel_id).await {
            tracing::warn!(%guild_id, %channel_id, error = %e, "restore_queues: failed to reconnect");
            continue;
        }

        let track_count = tracks.len();
        for track in tracks {
            if let Err(e) = player.play(track, false, channel_id).await {
                tracing::warn!(%guild_id, error = %e, "restore_queues: failed to re-queue a track");
            }
        }
        tracing::info!(%guild_id, %channel_id, track_count, "restore_queues: restored");
    }
}

async fn on_error(error: poise::FrameworkError<'_, Arc<BotData>, anyhow::Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            tracing::error!(?error, "framework setup failed");
        }
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::warn!(command = %ctx.command().name, ?error, "command error");
            if ctx.data().config.error_announce {
                let reply = poise::CreateReply::default()
                    .content(format!("Error: {error}"))
                    .ephemeral(true);
                let _ = ctx.send(reply).await;
            }
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!(?e, "error while handling a framework error");
            }
        }
    }
}
