use poise::{ChoiceParameter, CreateReply, serenity_prelude::ActivityType};
use tokio::time::Instant;

use crate::{
    Context, Error,
    utils::{create_activity, eph},
};

#[derive(ChoiceParameter)]
enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Competing,
    Custom,
}

impl From<ActivityKind> for ActivityType {
    fn from(value: ActivityKind) -> Self {
        match value {
            ActivityKind::Playing => ActivityType::Playing,
            ActivityKind::Streaming => ActivityType::Streaming,
            ActivityKind::Listening => ActivityType::Listening,
            ActivityKind::Watching => ActivityType::Watching,
            ActivityKind::Competing => ActivityType::Competing,
            ActivityKind::Custom => ActivityType::Custom,
        }
    }
}

/// Ping the bot.
#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let latency = ctx.ping().await.as_millis();
    let t1 = Instant::now();
    eph(ctx, format!("Websocket ping is {latency} ms")).await?;
    let rest = t1.elapsed().as_millis();
    eph(ctx, format!("REST API ping is {rest} ms")).await?;
    Ok(())
}

/// Send a message as the bot.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "SEND_MESSAGES",
    default_member_permissions = "BAN_MEMBERS"
)]
pub async fn echo(
    ctx: Context<'_>,
    #[description = "The message to send."]
    #[min_length = 1]
    #[max_length = 2000]
    message: String,
) -> Result<(), Error> {
    let message = message.replace("\\n", "\n");
    ctx.channel_id().say(&ctx, message).await?;
    ctx.say("Message sent!").await?;

    Ok(())
}

/// Change the bot presence.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR",
    owners_only
)]
pub async fn presence(
    ctx: Context<'_>,
    #[description = "The type of activity."]
    #[rename = "type"]
    kind: ActivityKind,
    #[description = "The message to display"] message: String,
    #[description = "The URL to set for streams"] url: Option<String>,
) -> Result<(), Error> {
    let activity_data = create_activity(kind.into(), &message, url.as_deref())?;
    ctx.serenity_context().set_activity(Some(activity_data));
    ctx.send(
        CreateReply::default()
            .content("Presence updated.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR",
    owners_only
)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}
