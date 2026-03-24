use crate::Command;

pub mod anti_spam;
pub mod auto_role;
pub mod basic;
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
        censor::censor(),
        feeds::feed(),
        guild_config::guild_config(),
        links::link(),
        links::link_config(),
        sticky::stick(),
        streams::stream_observer(),
    ]
}
