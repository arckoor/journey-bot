use std::{collections::HashMap, sync::Arc, time::Duration};

use poise::{
    CreateReply,
    serenity_prelude::{
        CacheHttp, ChannelId, EditMessage, GuildId, Mentionable, MessageId, futures,
    },
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};
use tokio::sync::RwLock;
use tokio_retry2::{Retry, RetryError, strategy::ExponentialFactorBackoff};
use tracing::{info, warn};
use twitch_api::{
    helix::{
        Request, RequestGet, Response,
        games::GetGamesRequest,
        streams::{GetStreamsRequest, Stream},
        users::GetUsersRequest,
    },
    twitch_oauth2::{AppAccessToken, ClientId, ClientSecret, TwitchToken},
};

use crate::{
    Context, Error,
    config::TwitchConfig,
    emoji::Emoji,
    store::Store,
    utils::{
        BotError, LogError, eph, fetch_sheet, guild_log, now, schedule_at_interval, send_message,
    },
    views::embed::default_embed,
};

const POSTED_STREAM_LIFETIME: u64 = 60 * 60 * 24 * 27;
const BLACKLIST_STAGING_TIME: u64 = 60 * 60 * 24;

pub struct TwitchClient {
    token: RwLock<AppAccessToken>,
    client: twitch_api::TwitchClient<'static, reqwest::Client>,
    filter_words: Vec<String>,
    new_threshold: f64,
    disappear_threshold: f64,
    offline_threshold: f64,
    max_concurrent_streams: usize,
}

impl TwitchClient {
    pub async fn new(config: TwitchConfig) -> Result<Self, BotError> {
        let client = twitch_api::TwitchClient::<reqwest::Client>::new();

        let token = AppAccessToken::get_app_access_token(
            &client,
            ClientId::from(config.id),
            ClientSecret::from(config.secret),
            vec![],
        )
        .await
        .map_err(|_| BotError::new("Error while getting initial twitch token"))?;

        Ok(Self {
            token: RwLock::new(token),
            client,
            filter_words: config.filter_words,
            new_threshold: config.new_threshold,
            disappear_threshold: config.disappear_threshold,
            offline_threshold: config.offline_threshold,
            max_concurrent_streams: config.max_concurrent_streams,
        })
    }

    async fn get_token(&self) -> Result<AppAccessToken, BotError> {
        let token = self.token.read().await;
        if token.is_elapsed() {
            drop(token);
            let mut token = self.token.write().await;
            if token.is_elapsed() {
                token
                    .refresh_token(&self.client)
                    .await
                    .map_err(|_| BotError::new("Error while refreshing twitch token"))?;
            }
            Ok(token.clone())
        } else {
            Ok(token.clone())
        }
    }

    pub async fn req<R, D>(&self, req: R) -> Result<Response<R, D>, BotError>
    where
        R: Request<Response = D> + Request + RequestGet,
        D: serde::de::DeserializeOwned + PartialEq,
    {
        self.client
            .helix
            .req_get(req, &self.get_token().await?)
            .await
            .map_err(|e| BotError::new(format!("Error making request: {e}")))
    }
}

pub struct TwitchScheduler;

impl TwitchScheduler {
    pub async fn schedule_all(store: Arc<Store>) {
        for stream in sea_entity::stream_observer::Entity::find()
            .all(&store.db.sea)
            .await
            .expect("Failed to fetch stream observers")
        {
            Self::schedule(stream.id, store.clone());
        }

        schedule_at_interval(
            store.clone(),
            Duration::from_secs(60 * 60),
            |store: Arc<Store>| async move {
                Self::update_auto_blacklists(store.clone()).await;
                Self::add_staged_censor_items(store.clone()).await;
            },
        );
        schedule_at_interval(
            store.clone(),
            Duration::from_secs(60 * 60 * 2),
            Self::expire_old_posted_streams,
        );
    }

    pub fn schedule(id: String, store: Arc<Store>) {
        tokio::spawn(async move {
            // sleep at startup so we have enough time to init everything
            tokio::time::sleep(Duration::from_secs(5)).await;
            Self::watch_game(id, store.clone()).await
        });
    }

    async fn update_auto_blacklists(store: Arc<Store>) {
        let Ok(observers) = sea_entity::stream_observer::Entity::find()
            .all(&store.db.sea)
            .await
        else {
            return;
        };

        for observer in observers {
            if let Some(sheet_id) = observer.auto_blacklist_sheet_id.clone()
                && let Some(column_name) = observer.auto_blacklist_column_name.clone()
            {
                let observer_id = observer.id.clone();
                let guild_id = GuildId::new(observer.guild_id as u64);
                let columns = vec![column_name.to_string()];
                let Ok((staged, removed)) = store
                    .db
                    .stage_new_items::<sea_entity::stream_observer::Entity>(
                        observer,
                        &observer_id,
                        columns,
                        &sheet_id,
                    )
                    .await
                else {
                    continue;
                };

                for added in staged {
                    guild_log(
                        store.clone(),
                        guild_id,
                        Emoji::Twitch,
                        format!(
                            "User `{}` was staged to be added to the blacklist from observer `{}`",
                            added, observer_id
                        ),
                        None,
                    )
                    .await;
                }

                for (rem, was_staged) in removed {
                    if was_staged {
                        guild_log(
                            store.clone(),
                            guild_id,
                            Emoji::Twitch,
                            format!(
                                "User `{}` was removed from the blacklist staging area for observer `{}`",
                                rem, observer_id
                            ),
                            None,
                        )
                        .await;
                    } else {
                        guild_log(
                            store.clone(),
                            guild_id,
                            Emoji::Twitch,
                            format!(
                                "User `{}` was removed from the blacklist for observer `{}`",
                                rem, observer_id
                            ),
                            None,
                        )
                        .await;
                    }
                }
            }
        }
    }

    async fn add_staged_censor_items(store: Arc<Store>) {
        let Ok(observers) = sea_entity::stream_observer::Entity::find()
            .all(&store.db.sea)
            .await
        else {
            return;
        };

        let now = now().as_secs_f64();
        let diff = now - BLACKLIST_STAGING_TIME as f64;

        for observer in observers {
            let observer_id = observer.id.clone();
            let guild_id = GuildId::new(observer.guild_id as u64);
            let channel_id = ChannelId::new(observer.channel_id as u64);

            let Ok(committed) = store
                .db
                .commit_staged_items::<sea_entity::stream_observer::Entity>(
                    &observer_id,
                    diff,
                    observer,
                )
                .await
            else {
                continue;
            };

            for added in committed {
                guild_log(
                    store.clone(),
                    guild_id,
                    Emoji::Twitch,
                    format!(
                        "User `{}` was blacklisted from observer `{}`",
                        added, observer_id
                    ),
                    None,
                )
                .await;
                Self::delete_posted_streams(
                    store.clone(),
                    &observer_id,
                    guild_id,
                    channel_id,
                    &added,
                )
                .await;
            }
        }
    }

    async fn expire_old_posted_streams(store: Arc<Store>) {
        let now = now().as_secs_f64();
        let diff = now - POSTED_STREAM_LIFETIME as f64;
        let _ = sea_entity::posted_stream::Entity::delete_many()
            .filter(sea_entity::posted_stream::Column::CreatedAt.lt(diff))
            .exec(&store.db.sea)
            .await;
    }

    async fn delete_posted_streams(
        store: Arc<Store>,
        observer: &str,
        guild_id: GuildId,
        channel_id: ChannelId,
        user_login: &str,
    ) {
        let Ok(posted_streams) = sea_entity::posted_stream::Entity::delete_many()
            .filter(
                sea_orm::Condition::all()
                    .add(sea_entity::posted_stream::Column::UserLogin.eq(user_login))
                    .add(sea_entity::posted_stream::Column::StreamObserverId.eq(observer)),
            )
            .exec_with_returning(&store.db.sea)
            .await
        else {
            return;
        };

        let posted_streams = posted_streams
            .into_iter()
            .map(|m| MessageId::new(m.message_id as u64))
            .collect::<Vec<_>>();

        let posted_streams_count = posted_streams.len();

        for chunk in posted_streams.chunks(100) {
            if chunk.len() == 1 {
                let message = chunk.iter().next().unwrap();
                let _ = store.ctx.delete_message(channel_id, *message, None).await;
            } else {
                let _ = store
                    .ctx
                    .delete_messages(
                        channel_id,
                        &serde_json::to_value(HashMap::from([("messages", chunk)])).unwrap(),
                        None,
                    )
                    .await;
            }
        }

        if posted_streams_count > 0 {
            guild_log(
                store.clone(),
                guild_id,
                Emoji::Twitch,
                format!(
                    "User `{}` had {} message(s) removed from observer `{}` because they were blacklisted.",
                    user_login,
                    posted_streams_count,
                    observer
                ),
                None
            )
            .await;
        }
    }

    async fn watch_game(id: String, store: Arc<Store>) {
        info!("Starting stream observer for stream {}", id);

        async fn query_stream<'a>(
            store: Arc<Store>,
            game_ids: &'a [String],
        ) -> Result<
            Response<GetStreamsRequest<'a>, Vec<twitch_api::helix::streams::Stream>>,
            RetryError<()>,
        > {
            match store
                .twitch_client
                .req(
                    GetStreamsRequest::game_ids(game_ids)
                        .first(store.twitch_client.max_concurrent_streams),
                )
                .await
            {
                Ok(data) => Ok(data),
                Err(_) => RetryError::to_transient(()),
            }
        }

        let retry_strategy = ExponentialFactorBackoff::from_millis(
            Duration::from_secs(15).as_millis().try_into().unwrap(),
            2.0,
        )
        .max_delay(Duration::from_secs(60 * 60 * 2));

        loop {
            let Ok(Some(observer)) = sea_entity::stream_observer::Entity::find_by_id(&id)
                .one(&store.db.sea)
                .await
            else {
                break;
            };
            let game_ids = [observer.game_id.clone()];

            let streams = Retry::spawn(retry_strategy.clone(), || {
                query_stream(store.clone(), &game_ids)
            })
            .await
            .unwrap();

            if streams.data.len() == store.twitch_client.max_concurrent_streams {
                let _ = guild_log(
                    store.clone(),
                    GuildId::new(observer.guild_id as u64),
                    Emoji::Warning,
                    format!(
                        "Stream observer `{}` (`{}`) has reached the maximum number of concurrent streams and is at risk of dropping streams.",
                        observer.id, observer.game_id
                    ),
                    None
                ).await;
            }

            let now = now().as_secs_f64();
            for stream in streams.data {
                Self::process_stream(store.clone(), stream, &observer, now)
                    .await
                    .log("TwitchScheduler::process_stream");
            }
            Self::remove_known_streams(store.clone(), observer, now)
                .await
                .log("TwitchScheduler::remove_known_streams");
            // we don't want to hammer the api
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    async fn process_stream(
        store: Arc<Store>,
        stream: Stream,
        observer: &sea_entity::stream_observer::Model,
        now: f64,
    ) -> Result<(), BotError> {
        let filtered = {
            let title = stream.title.to_lowercase();
            let tags = stream
                .tags
                .iter()
                .map(|tag| tag.to_lowercase())
                .collect::<Vec<_>>();
            store
                .twitch_client
                .filter_words
                .iter()
                .any(|fw| title.contains(fw) || tags.iter().any(|tag| tag.contains(fw)))
        };
        if observer
            .blacklist
            .contains(&stream.user_login.as_str().to_owned())
            || observer
                .auto_blacklist
                .contains(&stream.user_login.as_str().to_owned())
            || stream.game_id.as_str() != observer.game_id
            || filtered
        {
            return Ok(());
        }

        if let Some(users) = store
            .twitch_client
            .req(GetUsersRequest::logins(vec![stream.user_login.clone()]))
            .await
            .ok()
            && let Some(user) = users.first()
            && let Some(desc) = user.description.map(|desc| desc.to_lowercase())
            && store
                .twitch_client
                .filter_words
                .iter()
                .any(|fw| desc.contains(fw))
        {
            return Ok(());
        }

        let mut message_id = None;
        let ks_id = Self::check_stream_known(store.clone(), &stream, observer).await;
        if ks_id.is_none() {
            info!(
                "Found new stream for observer {}: {}, {} is live playing {} since {}",
                observer.id,
                stream.id.as_str(),
                stream.user_name.as_str(),
                stream.game_name,
                stream.started_at
            );
            let message = Self::post(store.clone(), observer, &stream).await?;
            let _ = sea_entity::posted_stream::ActiveModel {
                message_id: Set(message.get() as i64),
                user_login: Set(stream.user_login.to_string()),
                stream_observer_id: Set(observer.id.clone()),
                ..Default::default()
            }
            .insert(&store.db.sea)
            .await;

            message_id = Some(message);
        }
        Self::update_known_stream(store.clone(), observer, stream, now, message_id, ks_id).await?;
        Ok(())
    }

    async fn check_stream_known(
        store: Arc<Store>,
        stream: &Stream,
        observer: &sea_entity::stream_observer::Model,
    ) -> Option<i32> {
        let ks = sea_entity::known_stream::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(sea_entity::known_stream::Column::StreamObserverId.eq(&observer.id))
                    .add(
                        sea_orm::Condition::any()
                            .add(sea_entity::known_stream::Column::StreamId.eq(stream.id.as_str()))
                            .add(
                                sea_orm::Condition::all()
                                    .add(
                                        sea_entity::known_stream::Column::UserId
                                            .eq(stream.user_id.as_str()),
                                    )
                                    .add(
                                        sea_entity::known_stream::Column::UserLogin
                                            .eq(stream.user_login.as_str()),
                                    ),
                            ),
                    ),
            )
            .one(&store.db.sea)
            .await
            .ok()
            .flatten();

        if let Some(ks) = ks {
            if ks.stream_id != stream.id.as_str() {
                info!(
                    "Known stream {} has changed stream id to {}",
                    ks.stream_id,
                    stream.id.as_str()
                );
            }
            Some(ks.id)
        } else {
            None
        }
    }

    async fn update_known_stream(
        store: Arc<Store>,
        observer: &sea_entity::stream_observer::Model,
        stream: Stream,
        now: f64,
        message_id: Option<MessageId>,
        ks_id: Option<i32>,
    ) -> Result<(), BotError> {
        if let Some(id) = ks_id {
            let mut known_stream = sea_entity::known_stream::Entity::find_by_id(id)
                .one(&store.db.sea)
                .await?
                .ok_or(BotError::new("Known stream not found despite ID given"))?
                .into_active_model();
            known_stream.last_seen = Set(now);
            known_stream.stream_id = Set(stream.id.take());
            known_stream.update(&store.db.sea).await?;
        } else {
            sea_entity::known_stream::ActiveModel {
                stream_id: Set(stream.id.take()),
                stream_observer_id: Set(observer.id.clone()),
                user_id: Set(stream.user_id.take()),
                user_login: Set(stream.user_login.take()),
                first_seen: Set(now),
                last_seen: Set(now),
                message_id: Set(message_id.map(|id| id.get() as i64)),
                ..Default::default()
            }
            .insert(&store.db.sea)
            .await?;
        }

        Ok(())
    }

    async fn remove_known_streams(
        store: Arc<Store>,
        observer: sea_entity::stream_observer::Model,
        now: f64,
    ) -> Result<(), BotError> {
        let known_streams = sea_entity::known_stream::Entity::find()
            .filter(sea_entity::known_stream::Column::StreamObserverId.eq(&observer.id))
            .all(&store.db.sea)
            .await?;
        let channel_id = ChannelId::new(observer.channel_id as u64);

        for ks in known_streams {
            let first_seen = ks.first_seen;
            let last_seen = ks.last_seen;
            // we just updated this stream, so it's most certainly not "old"
            if last_seen == now {
                continue;
            }
            let changed_game =
                Self::check_stream_game_changed(store.clone(), &ks.user_login, &observer.game_id)
                    .await;
            // the stream started recently enough to look at it in detail
            if (now - first_seen) <= store.twitch_client.new_threshold {
                if (changed_game || (now - last_seen) >= store.twitch_client.disappear_threshold)
                    && let Some(message_id) = ks.message_id
                {
                    let message_id = MessageId::new(message_id as u64);
                    let _ = store.ctx.delete_message(channel_id, message_id, None).await;
                    let _ =
                        sea_entity::posted_stream::Entity::delete_by_id(message_id.get() as i64)
                            .exec(&store.db.sea)
                            .await;
                    info!(
                        "Known stream {} from observer {} ({}) was removed because it disappeared to quickly",
                        ks.id, observer.id, observer.game_id
                    );
                    let _ = ks.into_active_model().delete(&store.db.sea).await;
                }
            }
            // it's a decently old stream
            else if (changed_game || (now - last_seen) > store.twitch_client.offline_threshold)
                && let Some(message_id) = ks.message_id
            {
                let message_id = MessageId::new(message_id as u64);
                if let Ok(mut msg) = store.ctx.get_message(channel_id, message_id).await {
                    let _ = msg
                        .edit(
                            &store.ctx.http(),
                            EditMessage::new().content(format!(
                                "{}{}",
                                msg.content,
                                observer
                                    .end_template
                                    .replace("{{game}}", &observer.game_name)
                            )),
                        )
                        .await;
                }
                info!(
                    "Removed known stream {} from observer {} ({})",
                    ks.id, observer.id, observer.game_id
                );
                let _ = ks.into_active_model().delete(&store.db.sea).await;
            }
        }

        Ok(())
    }

    async fn check_stream_game_changed(
        store: Arc<Store>,
        user_login: &str,
        expected_game_id: &str,
    ) -> bool {
        let user_logins = vec![user_login];
        let Ok(stream) = store
            .twitch_client
            .req(GetStreamsRequest::user_logins(user_logins))
            .await
        else {
            return false;
        };

        let Some(stream) = stream.data.first() else {
            return false;
        };

        stream.game_id.as_str() != expected_game_id
    }

    async fn post(
        store: Arc<Store>,
        observer: &sea_entity::stream_observer::Model,
        stream: &Stream,
    ) -> Result<MessageId, BotError> {
        let msg = observer
            .template
            .replace("{{title}}", &Self::escape(&stream.title))
            .replace("{{user}}", &Self::escape(stream.user_name.as_str()))
            .replace("{{user_login}}", &Self::escape(stream.user_login.as_str()))
            .replace("{{game}}", &Self::escape(&stream.game_name))
            .replace(
                "{{tags}}",
                &stream
                    .tags
                    .iter()
                    .map(|tag| format!("`{tag}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .replace("{{viewer_count}}", &format!("{}", stream.viewer_count))
            .replace(
                "{{link}}",
                &format!("https://www.twitch.tv/{}", stream.user_login.as_str()),
            );

        match send_message(
            store.clone(),
            ChannelId::new(observer.channel_id as u64),
            msg,
            None,
        )
        .await
        {
            Ok(msg) => Ok(msg.id),
            Err(err) => {
                warn!(
                    "Error while posting stream in guild {}: {}",
                    observer.guild_id, err
                );
                guild_log(
                    store.clone(),
                    GuildId::new(observer.guild_id as u64),
                    Emoji::Warning,
                    format!("I could not send a stream message for observer (`{}`) in channel (`{}`). I will try again in 10 minutes.", observer.id, observer.channel_id
                    ),
                    None
                )
                .await;
                tokio::time::sleep(Duration::from_secs(60 * 10)).await;
                Err(BotError::new(format!(
                    "Unable to send stream message for observer {}",
                    observer.id
                )))
            }
        }
    }

    fn escape(text: &str) -> String {
        let mut text = text.to_string();
        for char in ["_", "*", "~", "`", "|"] {
            text = text.replace(char, &format!("\\{char}"));
        }
        text
    }
}

#[poise::command(
    slash_command,
    subcommands(
        "template_help",
        "list",
        "info",
        "add",
        "edit",
        "remove",
        "blacklist_add",
        "blacklist_remove",
        "blacklist_sheet_set",
        "blacklist_sheet_remove"
    ),
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "SEND_MESSAGES",
    rename = "stream-observer"
)]
pub async fn stream_observer(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Stream observer template help.
#[poise::command(slash_command, rename = "template-help")]
async fn template_help(ctx: Context<'_>) -> Result<(), Error> {
    let embed = default_embed(ctx)
        .title("Stream observer template help.")
        .description("Explanation of the template syntax.")
        .fields(
            [
                ("Line breaks", "Line breaks are represented by `\\n`."),
                (
                    "Variables",
                    "Variables are replaced with the corresponding value from a stream.",
                ),
                ("{{title}}", "The title of the stream."),
                ("{{user}}", "The user streaming."),
                ("{{user_login}}", "The user streaming."),
                ("{{game}}", "The game being streamed."),
                ("{{tags}}", "The tags of the stream."),
                ("{{viewer_count}}", "The number of viewers."),
                ("{{link}}", "The link to the stream."),
                (
                    "End template",
                    "For the end template, only {{game}} is available.",
                ),
            ]
            .into_iter()
            .map(|(n, v)| (n, v, false)),
        );

    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}

/// List all stream observers.
#[poise::command(slash_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let observers = sea_entity::stream_observer::Entity::find()
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db.sea)
        .await?;

    if observers.is_empty() {
        eph(ctx, "No stream observers found.").await?;
        return Ok(());
    }

    let mut embed = default_embed(ctx)
        .title("Stream observers")
        .description("List of all stream observers.");

    for observer in observers {
        let channel = ChannelId::new(observer.channel_id as u64)
            .name(ctx)
            .await
            .unwrap_or("Unknown".to_string());
        embed = embed.field(
            format!("#{channel} | ID: {}", observer.id),
            format!("Game ID: {}", observer.game_id),
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
    let observers = sea_entity::stream_observer::Entity::find()
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get()))
        .all(&ctx.data().db.sea)
        .await
        .unwrap_or(Vec::new());

    futures::stream::iter(
        observers
            .into_iter()
            .filter(move |m| m.id.starts_with(partial))
            .map(|m| m.id),
    )
}

/// Get info about a stream observer.
#[poise::command(slash_command)]
async fn info(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to get info about."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };
    let channel = ChannelId::new(observer.channel_id as u64).mention();

    let blacklist = if observer.blacklist.is_empty() {
        "None".to_string()
    } else {
        observer.blacklist.join(", ")
    };
    let auto_blacklist = if observer.auto_blacklist.is_empty() {
        "None".to_string()
    } else {
        observer.auto_blacklist.join(", ")
    };

    let embed = default_embed(ctx)
        .title("Stream observer info")
        .description("Info about a stream observer")
        .fields(
            [
                ("ID", observer.id),
                ("Game ID", observer.game_id),
                ("Game name", observer.game_name),
                ("Channel", channel.to_string()),
                ("Template", format!("`{}`", observer.template)),
                ("End template", format!("`{}`", observer.end_template)),
                ("Blacklisted users", blacklist),
                ("Auto blacklisted users", auto_blacklist),
            ]
            .into_iter()
            .map(|(n, v)| (n, v, false)),
        );

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Observe a game for streams.
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The ID of the game to watch."]
    #[min_length = 1]
    #[rename = "game-id"]
    game_id: String,
    #[description = "The template to use for the stream."] template: Option<String>,
    #[description = "The template to use for the end of the stream. Gets appended to the message when the stream ends."]
    end_template: Option<String>,
    #[description = "The ID of the google sheet to update the auto blacklist from."]
    #[rename = "blacklist-sheet-id"]
    blacklist_sheet_id: Option<String>,
    #[description = "The name of the column that contains user logins to blacklist"]
    #[rename = "blacklist-sheet-column-name"]
    blacklist_sheet_column_name: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let game_ids = vec![game_id];
    let games = ctx
        .data()
        .twitch_client
        .req(GetGamesRequest::ids(game_ids))
        .await?
        .data;

    if games.len() != 1 {
        eph(ctx, "Game not found.").await?;
        return Ok(());
    }

    let game = games.into_iter().next().unwrap();

    let template = template
        .unwrap_or(
            "{{user}} is playing {{game}} with {{viewer_count}} viewers.\n{{title}} - {{link}}"
                .to_string(),
        )
        .replace("\\n", "\n");
    let end_template = end_template
        .unwrap_or("\n\n{{game}} is no longer being streamed.".to_string())
        .replace("\\n", "\n");

    if blacklist_sheet_id.is_some() && blacklist_sheet_column_name.is_none()
        || blacklist_sheet_id.is_none() && blacklist_sheet_column_name.is_some()
    {
        eph(ctx, "If one of blacklist-sheet-id and blacklist-sheet-column-name is specified, the other must be specified too.").await?;
        return Ok(());
    }

    let mut has_sheet = false;

    if let Some(sheet_id) = &blacklist_sheet_id
        && let Some(column_name) = &blacklist_sheet_column_name
    {
        let mut sheet = fetch_sheet(sheet_id).await?;
        if !sheet
            .headers()
            .map_err(|_| BotError::new("Failed to deserialize sheet headers"))?
            .iter()
            .any(|h| h == column_name)
        {
            eph(ctx, "The specified column was not found in the sheet.").await?;
            return Ok(());
        }
        has_sheet = true;
    }

    let observer = sea_entity::stream_observer::ActiveModel {
        id: Set(cuid2::slug()),
        guild_id: Set(guild_id.get() as i64),
        channel_id: Set(ctx.channel_id().get() as i64),
        game_id: Set(game.id.to_string()),
        game_name: Set(game.name.clone()),
        template: Set(template),
        end_template: Set(end_template),
        blacklist: Set(vec![]),
        auto_blacklist_sheet_id: Set(blacklist_sheet_id),
        auto_blacklist_column_name: Set(blacklist_sheet_column_name),
        auto_blacklist: Set(vec![]),
    }
    .insert(&ctx.data().db.sea)
    .await?;

    ctx.say(format!("Added stream observer for {}.", game.name))
        .await?;

    TwitchScheduler::schedule(observer.id.clone(), ctx.data().clone());

    info!(
        "A stream observer {} ({} - {}) was added to channel {} ({}) by {} ({})",
        observer.id,
        observer.game_id,
        observer.game_name,
        ctx.channel_id()
            .name(&ctx)
            .await
            .unwrap_or("<#Unknown>".to_string()),
        ctx.channel_id().get(),
        ctx.author().name,
        ctx.author().id
    );

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        format!(
            "A stream observer `{}` (`{}` - `{}`) was added to {} (`{}`) by {} (`{}`)",
            observer.id,
            observer.game_id,
            observer.game_name,
            ctx.channel_id().mention(),
            ctx.channel_id().get(),
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    if has_sheet {
        TwitchScheduler::update_auto_blacklists(ctx.data().clone()).await;
    }

    Ok(())
}

/// Edit a stream observer.
#[poise::command(slash_command)]
async fn edit(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to edit."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
    #[description = "The new template to use for the observer"] template: Option<String>,
    #[description = "The new end template to use for the observer"]
    #[rename = "end-template"]
    end_template: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(&id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    if template.is_none() && end_template.is_none() {
        eph(ctx, "You must specify a new template or a new end template").await?;
        return Ok(());
    }

    let mut observer = observer.into_active_model();
    if let Some(template) = template {
        observer.template = Set(template.replace("\\n", "\n"));
    }
    if let Some(end_template) = end_template {
        observer.end_template = Set(end_template.replace("\\n", "\n"));
    }
    observer.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        format!(
            "Stream observer `{}` was updated by {} (`{}`)",
            id,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    ctx.say("Stream observer updated.").await?;

    Ok(())
}

/// Remove a stream observer.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to remove."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    id: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    let log_msg = format!(
        "A stream observer {} ({} - {}) was removed from channel {} ({}) by {} ({})",
        observer.id,
        observer.game_id,
        observer.game_name,
        ctx.channel_id()
            .name(&ctx)
            .await
            .unwrap_or("<#Unknown>".to_string()),
        ctx.channel_id().get(),
        ctx.author().name,
        ctx.author().id
    );
    let guild_log_msg = format!(
        "A stream observer `{}` (`{}` - `{}`) was removed from {} (`{}`) by {} (`{}`)",
        observer.id,
        observer.game_id,
        observer.game_name,
        ctx.channel_id().mention(),
        ctx.channel_id().get(),
        ctx.author().name,
        ctx.author().id
    );

    observer
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    ctx.say("Stream observer removed.").await?;
    info!(log_msg);
    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        guild_log_msg,
        None,
    )
    .await;

    Ok(())
}

/// Blacklist a Twitch user.
#[poise::command(slash_command, rename = "blacklist-user")]
async fn blacklist_add(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to blacklist the user from."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    #[rename = "observer-id"]
    observer_id: String,
    #[description = "The user to blacklist"]
    #[min_length = 1]
    #[rename = "user-login"]
    user_login: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(&observer_id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    if observer.blacklist.contains(&user_login) {
        eph(ctx, "User already blacklisted.").await?;
        return Ok(());
    }

    let user_logins = vec![user_login.clone()];
    let user = ctx
        .data()
        .twitch_client
        .req(GetUsersRequest::logins(user_logins))
        .await?
        .data
        .into_iter()
        .next();

    let mut blacklist = observer.blacklist.clone();
    blacklist.push(user_login.clone());

    let channel_id = ChannelId::new(observer.channel_id as u64);
    let mut observer = observer.into_active_model();
    observer.blacklist = Set(blacklist);
    observer.update(&ctx.data().db.sea).await?;

    if let Some(user) = user {
        ctx.say(format!(
            "User `{}` (`{}`) added to the blacklist.",
            user.display_name, user.id
        ))
        .await?;
    } else {
        ctx.say(format!(
            "I didn't find a user with the login `{}`, but I added them to the blacklist anyway.",
            user_login
        ))
        .await?;
    }
    info!(
        "User {} was blacklisted from observer {} by {} ({})",
        user_login,
        observer_id,
        ctx.author().name,
        ctx.author().id
    );
    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        format!(
            "User `{}` was blacklisted from observer `{}` by {} (`{}`)",
            user_login,
            observer_id,
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;
    TwitchScheduler::delete_posted_streams(
        ctx.data().clone(),
        &observer_id,
        guild_id,
        channel_id,
        &user_login,
    )
    .await;

    Ok(())
}

/// Remove a user from the blacklist.
#[poise::command(slash_command, rename = "un-blacklist-user")]
async fn blacklist_remove(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to un-blacklist the user from."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    #[rename = "observer-id"]
    observer_id: String,
    #[description = "The user to remove from the blacklist"]
    #[min_length = 1]
    #[rename = "user-login"]
    user_login: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(observer_id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    if !observer.blacklist.contains(&user_login) {
        eph(ctx, "User not blacklisted.").await?;
        return Ok(());
    }

    let blacklist = observer
        .blacklist
        .clone()
        .into_iter()
        .filter(|user| *user != user_login)
        .collect();

    let log_msg = format!(
        "User {} was removed from the blacklist for observer {} by {} ({})",
        user_login,
        observer.id,
        ctx.author().name,
        ctx.author().id,
    );

    let guild_log_msg = format!(
        "User `{}` was removed from the blacklist for observer `{}` by {} (`{}`)",
        user_login,
        observer.id,
        ctx.author().name,
        ctx.author().id,
    );

    let mut observer = observer.into_active_model();
    observer.blacklist = Set(blacklist);
    observer.update(&ctx.data().db.sea).await?;

    ctx.say(format!("User `{}` removed from blacklist.", user_login))
        .await?;

    info!(log_msg);
    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        guild_log_msg,
        None,
    )
    .await;

    Ok(())
}

/// Set an auto blacklist sheet for an observer.
#[poise::command(slash_command, rename = "blacklist-sheet-set")]
async fn blacklist_sheet_set(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to add the auto blacklist sheet to."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    #[rename = "observer-id"]
    observer_id: String,
    #[description = "The ID of the google sheet to update the auto blacklist from."]
    #[rename = "blacklist-sheet-id"]
    blacklist_sheet_id: String,
    #[description = "The name of the column that contains user logins to blacklist"]
    #[rename = "blacklist-sheet-column-name"]
    blacklist_sheet_column_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(&observer_id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    let mut sheet = fetch_sheet(&blacklist_sheet_id).await?;
    if !sheet
        .headers()
        .map_err(|_| BotError::new("Failed to deserialize sheet headers"))?
        .iter()
        .any(|h| h == blacklist_sheet_column_name)
    {
        eph(ctx, "The specified column was not found in the sheet.").await?;
        return Ok(());
    }

    let guild_log_msg = format!(
        "An auto blacklist sheet `{}` was added for observer `{}` by {} (`{}`)",
        blacklist_sheet_id,
        observer_id,
        ctx.author().name,
        ctx.author().id,
    );

    let mut observer = observer.into_active_model();
    observer.auto_blacklist_sheet_id = Set(Some(blacklist_sheet_id));
    observer.auto_blacklist_column_name = Set(Some(blacklist_sheet_column_name));
    observer.update(&ctx.data().db.sea).await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Twitch,
        guild_log_msg,
        None,
    )
    .await;

    ctx.say("Blacklist sheet added.").await?;

    Ok(())
}

/// Remove the auto blacklist sheet for an observer.
#[poise::command(slash_command, rename = "blacklist-sheet-remove")]
async fn blacklist_sheet_remove(
    ctx: Context<'_>,
    #[description = "The ID of the stream observer to add the auto blacklist sheet to."]
    #[min_length = 10]
    #[max_length = 10]
    #[autocomplete = "autocomplete_id"]
    #[rename = "observer-id"]
    observer_id: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;
    let Some(observer) = sea_entity::stream_observer::Entity::find_by_id(&observer_id)
        .filter(sea_entity::stream_observer::Column::GuildId.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "No stream observer found with that id").await?;
        return Ok(());
    };

    if observer.auto_blacklist_sheet_id.is_none() {
        eph(ctx, "This observer does not have a blacklist sheet set.").await?;
    };

    let mut observer = observer.into_active_model();
    observer.auto_blacklist = Set(vec![]);
    observer.auto_blacklist_sheet_id = Set(None);
    observer.auto_blacklist_column_name = Set(None);
    observer.update(&ctx.data().db.sea).await?;

    ctx.say("Blacklist sheet removed.").await?;

    Ok(())
}
