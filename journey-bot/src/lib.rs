pub mod commands;
pub mod config;
pub mod db;
pub mod emoji;
pub mod store;
pub mod utils;
pub mod views;

use std::sync::Arc;

use config::JourneyBotConfig;
use poise::{
    CreateReply,
    serenity_prelude::{self as serenity},
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    commands::{
        anti_spam::PoolManager, censor::CensorScheduler, feeds::RedditScheduler,
        streams::TwitchScheduler,
    },
    config::ActivityConfig,
    store::Store,
    utils::{LogError, create_activity},
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Arc<Store>, Error>;
type Command =
    poise::Command<Arc<Store>, Box<dyn serde::ser::StdError + std::marker::Send + Sync + 'static>>;

pub async fn launch(config: JourneyBotConfig) -> Result<(), serenity::Error> {
    let JourneyBotConfig { bot, store } = config;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::commands(),
            pre_command: |ctx| {
                Box::pin(async move {
                    let author = &ctx.author().name;
                    let author_id = &ctx.author().id;
                    let channel = &ctx
                        .channel_id()
                        .name(&ctx)
                        .await
                        .unwrap_or_else(|_| "<unknown>".to_string());
                    let channel_id = &ctx.channel_id();
                    let guild = &ctx
                        .guild_id()
                        .and_then(|id| id.name(ctx))
                        .unwrap_or("unknown".to_string());
                    let guild_id = &ctx.guild_id().map(u64::from).unwrap_or(0);

                    info!(
                        "{} ({}) - {} ({}) - #{} ({}) used slash command '{}'",
                        author,
                        author_id,
                        guild,
                        guild_id,
                        channel,
                        channel_id,
                        &ctx.invocation_string()
                    );
                })
            },
            on_error: |error| {
                Box::pin(async move {
                    match error {
                        poise::FrameworkError::Setup {
                            error, framework, ..
                        } => {
                            framework.shard_manager().shutdown_all().await;
                            panic!("{}", error);
                        }
                        poise::FrameworkError::Command { error, ctx, .. } => {
                            let error = error.to_string();
                            warn!("An error occurred in a command: {}", error);
                            let _ = ctx
                                .send(CreateReply::default().content(error).ephemeral(true))
                                .await;
                            return;
                        }
                        _ => {}
                    }
                    if let Err(e) = poise::builtins::on_error(error).await {
                        tracing::error!("Error while handling error: {}", e);
                    }
                })
            },
            event_handler: |ctx, event, _framework, store| {
                Box::pin(async move { event_handler(ctx, event, store.clone()).await })
            },
            initialize_owners: true,
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                let (tx, rx) = mpsc::channel(100);
                let store = Arc::new(Store::new(store, ctx.http.clone(), tx).await?);
                #[cfg(debug_assertions)]
                {
                    use poise::serenity_prelude::GuildId;

                    let guild_id = GuildId::new(store.admin_guild);
                    guild_id.set_commands(ctx, vec![]).await?;
                    poise::builtins::register_in_guild(
                        ctx,
                        &framework.options().commands,
                        guild_id,
                    )
                    .await?;
                }
                #[cfg(not(debug_assertions))]
                {
                    serenity::Command::set_global_commands(ctx, vec![]).await?;
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                }
                RedditScheduler::schedule_all(store.clone()).await?;
                TwitchScheduler::schedule_all(store.clone()).await?;
                PoolManager::schedule(store.clone(), rx);
                CensorScheduler::schedule_all(store.clone()).await;
                Ok(store)
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let mut client = serenity::ClientBuilder::new(bot.bot_token, intents).framework(framework);

    if let Some(activity) = bot.activity {
        let ActivityConfig { kind, message, url } = activity;
        let activity_data = create_activity(kind, &message, url.as_deref()).unwrap();
        client = client.activity(activity_data);
    }

    client.await.unwrap().start().await
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    store: Arc<Store>,
) -> Result<(), Error> {
    #[cfg(debug_assertions)]
    info!("Got an event: {:?}", event.snake_case_name());

    match event {
        serenity::FullEvent::GuildMemberAddition { new_member } => {
            commands::guild_config::on_member_join(store.clone(), new_member)
                .await
                .log("commands::modlog::on_member_join")
        }
        serenity::FullEvent::GuildMemberUpdate { new, event, .. } => {
            commands::ensure_role::on_member_update(store.clone(), ctx, new, event)
                .await
                .log("commands::ensure_role::on_member_update");
        }
        serenity::FullEvent::Message { new_message } => {
            commands::anti_spam::on_message(store.clone(), new_message)
                .await
                .log("commands::anti_spam::on_message");
            commands::censor::on_message(store.clone(), new_message)
                .await
                .log("commands::censor::on_message");
            commands::sticky::on_message(store.clone(), new_message)
                .await
                .log("commands::sticky::on_message");
        }
        _ => {}
    }

    Ok(())
}
