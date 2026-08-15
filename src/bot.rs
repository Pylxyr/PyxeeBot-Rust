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

pub struct BotData {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub songbird: Arc<Songbird>,
    pub extractor: Arc<Extractor>,
    pub lastfm: Option<LastFmClient>,
    pub lyrics: crate::lyrics::LyricsClient,
    pub http_client: reqwest::Client,
    pub http: Arc<serenity::Http>,
    pub players: DashMap<serenity::GuildId, Arc<GuildPlayer>>,

    pub recent_searches: Cache<serenity::GuildId, Arc<Vec<Track>>>,

    pub vibe_history: DashMap<serenity::GuildId, std::collections::VecDeque<String>>,

    pub skip_votes: DashMap<serenity::GuildId, (String, HashSet<u64>)>,

    pub np_refreshers: DashMap<serenity::GuildId, tokio::task::AbortHandle>,
    pub recent_logs: crate::logbuf::RecentLogs,
}

impl BotData {

    pub async fn player_for(&self, guild_id: serenity::GuildId) -> Arc<GuildPlayer> {
        if let Some(existing) = self.players.get(&guild_id) {
            return existing.clone();
        }
        let stay_connected = self.db.get_stay_connected(guild_id.get()).await;
        let autoplay = self.db.get_autoplay(guild_id.get()).await;
        let volume = self.db.get_volume(guild_id.get()).await;
        self.players
            .entry(guild_id)
            .or_insert_with(|| {
                GuildPlayer::spawn(
                    guild_id,
                    self.songbird.clone(),
                    self.extractor.clone(),
                    self.http_client.clone(),
                    self.http.clone(),
                    self.lastfm.clone(),
                    self.config.clone(),
                    self.db.clone(),
                    stay_connected,
                    autoplay,
                    volume,
                )
            })
            .clone()
    }
}

pub type Context<'a> = poise::Context<'a, Arc<BotData>, anyhow::Error>;

pub fn setup_logging(config: &Config) -> anyhow::Result<crate::logbuf::RecentLogs> {
    use tracing_subscriber::prelude::*;

    let base = if config.log_level.trim().is_empty() {
        "info"
    } else {
        config.log_level.as_str()
    };
    let directive_str =
        format!("{base},pyxeebot=debug,songbird=debug,symphonia_core=debug,symphonia=debug");
    let fallback = "info,pyxeebot=debug,songbird=debug,symphonia_core=debug,symphonia=debug";
    let filter = EnvFilter::try_new(&directive_str).or_else(|_| EnvFilter::try_new(fallback))?;

    let stdout_layer = fmt::layer().with_ansi(false);
    let recent_logs = crate::logbuf::RecentLogs::new();

    let recent_logs_layer = fmt::layer()
        .with_writer(recent_logs.clone())
        .with_ansi(false)
        .with_filter(tracing_subscriber::filter::LevelFilter::WARN);
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(recent_logs_layer);

    if config.log_to_file {
        let appender = tracing_appender::rolling::never(&config.log_dir, "musicbot.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);

        Box::leak(Box::new(guard));
        let file_layer = fmt::layer().with_writer(writer).with_ansi(false);
        registry.with(file_layer).init();
    } else {
        registry.init();
    }
    Ok(recent_logs)
}

pub async fn run(config: Config, db: Database, recent_logs: crate::logbuf::RecentLogs) -> anyhow::Result<()> {
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

                let api_http_client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()?;
                let lastfm = setup_config
                    .lastfm_api_key
                    .clone()
                    .map(|key| LastFmClient::new(key, api_http_client.clone()));
                let lyrics = crate::lyrics::LyricsClient::new(api_http_client);

                let data = Arc::new(BotData {
                    config: setup_config,
                    db: Arc::new(db),
                    songbird,
                    extractor,
                    lastfm,
                    lyrics,
                    http_client,
                    http: ctx.http.clone(),
                    players: DashMap::new(),
                    recent_searches: Cache::builder()
                        .max_capacity(200)
                        .time_to_live(Duration::from_secs(30 * 60))
                        .build(),
                    vibe_history: DashMap::new(),
                    skip_votes: DashMap::new(),
                    np_refreshers: DashMap::new(),
                    recent_logs: recent_logs.clone(),
                });

                if data.config.restore_queue_on_restart {
                    let data = data.clone();
                    tokio::spawn(async move {
                        restore_queues(data).await;
                    });
                }

                {
                    let data = data.clone();
                    let http = ctx.http.clone();
                    tokio::spawn(async move {
                        watch_failures(data, http).await;
                    });
                }

                {
                    let db = data.db.clone();
                    tokio::spawn(async move {
                        checkpoint_wal_periodically(db).await;
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
            if let Err(e) = player.play(track, false, channel_id, channel_id).await {
                tracing::warn!(%guild_id, error = %e, "restore_queues: failed to re-queue a track");
            }
        }
        tracing::info!(%guild_id, %channel_id, track_count, "restore_queues: restored");
    }
}

async fn watch_failures(data: Arc<BotData>, http: Arc<serenity::Http>) {
    const CHECK_INTERVAL: Duration = Duration::from_secs(60);
    const FAILURE_THRESHOLD: u32 = 5;

    let mut resolve_alerted = false;
    let mut playback_alerted = false;
    loop {
        tokio::time::sleep(CHECK_INTERVAL).await;

        let resolve_streak = data.extractor.consecutive_resolve_failures();
        if resolve_streak >= FAILURE_THRESHOLD && !resolve_alerted {
            resolve_alerted = true;
            let header =
                format!("⚠️ PyxeeBot has hit **{resolve_streak} consecutive** yt-dlp resolve failures.");
            alert_owners(&data, &http, &header).await;
        } else if resolve_streak == 0 {
            resolve_alerted = false;
        }

        let playback_streak = data.extractor.consecutive_playback_failures();
        if playback_streak >= FAILURE_THRESHOLD && !playback_alerted {
            playback_alerted = true;
            let header = format!(
                "⚠️ PyxeeBot has hit **{playback_streak} consecutive** playback failures (resolved fine, but songbird couldn't stream it)."
            );
            alert_owners(&data, &http, &header).await;
        } else if playback_streak == 0 {
            playback_alerted = false;
        }
    }
}

async fn alert_owners(data: &BotData, http: &serenity::Http, header: &str) {
    tracing::warn!(header, "watch_failures: alerting owners");
    let logs = data.recent_logs.snapshot();

    const LOG_BUDGET: usize = 1500;
    let logs = if logs.len() > LOG_BUDGET {
        let cutoff = logs.len() - LOG_BUDGET;
        let safe_cutoff = (cutoff..=logs.len())
            .find(|&i| logs.is_char_boundary(i))
            .unwrap_or(logs.len());
        format!("...(truncated)...\n{}", &logs[safe_cutoff..])
    } else {
        logs
    };
    let content = format!("{header}\n```\n{logs}\n```\nFull history: `journalctl -u pyxeebotr`.");
    for owner in &data.config.bot_owners {
        let builder = serenity::CreateMessage::new().content(content.clone());
        if let Err(e) = serenity::UserId::new(*owner)
            .direct_message(http, builder)
            .await
        {
            tracing::warn!(owner, error = %e, "watch_failures: failed to DM owner");
        }
    }
}

async fn checkpoint_wal_periodically(db: Arc<Database>) {
    const INTERVAL: Duration = Duration::from_secs(20 * 60);
    loop {
        tokio::time::sleep(INTERVAL).await;
        if let Err(e) = db.checkpoint_truncate().await {
            tracing::warn!(error = %e, "checkpoint_wal_periodically: checkpoint failed");
        }
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
                    .content("That command hit an error. It's been logged.")
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
