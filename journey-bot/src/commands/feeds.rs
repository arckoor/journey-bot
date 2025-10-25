use std::{sync::Arc, time::Duration};

use poise::{
    CreateReply,
    serenity_prelude::{
        ChannelId, GuildId, Mentionable,
        futures::{self, Stream},
    },
};
use regex::Regex;
use roux::{Me, Reddit, Subreddit, response::BasicThing, submission::SubmissionData};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio_retry2::{Retry, RetryError, strategy::ExponentialFactorBackoff};
use tracing::{info, warn};

use crate::{
    Context, Error,
    config::RedditConfig,
    emoji::Emoji,
    store::Store,
    utils::{BotError, LogError, eph, guild_log, now, send_message, timestamp_from_f64_with_tz},
    views::embed::default_embed,
};

#[derive(Clone, Debug, Deserialize)]
struct RedditToken {
    exp: f32,
}

pub struct RedditClient {
    client: RwLock<Me>,
    config: RedditConfig,
}

impl RedditClient {
    pub async fn new(config: RedditConfig) -> Result<Self, BotError> {
        let client = Self::make_client(&config).await?;

        Ok(Self {
            client: RwLock::new(client),
            config,
        })
    }

    async fn make_client(config: &RedditConfig) -> Result<Me, BotError> {
        Reddit::new(&config.user_agent, &config.id, &config.secret)
            .username(&config.username)
            .password(&config.password)
            .login()
            .await
            .map_err(|_| BotError::new("Failed to open reddit client"))
    }

    async fn get_client(&self) -> Result<Me, BotError> {
        let client = self.client.read().await;
        let token = client.config.access_token.clone().unwrap_or(String::new());
        let expiry = jsonwebtoken::dangerous::insecure_decode::<RedditToken>(token)
            .map(|t| t.claims.exp)
            .unwrap_or(0.0);
        let now = now().as_secs_f32();
        if expiry <= (now - 60.0) {
            info!("Rotating reddit access token");
            drop(client);
            let mut client = self.client.write().await;
            *client = Self::make_client(&self.config).await?;
            Ok(client.clone())
        } else {
            Ok(client.clone())
        }
    }
}

pub struct RedditScheduler;

impl RedditScheduler {
    pub async fn schedule_all(store: Arc<Store>) -> Result<(), BotError> {
        for feed in sea_entity::reddit_feed::Entity::find()
            .all(&store.db.sea)
            .await?
        {
            Self::schedule(feed.id, store.clone());
        }
        Ok(())
    }

    pub fn schedule(id: String, store: Arc<Store>) {
        tokio::spawn(async move {
            // sleep at startup so we have enough time to init everything
            tokio::time::sleep(Duration::from_secs(5)).await;
            Self::watch_subreddit(id, store.clone())
                .await
                .log("RedditScheduler::schedule")
        });
    }

    pub async fn watch_subreddit(id: String, store: Arc<Store>) -> Result<(), BotError> {
        info!("{}", format!("Started observer for feed {}", id));
        let feed = sea_entity::reddit_feed::Entity::find_by_id(&id)
            .one(&store.db.sea)
            .await?
            .ok_or(BotError::new("Feed not found"))?;
        let mut latest_time = feed.latest_post;

        async fn query_subreddit(
            store: Arc<Store>,
            subreddit: &str,
        ) -> Result<Vec<BasicThing<SubmissionData>>, RetryError<()>> {
            let client = store
                .reddit_client
                .get_client()
                .await
                .map_err(|_| RetryError::to_transient::<()>(()).unwrap_err())?;
            let subreddit = Subreddit::new_oauth(subreddit, &client.client);

            match subreddit.latest(25, None).await {
                Ok(data) => Ok(data.data.children.into_iter().rev().collect()),
                Err(err) => {
                    warn!("Got error from reddit api: {}", err);
                    RetryError::to_transient(())
                }
            }
        }

        let retry_strategy = ExponentialFactorBackoff::from_millis(
            Duration::from_secs(15).as_millis().try_into().unwrap(),
            2.0,
        )
        .max_delay(Duration::from_secs(60 * 60 * 2));

        loop {
            // refetch every time, if it's removed we just terminate
            let Ok(Some(mut feed)) = sea_entity::reddit_feed::Entity::find_by_id(&id)
                .one(&store.db.sea)
                .await
            else {
                info!(
                    "{}",
                    format!(
                        "Terminated observer for feed {} because it doesn't exist anymore",
                        id
                    )
                );
                break Ok(());
            };

            let submissions = Retry::spawn(retry_strategy.clone(), || {
                query_subreddit(store.clone(), &feed.subreddit)
            })
            .await
            .unwrap();

            for submission in submissions {
                if submission.data.created_utc > latest_time {
                    Self::post(store.clone(), &feed, &submission).await;
                    latest_time = submission.data.created;
                    {
                        let mut updated_feed = feed.into_active_model();
                        updated_feed.latest_post = Set(submission.data.created);
                        feed = updated_feed.update(&store.db.sea).await?;
                    }
                }
            }
            // we don't want to hammer the api
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    async fn post(
        store: Arc<Store>,
        feed: &sea_entity::reddit_feed::Model,
        submission: &BasicThing<SubmissionData>,
    ) {
        let msg = feed
            .template
            .replace("{{title}}", &submission.data.title)
            .replace(
                "{{link}}",
                &format!("https://www.reddit.com{}", submission.data.permalink),
            );

        if let Err(err) = send_message(
            store.clone(),
            ChannelId::new(feed.channel_id as u64),
            msg,
            None,
        )
        .await
        {
            warn!(
                "Error while posting reddit post in guild {}: {err}",
                feed.guild_id
            );
            guild_log(
                store.clone(),
                GuildId::new(feed.guild_id as u64),
                Emoji::Warning,
                format!(
                    "I could not send a post message for feed (`{}`) in channel (`{}`). I will try again in 10 minutes.", feed.id, feed.channel_id
                ),
                None,
            ).await;
            tokio::time::sleep(Duration::from_secs(60 * 10)).await;
        }
    }
}

#[poise::command(
    slash_command,
    subcommands("template_help", "list", "add", "edit", "remove"),
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn feed(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Feed template help.
#[poise::command(slash_command, rename = "template-help")]
async fn template_help(ctx: Context<'_>) -> Result<(), Error> {
    let embed = default_embed(ctx)
        .title("Feed template help.")
        .description("Explanation of template syntax.")
        .fields(
            [
                ("Line breaks", "Line breaks are represented by `\\n`."),
                (
                    "Variables",
                    "Variables are replaced with the corresponding value from the feed.",
                ),
                ("{{title}}", "The title of the content."),
                ("{{link}}", "The link to the content."),
            ]
            .into_iter()
            .map(|(n, v)| (n, v, false)),
        );

    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}

/// List all feeds in the server.
#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let feeds = sea_entity::reddit_feed::Entity::find()
        .filter(sea_entity::reddit_feed::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await?;

    if feeds.is_empty() {
        eph(ctx, "No feeds found.").await?;
        return Ok(());
    }

    let mut embed = default_embed(ctx)
        .title("Feeds")
        .description("List of all feeds in the server.");

    for feed in feeds.into_iter() {
        let channel = ChannelId::new(feed.channel_id as u64)
            .name(ctx)
            .await
            .unwrap_or("Unknown".to_string());
        let latest_post =
            timestamp_from_f64_with_tz(feed.latest_post, ctx.data().clone(), guild_id).await;
        embed = embed.field(
            format!(
                "#{channel} | r/{} | ID: {} | LP: {}",
                feed.subreddit, feed.id, latest_post
            ),
            format!("`{}`", feed.template),
            false,
        );
    }

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

#[poise::command(slash_command, subcommands("reddit"))]
async fn add(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a reddit feed to the server.
#[poise::command(slash_command)]
async fn reddit(
    ctx: Context<'_>,
    #[rename = "subreddit-name"]
    #[description = "The name of the subreddit"]
    #[min_length = 1]
    #[max_length = 21]
    subreddit_name: String,
    #[description = "The template for new posts."] template: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let re = Regex::new("[a-zA-Z0-9_]{1,21}").unwrap();

    if !re.is_match(&subreddit_name) {
        eph(ctx, "Invalid subreddit name.").await?;
        return Ok(());
    }

    ctx.defer().await?;

    let template = template
        .unwrap_or("{{title}}\n{{link}}".to_string())
        .replace("\\n", "\n");

    let client = ctx.data().reddit_client.get_client().await?;
    let subreddit = Subreddit::new_oauth(&subreddit_name, &client.client);
    let Ok(_) = subreddit.about().await else {
        eph(ctx, "Subreddit not found.").await?;
        return Ok(());
    };

    let feed = sea_entity::reddit_feed::ActiveModel {
        id: Set(cuid2::slug()),
        guild_id: Set(guild_id.get() as i64),
        channel_id: Set(ctx.channel_id().get() as i64),
        subreddit: Set(subreddit_name.clone()),
        template: Set(template),
        ..Default::default()
    }
    .insert(&ctx.data().db.sea)
    .await?;

    ctx.say(format!("Added feed for r/{}", subreddit_name))
        .await?;

    RedditScheduler::schedule(feed.id.clone(), ctx.data().clone());

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Feed,
        format!(
            "A feed for r/{} (`{}`) was added to {} by {} (`{}`)",
            subreddit_name,
            feed.id,
            ctx.channel_id().mention(),
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

async fn autocomplete_id<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let guild_id = ctx.guild_id().unwrap_or(GuildId::new(1));
    let feeds = sea_entity::reddit_feed::Entity::find()
        .filter(sea_entity::reddit_feed::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        feeds
            .into_iter()
            .filter(move |m| m.id.starts_with(partial))
            .map(|m| m.id),
    )
}

/// Edit a feed.
#[poise::command(slash_command)]
async fn edit(
    ctx: Context<'_>,
    #[description = "The ID of the feed to edit."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
    #[description = "The new template to use for the feed"] template: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let feed = sea_entity::reddit_feed::Entity::find_by_id(&id)
        .filter(sea_entity::reddit_feed::Column::GuildId.eq(guild_id.get()))
        .one(&ctx.data().db.sea)
        .await?;

    let Some(feed) = feed else {
        eph(ctx, "No feed found with that ID.").await?;
        return Ok(());
    };

    let mut feed = feed.into_active_model();
    feed.template = Set(template.replace("\\n", "\n"));
    feed.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Feed,
        format!(
            "Feed `{}` was updated by {} (`{}`)",
            id,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    ctx.say("Feed updated.").await?;

    Ok(())
}

/// Remove a feed from the server.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The ID of the feed to remove."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let feed = sea_entity::reddit_feed::Entity::find_by_id(&id)
        .filter(sea_entity::reddit_feed::Column::GuildId.eq(guild_id.get()))
        .one(&ctx.data().db.sea)
        .await?;

    let Some(feed) = feed else {
        eph(ctx, "No feed found with that ID.").await?;
        return Ok(());
    };

    let channel = ChannelId::new(feed.channel_id as u64);

    let guild_log_msg = format!(
        "A feed for r/{} (`{}`) was removed from {} by {} (`{}`)",
        feed.subreddit,
        feed.id,
        channel.mention(),
        ctx.author().name,
        ctx.author().id
    );

    feed.into_active_model().delete(&ctx.data().db.sea).await?;

    ctx.say("Feed removed.").await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Feed,
        guild_log_msg,
        None,
    )
    .await;

    Ok(())
}
