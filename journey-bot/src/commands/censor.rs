use std::{collections::HashSet, sync::Arc, time::Duration};

use poise::{
    CreateReply,
    serenity_prelude::{Channel, GuildId, Mentionable, Message},
};
use regex::Regex;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};
use tracing::info;

use crate::{
    Context, Error,
    db::{get_config, get_config_from_id},
    emoji::Emoji,
    store::Store,
    utils::{
        BotError, censor_log, eph, fetch_sheet, fetch_sheet_columns, guild_log, schedule_with_sleep,
    },
    views::embed::default_embed,
};

pub struct CensorScheduler;

impl CensorScheduler {
    pub async fn schedule_all(store: Arc<Store>) {
        schedule_with_sleep(
            store,
            Duration::from_secs(60 * 60 * 2),
            Self::update_auto_censor_lists,
        );
    }

    async fn update_auto_censor_lists(store: Arc<Store>) {
        let Ok(configs) = sea_entity::censor_config::Entity::find()
            .all(&store.db.sea)
            .await
        else {
            return;
        };

        for config in configs {
            if let Some(sheet_id) = &config.auto_censor_list_sheet_id {
                info!("Updating auto censor list for guild {}", config.id);
                let Ok(sheet) = fetch_sheet(sheet_id).await else {
                    continue;
                };
                let columns = config
                    .auto_censor_list_column_names
                    .iter()
                    .collect::<Vec<_>>();

                let Ok(mut columns) = fetch_sheet_columns(sheet, &columns).await else {
                    continue;
                };

                let mut censor_list = Vec::new();

                for column in &config.auto_censor_list_column_names {
                    censor_list.append(&mut columns.remove(column).unwrap());
                }

                let new = censor_list
                    .into_iter()
                    .map(|c| c.to_lowercase())
                    .collect::<HashSet<_>>();
                let old = config
                    .auto_censor_list
                    .clone()
                    .into_iter()
                    .collect::<HashSet<_>>();

                let added = new.difference(&old).cloned().collect::<Vec<_>>();
                let removed = old.difference(&new).cloned().collect::<Vec<_>>();

                let config_id = config.id;
                let guild_id = GuildId::new(config.id as u64);

                let mut config = config.into_active_model();
                config.auto_censor_list = Set(new.into_iter().collect::<Vec<_>>());
                let _ = config.update(&store.db.sea).await;

                info!(
                    "Added {} new char sequences to auto-censor-list for guild {}, removed {} char sequences",
                    added.len(),
                    config_id,
                    removed.len()
                );

                for added in added {
                    guild_log(
                        store.clone(),
                        guild_id,
                        Emoji::Twitch,
                        format!(
                            "Char sequence `{}` was added to the auto-censor-list",
                            added
                        ),
                        None,
                    )
                    .await;
                }

                for removed in removed {
                    guild_log(
                        store.clone(),
                        guild_id,
                        Emoji::Twitch,
                        format!(
                            "Char sequence `{}` was removed from the auto-censor-list",
                            removed,
                        ),
                        None,
                    )
                    .await;
                }
            }
        }
    }
}

#[poise::command(
    slash_command,
    subcommands("show", "log_channel", "add", "remove", "sheet_set", "sheet_remove"),
    guild_only,
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_MESSAGES | SEND_MESSAGES | VIEW_CHANNEL",
    rename = "censor-config"
)]
pub async fn censor(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show the censor config.
#[poise::command(slash_command)]
async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let censor_config = get_config::<sea_entity::censor_config::Entity>(ctx).await?;

    let censor_list = if censor_config.censor_list.is_empty() {
        "None".to_string()
    } else {
        censor_config
            .censor_list
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let auto_censor_list = if censor_config.auto_censor_list.is_empty() {
        "None".to_string()
    } else {
        censor_config
            .auto_censor_list
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let embed = default_embed(ctx).title("Censor config").fields(
        [
            ("Censor List", censor_list),
            ("Auto Censor List", auto_censor_list),
        ]
        .into_iter()
        .map(|(n, v)| (n, v, true)),
    );

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Configure the log channel for censor logs.
#[poise::command(slash_command, rename = "log-channel")]
async fn log_channel(
    ctx: Context<'_>,
    #[description = "The channel to set as the censor-log channel."] channel: Channel,
) -> Result<(), Error> {
    let guild = ctx
        .partial_guild()
        .await
        .ok_or(BotError::new("Expected a guild"))?;

    let bot_member = guild.member(ctx, &ctx.framework().bot_id).await?;
    let channel = channel
        .guild()
        .ok_or(BotError::new("Expected a guild channel"))?;

    let permissions = ctx
        .guild()
        .unwrap()
        .user_permissions_in(&channel, &bot_member);

    if !permissions.view_channel() {
        eph(ctx, "I don't have permission to view that channel.").await?;
    }

    if !permissions.send_messages() {
        eph(
            ctx,
            "I don't have permission to send messages in that channel.",
        )
        .await?;
    }

    if !permissions.attach_files() {
        eph(
            ctx,
            "I don't have permission to attach files in that channel.",
        )
        .await?;
    }

    let mut censor_config = get_config::<sea_entity::censor_config::Entity>(ctx)
        .await?
        .into_active_model();
    censor_config.log_channel = Set(Some(channel.id.into()));
    censor_config.update(&ctx.data().db.sea).await?;

    ctx.say(format!("Censor-log channel set to {}", channel.mention()))
        .await?;

    Ok(())
}

/// Add a char sequence to the censor list.
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The char-sequence to add to the censor list"]
    #[rename = "char-sequence"]
    char_sequence: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let censor_config = get_config::<sea_entity::censor_config::Entity>(ctx).await?;

    let char_sequence = char_sequence.to_lowercase();

    if censor_config.censor_list.contains(&char_sequence) {
        eph(ctx, "This char-sequence is already in the censor list.").await?;
    }

    let mut censor_list = censor_config.censor_list.clone();
    censor_list.push(char_sequence.clone());

    let mut censor_config = censor_config.into_active_model();
    censor_config.censor_list = Set(censor_list);
    censor_config.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The char sequence `{}` was added to the censor list by {} (`{}`)",
            char_sequence,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    ctx.say(format!("`{}` added to the censor list.", char_sequence))
        .await?;

    Ok(())
}

/// Remove a char sequence from the censor list.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The char-sequence to remove from the censor list"]
    #[rename = "char-sequence"]
    char_sequence: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let censor_config = get_config::<sea_entity::censor_config::Entity>(ctx).await?;

    let char_sequence = char_sequence.to_lowercase();

    if !censor_config.censor_list.contains(&char_sequence) {
        eph(ctx, "This char-sequence is not in the censor list.").await?;
    }

    let mut censor_list = censor_config.censor_list.clone();
    censor_list.retain(|c| c != &char_sequence);

    let mut censor_config = censor_config.into_active_model();
    censor_config.censor_list = Set(censor_list);
    censor_config.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The char sequence `{}` was removed from the censor list by {} (`{}`)",
            char_sequence,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    ctx.say(format!("`{}` removed from the censor list.", char_sequence))
        .await?;
    Ok(())
}

/// Set an auto censor sheet.
#[poise::command(slash_command, rename = "sheet-set")]
async fn sheet_set(
    ctx: Context<'_>,
    #[description = "The ID of the google sheet to update the auto-censor-list from."]
    #[rename = "censor-list-sheet-id"]
    censor_list_sheet_id: String,
    #[description = "The names of the sheet columns to update the censor-list from, comma separated."]
    #[rename = "censor-list-column_names"]
    censor_list_sheet_column_names: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let censor_config = get_config::<sea_entity::censor_config::Entity>(ctx).await?;
    let re = Regex::new("(, )+").unwrap();

    let column_names = re
        .split(&censor_list_sheet_column_names)
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let mut sheet = fetch_sheet(&censor_list_sheet_id).await?;

    let headers = sheet
        .headers()
        .map_err(|_| BotError::new("Failed to deserialize sheet headers"))?
        .into_iter()
        .collect::<Vec<_>>();

    for column in column_names.iter() {
        if !headers.contains(&column.as_str()) {
            eph(ctx, format!("Column `{}` not found in sheet.", column)).await?;
            return Ok(());
        }
    }

    let guild_log_msg = format!(
        "An auto-censor-list sheet `{}` was added by {} (`{}`)",
        censor_list_sheet_id,
        ctx.author().name,
        ctx.author().id,
    );

    let mut censor_config = censor_config.into_active_model();
    censor_config.auto_censor_list_sheet_id = Set(Some(censor_list_sheet_id));
    censor_config.auto_censor_list_column_names = Set(column_names);
    censor_config.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        guild_log_msg,
        None,
    )
    .await;

    ctx.say("Censor-list sheet added").await?;

    CensorScheduler::update_auto_censor_lists(ctx.data().clone()).await;

    Ok(())
}

/// Remove the auto censor list sheet.
#[poise::command(slash_command, rename = "sheet-remove")]
async fn sheet_remove(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let censor_config = get_config::<sea_entity::censor_config::Entity>(ctx).await?;

    if censor_config.auto_censor_list_sheet_id.is_none() {
        eph(ctx, "This observer does not have a blacklist sheet set.").await?;
    };

    let mut censor_config = censor_config.into_active_model();
    censor_config.auto_censor_list = Set(vec![]);
    censor_config.auto_censor_list_sheet_id = Set(None);
    censor_config.auto_censor_list_column_names = Set(vec![]);
    censor_config.update(&ctx.data().db.sea).await?;

    ctx.say("Censor-list sheet removed").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The auto censor list sheet was removed by {} (`{}`)",
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

pub async fn on_message(store: Arc<Store>, message: &Message) -> Result<(), Error> {
    if message.author.bot {
        return Ok(());
    }
    let Some(guild_id) = message.guild_id else {
        return Ok(());
    };

    let censor_config =
        get_config_from_id::<sea_entity::censor_config::Entity>(store.clone(), guild_id).await?;

    let content = message.content.to_lowercase();

    for censor in censor_config
        .censor_list
        .iter()
        .chain(censor_config.auto_censor_list.iter())
    {
        if content.contains(censor) {
            censor_log(
                store.clone(),
                guild_id,
                Emoji::Warning,
                format!(
                    "Censored message by {} (`{}`) in {}, char sequence `{}` is not allowed.\n```\n{}\n```",
                    message.author.name,
                    message.author.id,
                    message.channel_id.mention(),
                    censor,
                    content
                ),
                None,
            )
            .await;
            let _ = message.delete(&store.ctx).await;
            break;
        }
    }

    Ok(())
}
