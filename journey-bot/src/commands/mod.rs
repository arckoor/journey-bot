use std::sync::Arc;

use tokio::sync::mpsc::Receiver;

use crate::{Command, commands::anti_spam::ChannelMessage, store::Store, utils::BotError};

pub mod anti_spam;
pub mod auto_role;
pub mod basic;
pub mod bypass;
pub mod censor;
pub mod feeds;
pub mod guild_config;
pub mod links;
pub mod sticky;
pub mod streams;

pub fn commands() -> Vec<Command> {
    vec![
        anti_spam::as_config(),
        auto_role::auto_role(),
        basic::ping(),
        basic::echo(),
        basic::presence(),
        basic::register(),
        bypass::bypass(),
        bypass::bypass_config(),
        censor::censor(),
        feeds::feed(),
        guild_config::guild_config(),
        links::link(),
        links::link_config(),
        sticky::stick(),
        streams::stream_observer(),
    ]
}

pub async fn schedule(store: Arc<Store>, rx: Receiver<ChannelMessage>) -> Result<(), BotError> {
    anti_spam::PoolManager::schedule(store.clone(), rx);
    bypass::BypassScheduler::schedule(store.clone());
    censor::CensorScheduler::schedule_all(store.clone()).await;
    feeds::RedditScheduler::schedule_all(store.clone()).await?;
    streams::TwitchScheduler::schedule_all(store.clone()).await?;

    Ok(())
}
