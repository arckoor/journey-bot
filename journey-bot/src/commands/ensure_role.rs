use std::{collections::HashMap, sync::Arc};

use chrono::NaiveDateTime;
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, GuildMemberFlags, GuildMemberUpdateEvent, Http, Member, Mentionable,
        Role, RoleId,
    },
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};
use tracing::info;

use crate::{
    Context, Error,
    db::{get_config, get_config_from_id},
    store::Store,
    utils::eph,
    views::embed::default_embed,
};

#[poise::command(
    slash_command,
    subcommands("list", "add", "remove", "sweep", "set_onboarding_time"),
    guild_only,
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_ROLES",
    rename = "ensure-role"
)]
pub async fn ensure_role(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// List all ensured roles.
#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();
    let ensured_roles = sea_entity::ensured_role::Entity::find()
        .filter(sea_entity::ensured_role::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?
        .into_iter()
        .map(|role| role.role_id as u64)
        .collect::<Vec<_>>();

    if ensured_roles.is_empty() {
        eph(ctx, "No rules ensured.").await?;
        return Ok(());
    }

    let roles = ctx
        .partial_guild()
        .await
        .ok_or("Expected to be in a guild")?
        .roles
        .into_iter()
        .filter(|role| ensured_roles.contains(&role.0.get()))
        .map(|role| role.1.mention().to_string())
        .collect::<Vec<_>>();

    let embed = default_embed(ctx).title("Ensured Roles").field(
        "All ensured roles in the server",
        roles.join("\n"),
        false,
    );

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Add a role to ensure.
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The role to ensure is always present."] role: serenity::Role,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let ensured = sea_entity::ensured_role::Entity::find_by_id((guild_id.into(), role.id.into()))
        .one(&ctx.data().db.sea)
        .await?;

    if ensured.is_some() {
        eph(ctx, "Role already ensured.").await?;
        return Ok(());
    }

    sea_entity::ensured_role::ActiveModel {
        guild_id: Set(guild_id.into()),
        role_id: Set(role.id.into()),
    }
    .insert(&ctx.data().db.sea)
    .await?;

    ctx.say(format!("Role `{}` ensured.", role.name)).await?;

    Ok(())
}

/// Remove an ensured role.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The role to remove."] role: serenity::Role,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let ensured = sea_entity::ensured_role::Entity::find_by_id((guild_id.into(), role.id.into()))
        .one(&ctx.data().db.sea)
        .await?;

    let Some(ensured) = ensured else {
        eph(ctx, "Role not ensured.").await?;
        return Ok(());
    };

    ensured
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    ctx.say(format!("Role {} no longer ensured.", role.name))
        .await?;

    Ok(())
}

/// Sweep all members for ensured roles.
#[poise::command(slash_command)]
async fn sweep(ctx: Context<'_>) -> Result<(), Error> {
    let guild = ctx
        .partial_guild()
        .await
        .ok_or("Expected to be in a guild")?;
    let ensured_roles = sea_entity::ensured_role::Entity::find()
        .filter(sea_entity::ensured_role::Column::GuildId.eq(guild.id.get()))
        .all(&ctx.data().db.sea)
        .await?;

    if ensured_roles.is_empty() {
        eph(ctx, "No roles ensured.").await?;
        return Ok(());
    }

    let guild_config = get_config::<sea_entity::guild_config::Entity>(ctx).await?;
    ctx.defer().await?;
    let guild_roles = &guild.roles;

    let mut member_cnt = 0;
    let mut role_cnt = 0;

    let mut more = true;
    let mut last = None;

    while more {
        more = false;
        for member in guild.members(ctx, None, last).await? {
            more = true;
            last = Some(member.user.id);
            member_cnt += 1;
            if !member_is_valid_target(&member, &guild_config) {
                continue;
            }
            for ensured_role in ensured_roles.iter() {
                if add_role_to_member(ctx, ensured_role, &member, guild_roles).await? {
                    role_cnt += 1;
                }
            }
        }
    }

    ctx.say(format!(
        "I looked at {member_cnt} members and added {role_cnt} roles."
    ))
    .await?;

    Ok(())
}

/// Set the time onboarding was enabled.
#[poise::command(slash_command, rename = "set-onboarding-time")]
async fn set_onboarding_time(
    ctx: Context<'_>,
    #[description = "The time onboarding was enabled."] time: String,
) -> Result<(), Error> {
    let mut guild_config = get_config::<sea_entity::guild_config::Entity>(ctx)
        .await?
        .into_active_model();

    let str_time = NaiveDateTime::parse_from_str(&time, "%d-%m-%Y %H:%M:%S");
    let Ok(str_time) = str_time else {
        eph(ctx, "Invalid time format. Please use DD-MM-YYYY HH:MM:SS. Timestamp is expected to be a UNIX time.").await?;
        return Ok(());
    };

    let time = str_time.and_utc().timestamp() as f64;
    guild_config.onboarding_active_since = Set(time);
    guild_config.update(&ctx.data().db.sea).await?;

    ctx.say(format!("Onboarding time set to {str_time}."))
        .await?;

    Ok(())
}

pub async fn on_member_update(
    store: Arc<Store>,
    ctx: &serenity::Context,
    new: &Option<Member>,
    event: &GuildMemberUpdateEvent,
) -> Result<(), Error> {
    let ensured_roles = sea_entity::ensured_role::Entity::find()
        .filter(sea_entity::ensured_role::Column::GuildId.eq(event.guild_id.get()))
        .all(&store.db.sea)
        .await?;

    if ensured_roles.is_empty() {
        return Ok(());
    }

    let Some(member) = new else { return Ok(()) };

    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(store, member.guild_id).await?;

    if !member_is_valid_target(member, &guild_config) {
        return Ok(());
    }

    let guild_roles = event.guild_id.roles(&ctx).await?;
    for ensured_role in ensured_roles {
        add_role_to_member(ctx, &ensured_role, member, &guild_roles).await?;
    }

    Ok(())
}

fn member_is_valid_target(member: &Member, guild_config: &sea_entity::guild_config::Model) -> bool {
    if member.user.bot {
        return false;
    }

    let Some(ts) = member.joined_at else {
        return false;
    };

    // this just assumes onboarding is enabled
    // serenity doesn't expose that endpoint for whatever reason, and I can't be bothered to fork it
    if !(member
        .flags
        .intersection(GuildMemberFlags::COMPLETED_ONBOARDING)
        .bits()
        == 1
        || (ts.naive_utc().and_utc().timestamp() as f64) < guild_config.onboarding_active_since)
    {
        info!("member not valid because of flags");
        return false;
    }
    true
}

async fn add_role_to_member(
    ctx: impl AsRef<Http>,
    ensured_role: &sea_entity::ensured_role::Model,
    member: &Member,
    guild_roles: &HashMap<RoleId, Role>,
) -> Result<bool, Error> {
    if let Some(guild_role) = guild_roles.get(&(ensured_role.role_id as u64).into())
        && !member.roles.contains(&guild_role.id)
    {
        member.add_role(&ctx, guild_role.id).await?;
        info!(
            "{}",
            format!(
                "Added role {} to {} in {}.",
                ensured_role.role_id, member.user.id, ensured_role.guild_id
            )
        );
        return Ok(true);
    }
    Ok(false)
}
