use std::{collections::HashSet, sync::Arc, time::SystemTime};

use chrono::{DateTime, Duration, Utc};
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, ChannelId, CreateAttachment, Member, Mentionable, Role, RoleId,
    },
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, IntoActiveModel};

use crate::{
    Context, Error,
    db::{get_config, get_config_from_id},
    emoji::Emoji,
    store::Store,
    utils::{BotError, eph, guild_log},
    views::embed::default_embed,
};

#[poise::command(
    slash_command,
    subcommands(
        "show",
        "configure_ml_channel",
        "configure_new_threshold",
        "configure_time_zone",
        "trusted_roles"
    ),
    guild_only,
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "ATTACH_FILES | SEND_MESSAGES | VIEW_CHANNEL",
    rename = "guild-config"
)]
pub async fn guild_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show the config for this guild
#[poise::command(slash_command)]
async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_config = get_config::<sea_entity::guild_config::Entity>(ctx).await?;

    let trusted_roles = if guild_config.trusted_roles.is_empty() {
        "None".to_string()
    } else {
        guild_config
            .trusted_roles
            .into_iter()
            .map(|v| RoleId::new(v as u64).mention().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };

    let ml_channel = if let Some(ml_channel) = guild_config.guild_log {
        ChannelId::new(ml_channel as u64).mention().to_string()
    } else {
        "Not configured".to_string()
    };

    let embed = default_embed(ctx).title("Guild config").fields(
        [
            ("Mod-Log-Channel", ml_channel),
            ("Trusted Roles", trusted_roles),
            ("New Threshold", guild_config.new_user_threshold.to_string()),
            ("Timezone", guild_config.time_zone),
        ]
        .into_iter()
        .map(|(n, v)| (n, v, true)),
    );

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Set the mod-log channel.
#[poise::command(slash_command, rename = "mod-log-channel")]
async fn configure_ml_channel(
    ctx: Context<'_>,
    #[description = "The channel to set as the mod-log channel."] channel: serenity::Channel,
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

    let mut guild_config = get_config::<sea_entity::guild_config::Entity>(ctx)
        .await?
        .into_active_model();
    guild_config.guild_log = Set(Some(channel.id.into()));
    guild_config.update(&ctx.data().db.sea).await?;

    ctx.say(format!("Mod-Log channel set to {}", channel.mention()))
        .await?;

    Ok(())
}

/// Set the threshold for new users.
#[poise::command(slash_command, rename = "new-threshold")]
async fn configure_new_threshold(
    ctx: Context<'_>,
    #[description = "The new user threshold (in days)"]
    #[min = 1]
    #[max = 255]
    threshold: u8,
) -> Result<(), Error> {
    let mut guild_config = get_config::<sea_entity::guild_config::Entity>(ctx)
        .await?
        .into_active_model();
    guild_config.new_user_threshold = Set(threshold.into());
    guild_config.update(&ctx.data().db.sea).await?;

    ctx.say(format!("New user threshold set to {threshold} days."))
        .await?;

    Ok(())
}

/// Set the time zone for logs.
#[poise::command(slash_command, rename = "time-zone")]
async fn configure_time_zone(
    ctx: Context<'_>,
    #[description = "The time zone to use for logs."]
    #[rename = "time-zone"]
    time_zone: String,
) -> Result<(), Error> {
    let available_timezones = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|tz| tz.name().to_string())
        .collect::<HashSet<_>>();

    if !available_timezones.contains(&time_zone) {
        let mut all = available_timezones.into_iter().collect::<Vec<_>>();
        all.sort();
        let out = all.join("\n");

        ctx.send(
            CreateReply::default()
                .content("I don't know this time zone. See the attached file for all valid values.")
                .attachment(CreateAttachment::bytes(out, "time-zones.txt")),
        )
        .await?;

        return Ok(());
    }

    let mut guild_config = get_config::<sea_entity::guild_config::Entity>(ctx)
        .await?
        .into_active_model();
    guild_config.time_zone = Set(time_zone.clone());
    guild_config.update(&ctx.data().db.sea).await?;
    ctx.say(format!("Time zone set to {time_zone}")).await?;

    Ok(())
}

#[poise::command(
    slash_command,
    subcommands("tr_add", "tr_remove"),
    rename = "trusted-roles"
)]
async fn trusted_roles(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a trusted role.
#[poise::command(slash_command, rename = "add")]
async fn tr_add(ctx: Context<'_>, role: Role) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(ctx.data().clone(), guild_id)
            .await?;

    let role_id = role.id.get() as i64;

    if guild_config.trusted_roles.contains(&role_id) {
        eph(ctx, "That role is already trusted.").await?;
        return Ok(());
    }

    let mut trusted = guild_config.trusted_roles.clone();
    trusted.push(role_id);

    let mut guild_config = guild_config.into_active_model();
    guild_config.trusted_roles = Set(trusted);
    guild_config.update(&ctx.data().db.sea).await?;

    ctx.say("Trusted role added.").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The role `{}` (`{}`) was added as a trusted role by {} (`{}`).",
            role.name,
            role.id,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

// Remove a trusted role.
#[poise::command(slash_command, rename = "remove")]
async fn tr_remove(ctx: Context<'_>, role: Role) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(ctx.data().clone(), guild_id)
            .await?;

    let role_id = role.id.get() as i64;

    if !guild_config.trusted_roles.contains(&role_id) {
        eph(ctx, "That role is not trusted.").await?;
        return Ok(());
    }

    let trusted = guild_config
        .trusted_roles
        .clone()
        .into_iter()
        .filter(|v| *v != role_id)
        .collect::<Vec<_>>();

    let mut guild_config = guild_config.into_active_model();
    guild_config.trusted_roles = Set(trusted);
    guild_config.update(&ctx.data().db.sea).await?;

    ctx.say("Trusted role removed.").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The role `{}` (`{}`) was removed as a trusted role by {} (`{}`).",
            role.name,
            role.id,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

pub async fn on_member_join(store: Arc<Store>, new_member: &Member) -> Result<(), Error> {
    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), new_member.guild_id)
            .await?;

    let now: DateTime<Utc> = SystemTime::now().into();
    let created_at = new_member.user.created_at().to_utc();
    let diff = now - created_at;
    let threshold = Duration::days(guild_config.new_user_threshold.into());
    let age = if diff.num_days() > 0 {
        format!("{} days", diff.num_days())
    } else {
        format!(
            "{} hours, {} minutes",
            diff.num_hours(),
            diff.num_minutes() % 60
        )
    };
    let is_new = threshold > diff;
    let msg = format!(
        "{} (`{}`) has joined the server, account created {age} ago. {}",
        new_member.mention(),
        new_member.user.id,
        if is_new { ":new:" } else { "" }
    );

    guild_log(store, new_member.guild_id, Emoji::Join, msg, None).await;

    Ok(())
}
