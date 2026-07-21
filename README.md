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

**Playback** — `join`, `leave`, `play` (also resolves a pasted URL directly, not just search terms), `playnext` (queue at the front, ahead of everything else), `skip`, `stop`, `pause`, `resume`, `previous`, `loop`, `nowplaying` (with Pause/Skip/Loop buttons, and an optional auto-refreshing message — see `NP_AUTO_REFRESH`)

**Queue** — `queue`, `clear`, `shuffle`, `move`, `remove`, `history`, `toptracks`, `toprequestors`

**Search** — `search` (with a select-menu to pick a result directly), `why` (explains a search result's score breakdown)

**Playlists** — `playlist save/load/list/show/delete`

**Curation** — `vibe` (Last.fm-powered similar-artist discovery), `autoplay` (auto-queues a similar track instead of going idle — requires `LASTFM_API_KEY`)

**Admin** — `stay` (24/7 mode), `setdj`, `cleardj`, `dj`, `setprefix` (per-guild custom prefix), `stats`

**Permissions** — `clear`/`shuffle`/`move` need the DJ role or Manage Channels; `remove` needs to be the track's requester or a DJ; `skip`/`stop`/`pause`/`resume`/`previous`/`loop` need to be in the same voice channel as the bot (DJs are exempt from that last one, same as everywhere else).

## Architecture

- **`player/`** — an actor per guild (`PlayerActor`), driven by an mpsc command channel, broadcasting read-only state via a `watch` channel (`GuildPlayer` is the public handle). `PlayerState` (`queue.rs`) is the pure, fully-tested queue/loop-mode state machine — deliberately kept independent of songbird types.
- **`scoring/`** — search-result ranking engine (title/uploader overlap, fuzzy matching, discouraged-token penalties, JP-original detection, recency/view bonuses, etc). Feeds both `!search` and `!vibe`.
- **`extraction/`** — yt-dlp subprocess wrapper + a moka-backed resolve cache (TTL-based, no manual expiry bookkeeping). Two independent semaphores gate concurrency: `ytdlp_concurrent_extracts` for full per-video resolves, `ytdlp_curation_concurrency` for lighter `--flat-playlist` search listings. A few things are specifically tuned for time-to-first-audio: `!play` overlaps the "Searching..." reply with the actual search instead of sending it first; connecting to voice and speculatively resolving the picked track's stream both run concurrently (`tokio::join!`) instead of back-to-back; a pasted URL's extraction result primes the resolve cache so that speculative resolve is a hit instead of a redundant second full extraction; and background prefetch of upcoming tracks (`try_resolve_stream`) never queues for the single extract permit — if it's busy, prefetch just skips that round rather than sitting in front of an urgent on-demand resolve (e.g. from `!skip`).
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
| `NP_AUTO_REFRESH` | `false` | if set, `!nowplaying` edits its message in place every `NP_AUTO_REFRESH_INTERVAL` seconds instead of being a one-shot snapshot. Stops on its own once nothing's playing, the edit starts failing (message deleted), or after 2 hours, whichever comes first |
| `NP_AUTO_REFRESH_INTERVAL` | `30` | seconds, min `15` |
| `RESTORE_QUEUE_ON_RESTART` | `false` | reconnect and replay each guild's queue after a restart. **Off by default on purpose** — `release.yml` restarts the live service on every push to `main`, and turning this on means every routine deploy also auto-rejoins voice and resumes playback for every guild that was active. Turn it on if that's what you want; leave it off if you'd rather restarts be a clean stop |
| `BOT_ACTIVITY_URL` | `pylxyr.github.io/PyxeeBot-Page/` | shown as the bot's Discord presence ("Watching \<url\>"). Set to an empty string to disable |

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

Also runs in CI on every push to `main` (see [CI](#ci) below) — `build`/`deploy` won't proceed if this fails.

## Deploy

Deployment is fully automated via `.github/workflows/release.yml`: every push to `main` builds a release binary and does an atomic swap + `systemctl restart pyxeebotr` on the server over SSH, with automatic rollback if the service doesn't come back up. Pushing a `v*` tag also builds and attaches the binary to a GitHub Release, but only a push to `main` triggers the actual deploy step. Nothing manual is needed for a normal deploy — just push to `main`.

`deploy.sh` + `deploy/pyxeebotr.service` are a manual fallback (e.g. for a fresh server, or deploying without going through GitHub Actions) — both target the same `pyxeebotr` service at `~/pyxeebotr` that `release.yml` uses, so they should stay in sync with it if that ever changes.

## CI

`.github/workflows/release.yml` runs two jobs on every push to `main` (and `workflow_dispatch`):

- **`test`** — `cargo test --all-features`
- **`lint`** — `cargo clippy --all-targets --all-features -- -D warnings`
- **`build`** — only runs once `test` and `lint` both pass; builds the release binary and (on a `v*` tag) attaches it to a GitHub Release

`deploy` still only runs on a push to `main`, after `build` succeeds.

First run after adding `test`/`lint` as real gates might turn up pre-existing warnings this session didn't touch (clippy in particular, since `-D warnings` is strict and nothing enforced it before) — that's expected the first time, not a sign something broke.

(A `cargo audit` job was tried and removed — the findings weren't covered by `.cargo/audit.toml`'s existing exceptions, and the action needs `checks: write` permission this workflow doesn't grant, which errored on every run regardless of findings. `.cargo/audit.toml` is still there if you want to run `cargo audit` locally or wire it up properly later.)
