# PyxeeBot (Rust)

![Rust Edition](https://img.shields.io/badge/edition-2021-orange?logo=rust)
[![Release](https://img.shields.io/github/actions/workflow/status/pylxyr/PyxeeBot-Rust/release.yml?branch=main&label=build%20%2B%20deploy)](https://github.com/pylxyr/PyxeeBot-Rust/actions/workflows/release.yml)
![License](https://img.shields.io/badge/license-unspecified-lightgrey)
![Discord](https://img.shields.io/badge/platform-Discord-5865F2?logo=discord&logoColor=white)

> The build badge's `pylxyr/PyxeeBot-Rust` path is inferred from `BOT_ACTIVITY_URL`'s
> default (`pylxyr.github.io/...`) and this zip's folder name — not independently
> confirmed. Fix the owner/repo in the URL above if that's wrong. There's also no
> `LICENSE` file or `license` field in `Cargo.toml` yet, so the license badge is
> intentionally a placeholder — add one (e.g. MIT/Apache-2.0) if you want the repo
> to say something more specific than "unspecified."

A Discord music bot built on serenity + poise + songbird, running FFmpeg-free (Songbird 0.6 decodes audio in-process via Symphonia). Rust rewrite of the original Python PyxeeBot, targeting a self-hosted Oracle Cloud E2.Micro instance.

## Features

**Playback** — `join`, `leave`, `play` (also resolves a pasted URL directly, not just search terms), `skip`, `stop`, `pause`, `resume`, `previous`, `loop`, `nowplaying` (with Pause/Skip/Loop buttons)

**Queue** — `queue`, `clear`, `shuffle`, `move`, `remove`, `history`, `toptracks`, `toprequestors`

**Search** — `search` (with a select-menu to pick a result directly), `why` (explains a search result's score breakdown)

**Playlists** — `playlist save/load/list/show/delete`

**Curation** — `vibe` (Last.fm-powered similar-artist discovery), `autoplay` (auto-queues a similar track instead of going idle — requires `LASTFM_API_KEY`)

**Admin** — `stay` (24/7 mode), `setdj`, `cleardj`, `dj`, `setprefix` (per-guild custom prefix), `stats`

## Architecture

- **`player/`** — an actor per guild (`PlayerActor`), driven by an mpsc command channel, broadcasting read-only state via a `watch` channel (`GuildPlayer` is the public handle). `PlayerState` (`queue.rs`) is the pure, fully-tested queue/loop-mode state machine — deliberately kept independent of songbird types.
- **`scoring/`** — search-result ranking engine (title/uploader overlap, fuzzy matching, discouraged-token penalties, JP-original detection, recency/view bonuses, etc). Feeds both `!search` and `!vibe`.
- **`extraction/`** — yt-dlp subprocess wrapper + a moka-backed resolve cache (TTL-based, no manual expiry bookkeeping). Two independent semaphores gate concurrency: `ytdlp_concurrent_extracts` for full per-video resolves, `ytdlp_curation_concurrency` for lighter `--flat-playlist` search listings.
- **`db/`** — SQLite via sqlx, WAL mode. Guild settings are cached in-memory (`DashMap`) after first read.
- **`events.rs`** — voice-state tracking (empty-channel/stay-connected disconnect logic, force-kick rejoin) and component-interaction routing (buttons, select menus).
- **`lastfm.rs`** — minimal `artist.getsimilar` / `track.search` / `track.getsimilar` client for curation features.

## Configuration

All via environment variables (typically a `.env` file next to the binary — there's no `.env.example` checked in, so this table is the reference). Only `DISCORD_TOKEN` is required; everything else has a default.

| Variable | Default | Notes |
|---|---|---|
| `DISCORD_TOKEN` | — | **required** |
| `BOT_OWNERS` | *(none)* | comma-separated Discord user IDs |
| `DEFAULT_PREFIX` | `!` | no spaces allowed |
| `LOG_LEVEL` | `INFO` | pyxeebot/songbird/symphonia are always forced to at least `debug` regardless |
| `LOG_TO_FILE` | `true` | stdout (journalctl) is always on; this adds a file as a second sink |
| `LOG_DIR` | `logs` | relative to the working directory |
| `MAX_QUEUE_SIZE` | `100` | per guild |
| `MAX_QUEUE_SIZE_PER_USER` | `0` | `0` = unlimited |
| `MAX_PLAYLIST_SIZE` | `25` | |
| `IDLE_TIMEOUT_SECONDS` | `180` | min `30` |
| `EMPTY_CHANNEL_TIMEOUT_SECONDS` | `60` | min `15` |
| `YTDLP_COOKIES_FILE` | *(none)* | path, relative or absolute |
| `YTDLP_JS_RUNTIME_PATH` | *(none)* | e.g. a `deno` path, for YouTube's JS challenge solving |
| `YTDLP_POT_PROVIDER_BASE_URL` | *(none)* | PO token provider (e.g. bgutil-ytdlp-pot-provider) |
| `YTDLP_SOCKET_TIMEOUT` | `15` | seconds, min `5` |
| `YTDLP_PREFETCH_COUNT` | `1` | tracks ahead to speculatively resolve |
| `YTDLP_CONCURRENT_EXTRACTS` | `1` | full per-video resolves (stream URL) run at this concurrency — kept at 1 by default to avoid CPU contention on a single-vCPU box |
| `YTDLP_CURATION_CONCURRENCY` | `3` | clamped 1–6; concurrency for lighter `--flat-playlist` search listings, separate from the above |
| `NEAR_END_PREFETCH_SECONDS` | `30` | when to start resolving the next track |
| `OPUS_BITRATE_KBPS` | `64` | clamped 64–256 |
| `YTDLP_SEARCH_RESULTS` | `5` | clamped 1–10; candidates fetched for `!play`/`!vibe` |
| `YTDLP_RESOLVE_CACHE_SIZE` | `128` | resolved-stream cache entries, min `16` |
| `YTDLP_RESOLVE_CACHE_TTL_SECONDS` | `1800` | min `60` |
| `YTDLP_EXTRACT_TIMEOUT_SECONDS` | `45` | min `5` |
| `ERROR_ANNOUNCE` | `true` | whether command errors get an in-channel reply (they're always logged either way) |
| `LASTFM_API_KEY` | *(none)* | enables `!vibe` / `!autoplay` |

### Parsed but not yet wired up

These are read from the environment and validated, but nothing in the code currently acts on them — set them and nothing will happen yet:

| Variable | Default | What it's meant for |
|---|---|---|
| `NP_AUTO_REFRESH` / `NP_AUTO_REFRESH_INTERVAL` | `false` / `30` | periodically refresh the `!nowplaying` message in place |
| `RESTORE_QUEUE_ON_RESTART` | `true` | reload each guild's queue after a bot restart — the DB layer (`save_queue_snapshot`/`load_queue_snapshot`) exists and is tested, but nothing calls it, and the schema doesn't store a `channel_id` to reconnect to yet |
| `BOT_ACTIVITY_URL` | `pylxyr.github.io/PyxeeBot-Page/` | intended for the bot's Discord presence/status; not currently set anywhere |

## Requirements

- Rust, edition 2021 (see [Build](#build))
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on `PATH`
- A JS runtime (e.g. `deno`) is **recommended** but not required — set `YTDLP_JS_RUNTIME_PATH` if YouTube's JS challenges start causing failures. `ffmpeg` is *not* required: every yt-dlp call here is `--dump-json` (metadata + a direct stream URL, no download), and Songbird decodes the resulting stream in-process via Symphonia.

## Build

```bash
cargo build --release
```

Requires a C toolchain (MSVC on Windows, or `build-essential`/`cmake` on Linux) for `libopus`/bundled SQLite. On Ubuntu/WSL:

```bash
sudo apt install build-essential cmake libopus-dev
```

## Test

```bash
cargo test --all-features
```

Covers: scoring engine (golden-style ranking tests), player queue state machine (the exact regression classes fixed in the Python version — `total_duration` eviction accounting, `play_previous`, loop-mode requeue, `stay_connected`/idle guards), extraction argument builders, database round-trips, and UI content formatting.

Note: `cargo test` isn't currently run in CI (see below) — it's on you to run it locally before pushing to `main`.

## Deploy

Deployment is fully automated via `.github/workflows/release.yml`: every push to `main` builds a release binary and does an atomic swap + `systemctl restart pyxeebotr` on the server over SSH, with automatic rollback if the service doesn't come back up. Pushing a `v*` tag also builds and attaches the binary to a GitHub Release, but only a push to `main` triggers the actual deploy step. Nothing manual is needed for a normal deploy — just push to `main`.

`deploy.sh` and `deploy/musicbot.service` are older, currently-stale leftovers from before that pipeline existed: they target a `musicbot` systemd unit at `~/musicbot`, not the `pyxeebotr` unit at `~/pyxeebotr` that `release.yml` actually deploys to. Don't run `deploy.sh` expecting it to hit the live service — either update it to match `pyxeebotr`/`~/pyxeebotr`, or remove it, whichever you'd rather do.

## CI

Currently just the one workflow — `.github/workflows/release.yml` — which builds and deploys on push to `main`. There's no separate lint/test/audit workflow in this repo (despite `.cargo/audit.toml` existing with some documented `cargo audit` exceptions — it's not currently invoked anywhere in CI). Worth adding a workflow that runs `cargo test`/`cargo clippy`/`cargo audit` on pull requests, since right now nothing catches a regression before it's live.
