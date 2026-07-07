# PyxeeBot (Rust)

A Discord music bot built on serenity + poise + songbird, running FFmpeg-free (Songbird 0.6 decodes audio in-process via Symphonia). Rust rewrite of the original Python PyxeeBot, targeting a self-hosted Oracle Cloud E2.Micro instance.

## Features

**Playback** — `join`, `leave`, `play`, `skip`, `stop`, `pause`, `resume`, `previous`, `loop`, `nowplaying` (with Pause/Skip/Loop buttons)

**Queue** — `queue`, `clear`, `shuffle`, `move`, `remove`, `history`, `toptracks`, `toprequestors`

**Search** — `search` (with a select-menu to pick a result directly), `why` (explains a search result's score breakdown)

**Playlists** — `playlist save/load/list/show/delete`

**Curation** — `vibe` (Last.fm-powered similar-artist discovery), `autoplay` (auto-queues a similar track instead of going idle — requires `LASTFM_API_KEY`)

**Admin** — `stay` (24/7 mode), `setdj`, `cleardj`, `dj`, `setprefix` (per-guild custom prefix), `stats`

## Architecture

- **`player/`** — an actor per guild (`PlayerActor`), driven by an mpsc command channel, broadcasting read-only state via a `watch` channel (`GuildPlayer` is the public handle). `PlayerState` (`queue.rs`) is the pure, fully-tested queue/loop-mode state machine — deliberately kept independent of songbird types.
- **`scoring/`** — search-result ranking engine (title/uploader overlap, fuzzy matching, discouraged-token penalties, JP-original detection, recency/view bonuses, etc). Feeds both `!search` and `!vibe`.
- **`extraction/`** — yt-dlp subprocess wrapper + a moka-backed resolve cache (TTL-based, no manual expiry bookkeeping).
- **`db/`** — SQLite via sqlx, WAL mode. Guild settings are cached in-memory (`DashMap`) after first read.
- **`events.rs`** — voice-state tracking (empty-channel/stay-connected disconnect logic, force-kick rejoin) and component-interaction routing (buttons, select menus).
- **`lastfm.rs`** — minimal `artist.getsimilar` client for curation features.

## Setup

Copy `deploy/.env.example` to `.env` and fill in:

- `DISCORD_TOKEN`, `BOT_OWNERS` — required
- `LASTFM_API_KEY` — optional, enables `!vibe`/`!autoplay`
- Everything else has a sensible default; see the file for the full list (queue limits, timeouts, yt-dlp tuning).

Requires `yt-dlp` and `ffmpeg` (yt-dlp uses it for some format conversions) installed on the host.

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

## Deploy

`deploy.sh` builds locally and ships the binary via `scp`, then restarts the systemd service (`deploy/musicbot.service`) on the remote host:

```bash
SERVER_HOST=your.host SERVER_USER=ubuntu ./deploy.sh
```

Compile on the same architecture as the target (x86_64 Linux) — WSL Ubuntu works directly, no cross-compilation needed.

## CI

Three parallel jobs (`lint`, `test`, `audit`) — see `.github/workflows/ci.yml`. Known, tracked `cargo audit` exceptions (both blocked on upstream releases — serenity's `tokio-tungstenite` pin, and openmls's `libcrux-chacha20poly1305` pin) are documented in `.cargo/audit.toml`.
