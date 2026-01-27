use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, CreateAttachment, GuildId, GuildMemberUpdateEvent, Http, Member,
        Mentionable, Role, RoleId,
        futures::{self, Stream},
    },
};
use regex::Regex;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};

use crate::{
    Context, Error,
    emoji::Emoji,
    store::Store,
    utils::{
        BotError, add_roles_to_member, eph, guild_log, member_is_valid_target,
        remove_roles_from_member,
    },
    views::embed::default_embed,
};

#[poise::command(
    slash_command,
    subcommands("list", "add", "remove", "sweep"),
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_ROLES",
    rename = "auto-role"
)]
pub async fn auto_role(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// List all configured auto-role rules.
#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();
    let auto_roles = sea_entity::auto_role::Entity::find()
        .filter(sea_entity::auto_role::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?;

    let mut embed = default_embed(ctx).title("Auto-role rules");

    for auto_role in auto_roles {
        embed = embed.field(
            format!("ID: {}", auto_role.id),
            format!(
                "{} -> {}",
                auto_role
                    .required
                    .into_iter()
                    .map(|id| RoleId::new(id as u64).mention().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                RoleId::new(auto_role.granted as u64).mention()
            ),
            false,
        );
    }

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Add an auto-role rule.
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The IDs of the roles required to trigger this auto-role, comma separated."]
    required: String,
    #[description = "The role that is granted when this auto-role is triggered"]
    granted: serenity::Role,
) -> Result<(), Error> {
    let guild = ctx
        .partial_guild()
        .await
        .ok_or("Expected to be in a guild")?;

    let re = Regex::new("(, )+").unwrap();

    let required = re
        .split(&required)
        .map(|s| {
            s.parse::<u64>()
                .map_err(|_| BotError::new("Unable to parse `required` roles"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let required_roles = required
        .iter()
        .filter_map(|id| guild.roles.get(&(*id).into()).map(|r| r.id.get() as i64))
        .collect::<Vec<_>>();

    if required_roles.len() != required.len() {
        eph(ctx, "Unable to find some required roles").await?;
        return Ok(());
    }

    sea_entity::auto_role::ActiveModel {
        id: Set(cuid2::slug()),
        guild_id: Set(guild.id.get() as i64),
        required: Set(required_roles),
        granted: Set(granted.id.get() as i64),
    }
    .insert(&ctx.data().db.sea)
    .await?;

    ctx.say("Auto-role added.").await?;

    Ok(())
}

async fn autocomplete_id<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let guild_id = ctx.guild_id().unwrap_or(GuildId::new(1));
    let auto_roles = sea_entity::auto_role::Entity::find()
        .filter(sea_entity::auto_role::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        auto_roles
            .into_iter()
            .filter(move |m| m.id.starts_with(partial))
            .map(|m| m.id),
    )
}

/// Remove an auto-role rule.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The ID of the auto-role to remove."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let auto_role = sea_entity::auto_role::Entity::find_by_id(&id)
        .filter(sea_entity::auto_role::Column::GuildId.eq(guild_id.get()))
        .one(&ctx.data().db.sea)
        .await?;

    let Some(auto_role) = auto_role else {
        eph(ctx, "Auto-role not found.").await?;
        return Ok(());
    };

    auto_role
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    ctx.say(format!("Auto-role `{}` removed.", id)).await?;

    Ok(())
}

/// Sweep all members for auto-role rules.
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
    let auto_roles = sea_entity::auto_role::Entity::find()
        .filter(sea_entity::auto_role::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?;

    if auto_roles.is_empty() {
        eph(ctx, "No auto-role rules configured").await?;
        return Ok(());
    }

    let dry = dry.unwrap_or(false);
    ctx.defer().await?;
    let guild_roles = &guild.roles;

    let mut member_cnt = 0;
    let mut role_cnt = 0;

    let mut more = true;
    let mut last = None;

    let mut would_modify = Vec::new();

    while more {
        more = false;
        for member in guild.members(ctx, None, last).await? {
            more = true;
            last = Some(member.user.id);
            member_cnt += 1;
            if !member_is_valid_target(&member) {
                continue;
            }

            let (would_add, would_remove) = add_auto_roles_to_member(
                ctx.data().clone(),
                &ctx,
                &auto_roles,
                &member,
                guild_roles,
                guild_id,
                dry,
            )
            .await?;

            role_cnt += would_add.len() + would_remove.len();
            if dry {
                for (roles, modifier) in [(would_add, "+"), (would_remove, "-")] {
                    for (id, name) in roles {
                        would_modify.push(format!(
                            "{} ({}) -> {} {} ({})",
                            member.user.name, member.user.id, modifier, name, id
                        ));
                    }
                }
            }
        }
    }

    if !dry {
        ctx.say(format!(
            "I looked at {member_cnt} members and modified {role_cnt} roles."
        ))
        .await?;
    } else {
        let mut reply = CreateReply::default().content(format!(
            "I looked at {member_cnt} members and would modify {role_cnt} roles."
        ));
        if !would_modify.is_empty() {
            reply = reply.attachment(CreateAttachment::bytes(
                would_modify.join("\n"),
                "roles.txt",
            ));
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

    if !member_is_valid_target(member) {
        return Ok(());
    }

    let guild_id = event.guild_id.get();

    let auto_roles = sea_entity::auto_role::Entity::find()
        .filter(sea_entity::auto_role::Column::GuildId.eq(guild_id))
        .all(&store.db.sea)
        .await?;

    if auto_roles.is_empty() {
        return Ok(());
    }

    add_auto_roles_to_member(
        store,
        ctx,
        &auto_roles,
        member,
        &event.guild_id.roles(&ctx).await?,
        guild_id,
        false,
    )
    .await?;
    Ok(())
}

async fn add_auto_roles_to_member(
    store: Arc<Store>,
    ctx: &impl AsRef<Http>,
    auto_roles: &[sea_entity::auto_role::Model],
    member: &Member,
    guild_roles: &HashMap<RoleId, Role>,
    guild_id: u64,
    dry: bool,
) -> Result<
    (
        Vec<(u64, std::string::String)>,
        Vec<(u64, std::string::String)>,
    ),
    Error,
> {
    let mut roles_to_add = HashSet::new();
    let mut roles_to_remove = HashSet::new();

    for auto_role in auto_roles {
        if auto_role
            .required
            .iter()
            .all(|id| member.roles.contains(&(*id as u64).into()))
        {
            roles_to_add.insert(auto_role.granted as u64);
        }
        if auto_role
            .required
            .iter()
            .any(|id| !member.roles.contains(&(*id as u64).into()))
        {
            roles_to_remove.insert(auto_role.granted as u64);
        }
    }

    if !roles_to_add.is_disjoint(&roles_to_remove) {
        guild_log(
            store.clone(),
            guild_id.into(),
            Emoji::Warning,
            "Some auto-role rules are not disjoint, refusing to process.",
            None,
        )
        .await;
        return Err(Box::new(BotError::new(
            "Some auto-role rules are not disjoint, refusing to process.",
        )));
    }

    let mut would_add = Vec::new();
    let mut would_remove = Vec::new();

    if !roles_to_add.is_empty() {
        would_add = add_roles_to_member(
            ctx,
            &roles_to_add.into_iter().collect::<Vec<_>>(),
            member,
            guild_roles,
            guild_id,
            dry,
        )
        .await?;
    }

    if !roles_to_remove.is_empty() {
        would_remove = remove_roles_from_member(
            ctx,
            &roles_to_remove.into_iter().collect::<Vec<_>>(),
            member,
            guild_roles,
            guild_id,
            dry,
        )
        .await?;
    }

    Ok((would_add, would_remove))
}
