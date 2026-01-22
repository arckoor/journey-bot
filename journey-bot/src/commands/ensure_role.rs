use std::sync::Arc;

use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, CreateAttachment, GuildMemberUpdateEvent, Member, Mentionable,
    },
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};

use crate::{
    Context, Error,
    db::{get_config, get_config_from_id},
    store::Store,
    utils::{add_roles_to_member, eph, member_is_valid_target},
    views::embed::default_embed,
};

#[poise::command(
    slash_command,
    subcommands("list", "add", "remove", "sweep"),
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
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
async fn sweep(
    ctx: Context<'_>,
    #[description = "Whether to do a dry-sweep, that is list affected members but not add any roles to them."]
    dry: Option<bool>,
) -> Result<(), Error> {
    let guild = ctx
        .partial_guild()
        .await
        .ok_or("Expected to be in a guild")?;
    let guild_id = guild.id.get();
    let ensured_roles = sea_entity::ensured_role::Entity::find()
        .filter(sea_entity::ensured_role::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?
        .into_iter()
        .map(|m| m.role_id as u64)
        .collect::<Vec<_>>();

    if ensured_roles.is_empty() {
        eph(ctx, "No roles ensured.").await?;
        return Ok(());
    }

    let dry = dry.unwrap_or(false);
    let guild_config = get_config::<sea_entity::guild_config::Entity>(ctx).await?;
    ctx.defer().await?;
    let guild_roles = &guild.roles;

    let mut member_cnt = 0;
    let mut role_cnt = 0;

    let mut more = true;
    let mut last = None;

    let mut would_add = Vec::new();

    while more {
        more = false;
        for member in guild.members(ctx, None, last).await? {
            more = true;
            last = Some(member.user.id);
            member_cnt += 1;
            if !member_is_valid_target(&member, &guild_config) {
                continue;
            }
            let added_roles =
                add_roles_to_member(ctx, &ensured_roles, &member, guild_roles, guild_id, dry)
                    .await?;
            if !added_roles.is_empty() {
                role_cnt += added_roles.len();
                if dry {
                    for (id, name) in added_roles {
                        would_add.push(format!(
                            "{} ({}) -> {} ({})",
                            member.user.name, member.user.id, name, id
                        ));
                    }
                }
            }
        }
    }

    if !dry {
        ctx.say(format!(
            "I looked at {member_cnt} members and added {role_cnt} roles."
        ))
        .await?;
    } else {
        let mut reply = CreateReply::default().content(format!(
            "I looked at {member_cnt} members and would add {role_cnt} roles."
        ));
        if !would_add.is_empty() {
            reply = reply.attachment(CreateAttachment::bytes(would_add.join("\n"), "roles.txt"));
        }

        ctx.send(reply).await?;
    }

    Ok(())
}

pub async fn on_member_update(
    store: Arc<Store>,
    ctx: &serenity::Context,
    new: &Option<Member>,
    event: &GuildMemberUpdateEvent,
) -> Result<(), Error> {
    let Some(member) = new else { return Ok(()) };

    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), member.guild_id)
            .await?;

    if !member_is_valid_target(member, &guild_config) {
        return Ok(());
    }

    let ensured_roles = sea_entity::ensured_role::Entity::find()
        .filter(sea_entity::ensured_role::Column::GuildId.eq(event.guild_id.get()))
        .all(&store.db.sea)
        .await?
        .into_iter()
        .map(|m| m.role_id as u64)
        .collect::<Vec<_>>();

    if ensured_roles.is_empty() {
        return Ok(());
    }

    let guild_roles = event.guild_id.roles(&ctx).await?;
    add_roles_to_member(
        ctx,
        &ensured_roles,
        member,
        &guild_roles,
        event.guild_id.get(),
        false,
    )
    .await?;

    Ok(())
}
