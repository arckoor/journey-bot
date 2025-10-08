use std::{collections::HashMap, sync::Arc};

use poise::{
    CreateReply,
    serenity_prelude::{ChannelId, GuildId, Mentionable, Message, MessageId, futures},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};
use tracing::warn;

use crate::{
    Context, Error,
    db::Database,
    emoji::Emoji,
    store::Store,
    utils::{BotError, LogError, eph, guild_log, send_message},
    views::embed::default_embed,
};

pub struct StickyLock {
    locks: RwLock<HashMap<String, Mutex<()>>>,
}

impl StickyLock {
    pub async fn new(db: &Database) -> Result<Self, BotError> {
        let mut locks = HashMap::new();

        for model in sea_entity::sticky_message::Entity::find()
            .all(&db.sea)
            .await?
        {
            locks.insert(model.id, Mutex::new(()));
        }

        Ok(Self {
            locks: RwLock::new(locks),
        })
    }

    pub async fn add(&self, id: String) {
        let mut guard = self.locks.write().await;
        guard.entry(id).or_insert_with(|| Mutex::new(()));
    }

    pub async fn remove(&self, id: &str) {
        let mut guard = self.locks.write().await;
        if guard.contains_key(id) {
            guard.remove(id);
        }
    }

    pub async fn get(&'_ self) -> RwLockReadGuard<'_, HashMap<String, Mutex<()>>> {
        self.locks.read().await
    }
}

#[poise::command(
    slash_command,
    subcommands("list", "set", "remove"),
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn stick(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// List all stickies in the server.
#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let stickies = sea_entity::sticky_message::Entity::find()
        .filter(sea_entity::sticky_message::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?;

    if stickies.is_empty() {
        eph(ctx, "No stickies found").await?;
        return Ok(());
    }

    let mut embed = default_embed(ctx)
        .title("Stickies")
        .description("All stickies in this server.");

    for sticky in stickies {
        let channel_name = ChannelId::new(sticky.channel_id as u64)
            .name(&ctx)
            .await
            .unwrap_or("Unknown".to_string());

        embed = embed.field(
            format!("#{} | ID: {}", channel_name, sticky.id),
            format!("`{}`", sticky.content),
            false,
        );
    }

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

async fn autocomplete_id<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl futures::Stream<Item = String> + 'a {
    let guild_id = ctx.guild_id().unwrap_or(GuildId::new(1));
    let stickies = sea_entity::sticky_message::Entity::find()
        .filter(sea_entity::sticky_message::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        stickies
            .into_iter()
            .filter(move |m| m.id.starts_with(partial))
            .map(|m| m.id),
    )
}

/// Stick a message to the channel or modify the currently active one.
#[poise::command(slash_command)]
async fn set(
    ctx: Context<'_>,
    #[description = "The message to stick"] content: String,
    #[description = "Number of messages to ignore before the sticky is sent again. 0 for no limit."]
    #[rename = "message-limit"]
    message_limit: Option<u32>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let sticky = get_sticky(ctx, "0", guild_id).await;
    if sticky.is_some() {
        eph(ctx, "This channel already has a sticky message!").await?;
        return Ok(());
    }

    ctx.defer().await?;

    let content = content.replace("\\n", "\n");
    let message_limit = message_limit.unwrap_or(0) as i32;

    let sticky = sea_entity::sticky_message::ActiveModel {
        id: Set(cuid2::slug()),
        guild_id: Set(guild_id.get() as i64),
        channel_id: Set(ctx.channel_id().get() as i64),
        content: Set(content),
        messages_since: Set(0),
        current_id: Set(None),
        message_limit: Set(message_limit),
    }
    .insert(&ctx.data().db.sea)
    .await?;

    let id: String = sticky.id.clone();

    ctx.data().sticky.add(id.clone()).await;
    let _ = send_sticky(ctx.data().clone(), sticky, true).await;

    ctx.say("Sticky message added.").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Sticky,
        format!(
            "A sticky message (`{}`) was created in {} by {} (`{}`)",
            id,
            ctx.channel_id().mention(),
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

/// Unstick a message from the channel.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    #[description = "The ID of a sticky message."]
    id: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let id = id.unwrap_or("0".to_string());
    let Some(sticky) = get_sticky(ctx, &id, guild_id).await else {
        eph(ctx, "The specified sticky message was not found").await?;
        return Ok(());
    };

    let id = sticky.id.clone();
    let channel = ChannelId::new(sticky.channel_id as u64);

    sticky
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;
    ctx.data().sticky.remove(&id).await;

    ctx.say("Sticky message removed.").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Sticky,
        format!(
            "A sticky message (`{}`) in {} was removed by {} (`{}`)",
            id,
            channel.mention(),
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

async fn get_sticky(
    ctx: Context<'_>,
    id: &str,
    guild_id: GuildId,
) -> Option<sea_entity::sticky_message::Model> {
    sea_entity::sticky_message::Entity::find()
        .filter(
            sea_orm::Condition::any()
                .add(
                    sea_entity::sticky_message::Column::ChannelId.eq(ctx.channel_id().get() as i64),
                )
                .add(
                    sea_orm::Condition::all()
                        .add(sea_entity::sticky_message::Column::Id.eq(id))
                        .add(sea_entity::sticky_message::Column::GuildId.eq(guild_id.get() as i64)),
                ),
        )
        .one(&ctx.data().db.sea)
        .await
        .ok()
        .flatten()
}

async fn send_sticky(
    store: Arc<Store>,
    sticky: sea_entity::sticky_message::Model,
    now: bool,
) -> Result<(), BotError> {
    let messages_since = sticky.messages_since;
    let channel_id = ChannelId::new(sticky.channel_id as u64);

    if !now && messages_since + 1 < sticky.message_limit {
        let mut sticky = sticky.into_active_model();
        sticky.messages_since = Set(messages_since + 1);
        sticky.update(&store.db.sea).await?;
        return Ok(());
    }

    if let Some(message_id) = &sticky.current_id {
        let message_id = MessageId::new(*message_id as u64);
        let _ = store.ctx.delete_message(channel_id, message_id, None).await;
    }

    let new_message_id =
        match send_message(store.clone(), channel_id, sticky.content.clone(), None).await {
            Ok(msg) => Some(msg.id.get() as i64),
            Err(err) => {
                let guild_id = GuildId::new(sticky.guild_id as u64);
                warn!(
                    "Error while sending sticky message in channel {}: {}",
                    channel_id.get(),
                    err
                );
                let _ = guild_log(
                    store.clone(),
                    guild_id,
                    Emoji::Warning,
                    format!(
                        "Could not send a sticky message (`{}`) for {}.",
                        sticky.id,
                        channel_id.mention()
                    ),
                    None,
                )
                .await;
                None
            }
        };

    let mut sticky = sticky.into_active_model();
    sticky.messages_since = Set(0);
    sticky.current_id = Set(new_message_id);
    sticky.update(&store.db.sea).await?;

    Ok(())
}

pub async fn on_message(store: Arc<Store>, message: &Message) -> Result<(), Error> {
    if message.author.bot {
        return Ok(());
    }

    let Some(sticky_message) = sea_entity::sticky_message::Entity::find()
        .filter(sea_entity::sticky_message::Column::ChannelId.eq(message.channel_id.get() as i64))
        .one(&store.db.sea)
        .await?
    else {
        return Ok(());
    };

    let guard = store.sticky.get().await;

    let Some(mutex) = guard.get(&sticky_message.id) else {
        warn!(
            "Tried to acquire mutex for sticky message {}, but it doesn't exist!",
            sticky_message.id
        );
        return Ok(());
    };

    let Ok(lock) = mutex.try_lock() else {
        return Ok(());
    };

    send_sticky(store.clone(), sticky_message, false)
        .await
        .log("commands::sticky::on_message::send_sticky");

    drop(lock);

    Ok(())
}
