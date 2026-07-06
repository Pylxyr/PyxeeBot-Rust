mod admin;
mod curation;
mod helpers;
mod misc;
mod playback;
mod playlist;
mod queue;
mod search;

use std::sync::Arc;

use crate::bot::BotData;

type Error = anyhow::Error;

pub fn all() -> Vec<poise::Command<Arc<BotData>, Error>> {
    vec![
        misc::ping(),
        // playback
        playback::join(),
        playback::leave(),
        playback::play(),
        playback::skip(),
        playback::stop(),
        playback::pause(),
        playback::resume(),
        playback::previous(),
        playback::loop_cmd(),
        playback::nowplaying(),
        // queue
        queue::queue(),
        queue::clear(),
        queue::shuffle(),
        queue::move_track_cmd(),
        queue::remove(),
        queue::history(),
        queue::toptracks(),
        queue::toprequestors(),
        // search
        search::search(),
        search::why(),
        // playlist (subcommands registered internally)
        playlist::playlist(),
        // curation
        curation::vibe(),
        curation::autoplay(),
        // admin
        admin::stay(),
        admin::setdj(),
        admin::cleardj(),
        admin::dj(),
        admin::setprefix(),
        admin::stats(),
    ]
}
