// Scaffolding ahead of consumers — some items here (DB re-exports, model
// methods, player state) land before every consumer does across Phase 2/3.
#![allow(dead_code, unused_imports)]

pub mod bot;
pub mod commands;
pub mod components;
pub mod config;
pub mod constants;
pub mod db;
pub mod errors;
pub mod events;
pub mod extraction;
pub mod lastfm;
pub mod models;
pub mod player;
pub mod scoring;
