use std::{sync::Arc, time::Duration};

use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, GuildId, Mentionable, Message, RoleId, UserId,
        futures::{self, Stream},
    },
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};

use crate::{
    Context, Error,
    emoji::Emoji,
    store::Store,
    utils::{
        LogError, add_roles_to_member, eph, guild_log, now, remove_roles_from_member,
        schedule_at_interval,
    },
    views::embed::default_embed,
};

pub struct BypassScheduler;

impl BypassScheduler {
    pub fn schedule(store: Arc<Store>) {
        schedule_at_interval(
            store,
            Duration::from_secs(60 * 10),
            |store: Arc<Store>| async move {
                Self::remove_old_bypasses(store).await.log();
            },
        );
    }

    async fn remove_old_bypasses(store: Arc<Store>) -> Result<(), Error> {
        for (assigned, bypass_role) in sea_entity::assigned_bypass_role::Entity::find()
            .find_also_related(sea_entity::bypass_role::Entity)
            .all(&store.db.sea)
            .await?
        {
            let bypass_role =
                bypass_role.expect("Every assigned bypass must belong to a bypass role");
            if now().as_secs_f64() - assigned.timestamp > bypass_role.timeout as f64 {
                let (name, id) = Self::remove_bypass(store.clone(), &bypass_role, assigned).await?;
                guild_log(
                    store.clone(),
                    GuildId::new(bypass_role.guild_id as u64),
                    Emoji::Info,
                    format!(
                        "Bypass role {} (`{}`) was removed from {} (`{}`) because the timeout was reached",
                        bypass_role.name, bypass_role.id, name, id,
                    ),
                    None,
                )
                .await;
            }
        }

        Ok(())
    }

    async fn remove_bypass(
        store: Arc<Store>,
        bypass_role: &sea_entity::bypass_role::Model,
        assigned: sea_entity::assigned_bypass_role::Model,
    ) -> Result<(String, u64), Error> {
        let guild = GuildId::new(bypass_role.guild_id as u64)
            .to_partial_guild(&store.ctx)
            .await?;
        let member = guild
            .member(&store.ctx, UserId::new(assigned.user_id as u64))
            .await?;

        remove_roles_from_member(
            &store.ctx,
            &[bypass_role.role_id as u64],
            &member,
            &guild.roles,
            guild.id.get(),
            false,
        )
        .await?;

        assigned.into_active_model().delete(&store.db.sea).await?;

        Ok((member.user.name, member.user.id.get()))
    }
}

#[poise::command(
    slash_command,
    subcommands("list", "add", "remove"),
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_ROLES",
    rename = "bypass-config"
)]
pub async fn bypass_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let bypass_roles = sea_entity::bypass_role::Entity::find()
        .filter(sea_entity::bypass_role::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?;

    if bypass_roles.is_empty() {
        eph(ctx, "No bypass roles configured.").await?;
        return Ok(());
    }

    let mut embed = default_embed(ctx).title("Bypass roles");

    for bypass_role in bypass_roles {
        embed = embed.field(
            format!("{} | ID: {}", bypass_role.name, bypass_role.id),
            format!(
                "{} | Timeout: {}s",
                RoleId::new(bypass_role.role_id as u64).mention(),
                bypass_role.timeout
            ),
            false,
        );
    }

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The role to configure a bypass for."] role: serenity::Role,
    #[description = "The channel to configure a bypass for."] channel: serenity::Channel,
    #[description = "The time until the bypass is automatically removed (in seconds)."]
    timeout: u32,
    #[description = "Name of the bypass (used for autocomplete)."] name: String,
) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();
    let role_id = role.id.get();
    let channel_id = channel.id().get();

    if let Some(bypass_role) = sea_entity::bypass_role::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(sea_entity::bypass_role::Column::GuildId.eq(guild_id))
                .add(
                    sea_orm::Condition::any()
                        .add(sea_entity::bypass_role::Column::RoleId.eq(role_id))
                        .add(sea_entity::bypass_role::Column::ChannelId.eq(channel_id))
                        .add(sea_entity::bypass_role::Column::Name.eq(&name)),
                ),
        )
        .one(&ctx.data().db.sea)
        .await?
    {
        let msg = if bypass_role.role_id == role_id as i64 {
            format!(
                "Bypass for role {} (`{}`), channel {} (`{}`) already configured!",
                role.name,
                role_id,
                channel.guild().ok_or("Expected to be in a guild")?.name(),
                channel_id,
            )
        } else {
            format!(
                "Bypass with name {} already configured for role `{}`",
                name, bypass_role.role_id
            )
        };

        eph(ctx, msg).await?;
        return Ok(());
    }

    if timeout < 10 {
        eph(ctx, "Timeout is very low, this seems like a mistake.").await?;
        return Ok(());
    }

    sea_entity::bypass_role::ActiveModel {
        id: Set(cuid2::slug()),
        guild_id: Set(guild_id as i64),
        role_id: Set(role_id as i64),
        channel_id: Set(channel_id as i64),
        timeout: Set(timeout as i32),
        name: Set(name),
    }
    .insert(&ctx.data().db.sea)
    .await?;

    guild_log(
        ctx.data().clone(),
        GuildId::new(guild_id),
        Emoji::Info,
        format!(
            "A bypass was configured for role {} (`{}`), channel {} (`{}`) by {} (`{}`)",
            role.name,
            role_id,
            channel.mention(),
            channel_id,
            ctx.author().name,
            ctx.author().id.get()
        ),
        None,
    )
    .await;

    ctx.say("Bypass configured.").await?;

    Ok(())
}

async fn autocomplete_id<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let guild_id = ctx.guild_id().unwrap_or(GuildId::new(1));
    let bypass_roles = sea_entity::bypass_role::Entity::find()
        .filter(sea_entity::bypass_role::Column::GuildId.eq(guild_id.get() as i64))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        bypass_roles
            .into_iter()
            .filter(move |m| m.id.starts_with(partial))
            .map(|m| m.id),
    )
}

#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The ID of the bypass to remove."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.get();

    let bypass_role = sea_entity::bypass_role::Entity::find_by_id(&id)
        .filter(sea_entity::bypass_role::Column::GuildId.eq(guild_id))
        .one(&ctx.data().db.sea)
        .await?;

    let Some(bypass_role) = bypass_role else {
        eph(ctx, "Bypass role not found.").await?;
        return Ok(());
    };

    for assigned in sea_entity::assigned_bypass_role::Entity::find()
        .filter(sea_entity::assigned_bypass_role::Column::BypassRoleId.eq(&bypass_role.id))
        .all(&ctx.data().db.sea)
        .await?
    {
        BypassScheduler::remove_bypass(ctx.data().clone(), &bypass_role, assigned).await?;
    }

    let role_id = bypass_role.role_id;
    bypass_role
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    guild_log(
        ctx.data().clone(),
        GuildId::new(guild_id),
        Emoji::Info,
        format!(
            "The bypass configured for role `{}` was removed by {} (`{}`)",
            role_id,
            ctx.author().name,
            ctx.author().id.get()
        ),
        None,
    )
    .await;

    ctx.say("Bypass role removed.").await?;

    Ok(())
}

async fn autocomplete_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let guild_id = ctx.guild_id().unwrap_or(GuildId::new(1));
    let bypass_roles = sea_entity::bypass_role::Entity::find()
        .filter(sea_entity::bypass_role::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        bypass_roles
            .into_iter()
            .filter(move |m| m.name.starts_with(partial))
            .map(|m| m.name),
    )
}

#[poise::command(
    slash_command,
    subcommands(),
    guild_only,
    default_member_permissions = "BAN_MEMBERS",
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn bypass(
    ctx: Context<'_>,
    #[description = "The name of the bypass to apply."]
    #[autocomplete = "autocomplete_name"]
    name: String,
    #[description = "The member to apply the bypass to."] member: serenity::Member,
) -> Result<(), Error> {
    let guild = ctx
        .partial_guild()
        .await
        .ok_or("Expected to be in a guild")?;
    let guild_id = guild.id.get();
    let user_id = member.user.id.get();

    let Some(bypass_role) = sea_entity::bypass_role::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(sea_entity::bypass_role::Column::GuildId.eq(guild_id))
                .add(sea_entity::bypass_role::Column::Name.eq(name)),
        )
        .one(&ctx.data().db.sea)
        .await?
    else {
        return Ok(());
    };

    if let Some(assigned) = sea_entity::assigned_bypass_role::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(sea_entity::assigned_bypass_role::Column::BypassRoleId.eq(&bypass_role.id))
                .add(sea_entity::assigned_bypass_role::Column::UserId.eq(user_id as i64)),
        )
        .one(&ctx.data().db.sea)
        .await?
    {
        assigned
            .into_active_model()
            .delete(&ctx.data().db.sea)
            .await?;
    }

    add_roles_to_member(
        ctx,
        &[bypass_role.role_id as u64],
        &member,
        &guild.roles,
        guild_id,
        false,
    )
    .await?;

    sea_entity::assigned_bypass_role::ActiveModel {
        user_id: Set(user_id as i64),
        timestamp: Set(now().as_secs_f64()),
        bypass_role_id: Set(bypass_role.id),
        ..Default::default()
    }
    .insert(&ctx.data().db.sea)
    .await?;

    guild_log(
        ctx.data().clone(),
        guild.id,
        Emoji::Info,
        format!(
            "Bypass role {} (`{}`) was applied to {} (`{}`) by {} (`{}`)",
            bypass_role.name,
            bypass_role.role_id,
            member.user.name,
            user_id,
            ctx.author().name,
            ctx.author().id.get()
        ),
        None,
    )
    .await;

    ctx.say(format!(
        "Bypass applied, will expire in {} seconds.",
        bypass_role.timeout
    ))
    .await?;

    Ok(())
}

pub async fn on_message(store: Arc<Store>, message: &Message) -> Result<(), Error> {
    let Some(guild_id) = message.guild_id else {
        return Ok(());
    };
    let member = message.member(&store.ctx).await?;
    let guild = guild_id.to_partial_guild(&store.ctx).await?;

    for (assigned, bypass_role) in sea_entity::assigned_bypass_role::Entity::find()
        .find_also_related(sea_entity::bypass_role::Entity)
        .filter(
            sea_orm::Condition::all()
                .add(sea_entity::bypass_role::Column::GuildId.eq(guild_id.get() as i64))
                .add(sea_entity::bypass_role::Column::ChannelId.eq(message.channel_id.get() as i64))
                .add(
                    sea_entity::assigned_bypass_role::Column::UserId
                        .eq(message.author.id.get() as i64),
                ),
        )
        .all(&store.db.sea)
        .await?
    {
        let bypass_role = bypass_role.expect("Every assigned bypass must belong to a bypass role");

        remove_roles_from_member(
            &store.ctx,
            &[bypass_role.role_id as u64],
            &member,
            &guild.roles,
            guild_id.get(),
            false,
        )
        .await?;

        assigned.into_active_model().delete(&store.db.sea).await?;
    }

    Ok(())
}
