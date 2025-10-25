use std::sync::Arc;

use poise::serenity_prelude as serenity;
use tokio::sync::mpsc::Sender;

use crate::{
    commands::{
        anti_spam::ChannelMessage, feeds::RedditClient, links::Links, sticky::StickyLock,
        streams::TwitchClient,
    },
    config::StoreConfig,
    db::Database,
    emoji::EmojiStore,
    utils::BotError,
};

pub struct Store {
    pub admin_guild: u64,
    pub embed_color: (u8, u8, u8),
    pub db: Database,
    pub emoji: EmojiStore,
    pub links: Links,
    pub reddit_client: RedditClient,
    pub twitch_client: TwitchClient,
    pub sticky: StickyLock,
    pub anti_spam_sender: Sender<ChannelMessage>,
    pub ctx: Arc<serenity::Http>,
}

impl Store {
    pub async fn new(
        config: StoreConfig,
        ctx: Arc<serenity::Http>,
        anti_spam_sender: Sender<ChannelMessage>,
    ) -> Result<Self, BotError> {
        let StoreConfig { setup, emoji, api } = config;
        let db = Database::new(&setup.postgres_url).await?;
        let emoji = EmojiStore::new(ctx.clone(), setup.admin_guild, emoji).await?;
        let links = Links::new().await?;

        let reddit_client = RedditClient::new(api.reddit).await?;
        let twitch_client = TwitchClient::new(api.twitch).await?;

        let sticky = StickyLock::new(&db).await?;

        Ok(Self {
            emoji,
            admin_guild: setup.admin_guild,
            embed_color: setup.embed_color,
            db,
            links,
            reddit_client,
            twitch_client,
            sticky,
            anti_spam_sender,
            ctx,
        })
    }
}
