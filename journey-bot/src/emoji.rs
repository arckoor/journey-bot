use std::sync::Arc;

use poise::serenity_prelude::{self as serenity, EmojiId, GuildId, Http};

use crate::config::EmojiConfig;

pub enum Emoji {
    Ban,
    Feed,
    Info,
    Join,
    Sticky,
    Twitch,
    Warning,
}

pub struct EmojiStore {
    ban: serenity::Emoji,
    feed: serenity::Emoji,
    info: serenity::Emoji,
    join: serenity::Emoji,
    sticky: serenity::Emoji,
    twitch: serenity::Emoji,
    warning: serenity::Emoji,
}

impl EmojiStore {
    pub async fn new(ctx: Arc<Http>, admin_guild: u64, config: EmojiConfig) -> Self {
        let guild_id = GuildId::new(admin_guild);
        Self {
            ban: Self::get_emoji(&ctx, guild_id, config.ban).await,
            feed: Self::get_emoji(&ctx, guild_id, config.feed).await,
            info: Self::get_emoji(&ctx, guild_id, config.info).await,
            join: Self::get_emoji(&ctx, guild_id, config.join).await,
            sticky: Self::get_emoji(&ctx, guild_id, config.sticky).await,
            twitch: Self::get_emoji(&ctx, guild_id, config.twitch).await,
            warning: Self::get_emoji(&ctx, guild_id, config.warn).await,
        }
    }

    async fn get_emoji(ctx: &Arc<Http>, guild: GuildId, id: u64) -> serenity::Emoji {
        ctx.get_emoji(guild, EmojiId::new(id)).await.unwrap()
    }

    pub fn get(&self, category: Emoji) -> String {
        let emoji = match category {
            Emoji::Ban => &self.ban,
            Emoji::Feed => &self.feed,
            Emoji::Info => &self.info,
            Emoji::Join => &self.join,
            Emoji::Sticky => &self.sticky,
            Emoji::Twitch => &self.twitch,
            Emoji::Warning => &self.warning,
        };

        emoji.to_string()
    }
}
