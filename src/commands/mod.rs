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
        misc::lyrics(),

        playback::join(),
        playback::leave(),
        playback::play(),
        playback::playnext(),
        playback::skip(),
        playback::voteskip(),
        playback::stop(),
        playback::pause(),
        playback::resume(),
        playback::volume(),
        playback::previous(),
        playback::loop_cmd(),
        playback::nowplaying(),

        queue::queue(),
        queue::clear(),
        queue::shuffle(),
        queue::move_track_cmd(),
        queue::remove(),
        queue::history(),
        queue::toptracks(),
        queue::toprequestors(),
        queue::mystats(),

        search::search(),

        playlist::playlist(),

        curation::vibe(),
        curation::autoplay(),

        admin::stay(),
        admin::setdj(),
        admin::cleardj(),
        admin::dj(),
        admin::setprefix(),
        admin::stats(),
    ]
}
