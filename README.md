<div align="center">

# 🎵 PyxeeBot (Rust)

**A self-hosted Discord music bot** — serenity + poise + songbird, FFmpeg-free, tuned to run comfortably on a single-vCPU box.

![Rust Edition](https://img.shields.io/badge/edition-2021-orange?logo=rust&logoColor=white)
[![Build + Deploy](https://img.shields.io/github/actions/workflow/status/pylxyr/PyxeeBot-Rust/release.yml?branch=main&label=build%20%2B%20deploy&logo=githubactions&logoColor=white)](https://github.com/pylxyr/PyxeeBot-Rust/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
![Discord](https://img.shields.io/badge/platform-Discord-5865F2?logo=discord&logoColor=white)

![serenity](https://img.shields.io/badge/serenity-0.12.5-blue)
![poise](https://img.shields.io/badge/poise-0.6.2-blue)
![songbird](https://img.shields.io/badge/songbird-0.6.0-blue)
![tokio](https://img.shields.io/badge/tokio-1.52-blue)
![sqlx](https://img.shields.io/badge/sqlx-0.9-blue)


</div>

---

## Table of Contents

- ✨ [Features](#features)
- 🏗️ [Architecture](#architecture)
- ⚙️ [Configuration](#configuration)
- 📦 [Requirements](#requirements)
- 🔨 [Build](#build)
- 🧪 [Test](#test)
- 🚀 [Deploy](#deploy)
- 🔄 [CI](#ci)
- 📄 [License](#license)

---

## Features

A Rust rewrite of the original Python PyxeeBot, built for a self-hosted Oracle Cloud E2.Micro instance — audio is decoded entirely in-process via Symphonia, so there's no `ffmpeg` dependency at all.

| Category | Commands |
|---|---|
| **Playback** | `join`, `leave`, `play` *(resolves a pasted URL directly, not just search terms)*, `playnext` *(queue at the front, ahead of everything else)*, `skip`, `stop`, `pause`, `resume`, `previous`, `loop`, `nowplaying` *(Pause/Skip/Loop buttons, optional auto-refreshing message — see `NP_AUTO_REFRESH`)* |
| **Queue** | `queue`, `clear`, `shuffle`, `move`, `remove`, `history`, `toptracks`, `toprequestors` |
| **Search** | `search` *(select-menu to pick a result directly)*, `why` *(explains a search result's score breakdown)* |
| **Playlists** | `playlist save`, `playlist load`, `playlist list`, `playlist show`, `playlist delete` |
| **Curation** | `vibe` *(Last.fm-powered similar-artist discovery)*, `autoplay` *(auto-queues a similar track instead of going idle — requires `LASTFM_API_KEY`)* |
| **Admin** | `stay` *(24/7 mode)*, `setdj`, `cleardj`, `dj`, `setprefix` *(per-guild custom prefix)*, `stats` |

### Permissions

| Commands | Requirement |
|---|---|
| `clear`, `shuffle`, `move` | DJ role or Manage Channels |
| `remove` | The track's requester, or a DJ |
| `skip`, `stop`, `pause`, `resume`, `previous`, `loop` | Same voice channel as the bot (DJs exempt) |

---

## Architecture

| Module | What it does |
|---|---|
| `player/` | One actor per guild (`PlayerActor`), driven by an mpsc command channel, broadcasting read-only state via a `watch` channel (`GuildPlayer` is the public handle). `PlayerState` (`queue.rs`) is a pure, fully-tested queue/loop-mode state machine, deliberately kept independent of songbird types. |
| `scoring/` | Search-result ranking engine — title/uploader overlap, fuzzy matching, discouraged-token penalties, JP-original detection, recency/view bonuses, and more. Feeds both `!search` and `!vibe`. |
| `extraction/` | yt-dlp subprocess wrapper plus two moka-backed caches (TTL-based, no manual expiry bookkeeping): a resolve cache for stream URLs and a search-results cache shared across `!play`/`!vibe`/`!search`. Two independent semaphores gate concurrency — `ytdlp_concurrent_extracts` for full per-video resolves, `ytdlp_curation_concurrency` for lighter `--flat-playlist` search listings. |
| `db/` | SQLite via sqlx, WAL mode. Guild settings are cached in-memory (`DashMap`) after first read. |
| `events.rs` | Voice-state tracking (empty-channel/stay-connected disconnect logic, force-kick rejoin) and component-interaction routing (buttons, select menus). |
| `lastfm.rs` | Minimal `artist.getsimilar` / `track.search` / `track.getsimilar` client for curation features. |

### Time-to-first-audio

A handful of things are specifically tuned to get audio playing as fast as possible:

- `!play` overlaps the "Searching..." reply with the actual search, instead of sending it first and waiting.
- Connecting to voice and speculatively resolving the picked track's stream run concurrently (`tokio::join!`) instead of back-to-back.
- A pasted URL's extraction result primes the resolve cache, so the speculative resolve above is a hit instead of a redundant second full extraction.
- Background prefetch of upcoming tracks never queues for the single extract permit — if it's busy, prefetch just skips that round rather than sitting in front of an urgent on-demand resolve (e.g. from `!skip`).
- `!vibe`'s searches run concurrently instead of one at a time (queueing still happens sequentially, to preserve result order).
- The queue snapshot persisted for `restore_queue_on_restart` is hashed before cloning, so an unchanged queue costs a hash compare instead of a full clone on every command.
- Repeat search queries across `!play`/`!vibe`/`!search` hit a raw-results cache instead of re-running yt-dlp, shared across all three regardless of which one populated it.

What's *not* fixable from here: on constrained hardware, a cold (never-searched, never-resolved) track still pays real yt-dlp cost — a flat-playlist search, plus a full extraction that includes solving YouTube's JS anti-bot challenge. That's genuine CPU-bound work; profiling on a single-vCPU box showed the challenge-solving step alone accounting for several seconds of real user-space CPU time, confirmed independently of the bot via a standalone `yt-dlp` timing run. There's no code-level fix for that part.

---

## Configuration

All configuration is via environment variables (typically a `.env` file next to the binary — there's no `.env.example` checked in, so these tables are the reference). Only `DISCORD_TOKEN` is required; everything else has a default.

### Core / Discord

| Variable | Default | Notes |
|---|---|---|
| `DISCORD_TOKEN` | — | **required** |
| `BOT_OWNERS` | *(none)* | comma-separated Discord user IDs |
| `DEFAULT_PREFIX` | `!` | no spaces allowed |
| `LOG_LEVEL` | `INFO` | pyxeebot/songbird/symphonia are always forced to at least `debug` regardless |
| `LOG_TO_FILE` | `true` | stdout (journalctl) is always on; this adds a file as a second sink |
| `LOG_DIR` | `logs` | relative to the working directory |

### Queue & Limits

| Variable | Default | Notes |
|---|---|---|
| `MAX_QUEUE_SIZE` | `100` | per guild |
| `MAX_QUEUE_SIZE_PER_USER` | `0` | `0` = unlimited |
| `MAX_PLAYLIST_SIZE` | `25` | |
| `IDLE_TIMEOUT_SECONDS` | `180` | min `30` |
| `EMPTY_CHANNEL_TIMEOUT_SECONDS` | `60` | min `15` |

### yt-dlp & Extraction

| Variable | Default | Notes |
|---|---|---|
| `YTDLP_COOKIES_FILE` | *(none)* | path, relative or absolute |
| `YTDLP_JS_RUNTIME_PATH` | *(none)* | e.g. a `node`/`deno` path, for YouTube's JS challenge solving |
| `YTDLP_POT_PROVIDER_BASE_URL` | *(none)* | PO token provider (e.g. bgutil-ytdlp-pot-provider) |
| `YTDLP_SOCKET_TIMEOUT` | `15` | seconds, min `5` |
| `YTDLP_PREFETCH_COUNT` | `1` | tracks ahead to speculatively resolve |
| `YTDLP_CONCURRENT_EXTRACTS` | `1` | full per-video resolves (stream URL) run at this concurrency — kept at 1 by default to avoid CPU contention on a single-vCPU box |
| `YTDLP_CURATION_CONCURRENCY` | `3` | clamped 1–6; concurrency for lighter `--flat-playlist` search listings, separate from the above |
| `NEAR_END_PREFETCH_SECONDS` | `30` | when to start resolving the next track |
| `YTDLP_SEARCH_RESULTS` | `5` | clamped 1–10; candidates fetched for `!play`/`!vibe` |
| `YTDLP_RESOLVE_CACHE_SIZE` | `128` | resolved-stream cache entries, min `16` |
| `YTDLP_RESOLVE_CACHE_TTL_SECONDS` | `1800` | min `60` |
| `YTDLP_SEARCH_CACHE_SIZE` | `200` | cached raw search-result sets, min `16` |
| `YTDLP_SEARCH_CACHE_TTL_SECONDS` | `600` | min `30`; repeat `!play`/`!vibe`/`!search` queries within this window skip yt-dlp entirely |
| `YTDLP_EXTRACT_TIMEOUT_SECONDS` | `45` | min `5` |

### Playback & Presence

| Variable | Default | Notes |
|---|---|---|
| `OPUS_BITRATE_KBPS` | `64` | clamped 64–256 |
| `ERROR_ANNOUNCE` | `true` | whether command errors get an in-channel reply (they're always logged either way) |
| `NP_AUTO_REFRESH` | `false` | if set, `!nowplaying` edits its message in place every `NP_AUTO_REFRESH_INTERVAL` seconds instead of being a one-shot snapshot. Stops once nothing's playing, the edit starts failing, or after 2 hours — whichever comes first |
| `NP_AUTO_REFRESH_INTERVAL` | `30` | seconds, min `15` |
| `RESTORE_QUEUE_ON_RESTART` | `false` | reconnect and replay each guild's queue after a restart. **Off by default on purpose** — `release.yml` restarts the live service on every push to `main`, and turning this on means every routine deploy also auto-rejoins voice and resumes playback for every guild that was active |
| `BOT_ACTIVITY_URL` | `pylxyr.github.io/PyxeeBot-Page/` | shown as the bot's Discord presence ("Watching \<url\>"). Set to an empty string to disable |

### Curation

| Variable | Default | Notes |
|---|---|---|
| `LASTFM_API_KEY` | *(none)* | enables `!vibe` / `!autoplay` |

---

## Requirements

- Rust, edition 2021 (see [Build](#build))
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on `PATH`
- A JS runtime (e.g. `node` or `deno`) is **recommended** but not required — set `YTDLP_JS_RUNTIME_PATH` if YouTube's JS challenges start causing failures
- `ffmpeg` is **not** required — every yt-dlp call here is `--dump-json` (metadata + a direct stream URL, no download), and Songbird decodes the resulting stream in-process via Symphonia

---

## Build

```bash
cargo build --release
```

Requires a C toolchain (MSVC on Windows, or `build-essential`/`cmake` on Linux) for `libopus`/bundled SQLite. On Ubuntu/WSL:

```bash
sudo apt install build-essential cmake libopus-dev
```

---

## Test

```bash
cargo test --all-features
```

Covers: scoring engine (golden-style ranking tests), player queue state machine (the exact regression classes fixed in the Python version — `total_duration` eviction accounting, `play_previous`, loop-mode requeue, `stay_connected`/idle guards), extraction argument builders, database round-trips, and UI content formatting.

Also runs in CI on every push to `main` (see [CI](#ci) below) — `build`/`deploy` won't proceed if this fails.

---

## Deploy

Deployment is fully automated via `.github/workflows/release.yml`: every push to `main` builds a release binary and does an atomic swap + `systemctl restart pyxeebotr` on the server over SSH, with automatic rollback if the service doesn't come back up. Pushing a `v*` tag also builds and attaches the binary to a GitHub Release, but only a push to `main` triggers the actual deploy step. Nothing manual is needed for a normal deploy — just push to `main`.

`deploy.sh` + `deploy/pyxeebotr.service` are a manual fallback (e.g. for a fresh server, or deploying without going through GitHub Actions) — both target the same `pyxeebotr` service at `~/pyxeebotr` that `release.yml` uses, so they should stay in sync with it if that ever changes.

> ⚠️ **Migration files are append-only once deployed.** sqlx checksums each migration file; changing so much as a comment on one that's already run in production breaks startup with `migration N was previously applied but has been modified`. A fix always means a *new* migration file, never an edit to an old one.

---

## CI

`.github/workflows/release.yml` runs three jobs on every push to `main` (and `workflow_dispatch`):

| Job | What it does |
|---|---|
| `test` | `cargo test --all-features` |
| `lint` | `cargo clippy --all-targets --all-features -- -D warnings` |
| `build` | Only runs once `test` and `lint` both pass; builds the release binary and (on a `v*` tag) attaches it to a GitHub Release |

`deploy` still only runs on a push to `main`, after `build` succeeds.

`.cargo/audit.toml` exists with some documented `cargo audit` exceptions, but no CI job currently runs it — a prior attempt surfaced findings outside those exceptions and needed `checks: write` permission this workflow doesn't grant. Run `cargo audit` locally if you want it, or wire it up properly later.

---

## License

MIT — see [LICENSE](LICENSE).

<div align="center">

*<sub>The copyright holder name in <a href="LICENSE">LICENSE</a> was inferred from this repo's GitHub owner, not independently confirmed — update it if that's wrong.</sub>*

</div>
