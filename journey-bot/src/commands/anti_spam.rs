use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    iter::zip,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use poise::{
    ChoiceParameter, CreateReply,
    serenity_prelude::{
        ChannelId, CreateAttachment, EditMember, GuildId, Mentionable, Message, MessageId, UserId,
    },
};
use regex::Regex;
use sea_entity::sea_orm_active_enums::Punishment;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
};
use tokio::sync::mpsc::Receiver;
use tracing::{info, warn};
use unicode_normalization::UnicodeNormalization;

use crate::{
    Context, Error,
    db::get_config_from_id,
    emoji::Emoji,
    store::Store,
    utils::{
        BotError, LogError, censor_log, eph, guild_log, now, schedule_at_interval, send_message,
        timestamp_now,
    },
    views::embed::default_embed,
};

const RECENTLY_PUNISHED_LIFETIME: u64 = 60 * 60 * 24 * 7;

pub enum ChannelMessage {
    NewMessage(NewMessage),
    UpdateConfig(u64, bool),
    CleanMessages,
    PrintPool(GuildId, ChannelId),
}

pub struct NewMessage {
    id: u64,
    content: String,
    guild_id: u64,
    channel_id: u64,
    author_name: String,
    author_id: u64,
    timestamp: f64,
}

#[derive(Clone)]
struct BucketMessage {
    content: String,
    message_id: u64,
    channel_id: u64,
    timestamp: f64,
}

#[derive(Clone)]
struct Bucket {
    messages: VecDeque<BucketMessage>,
    last_score: f64,
}

impl Bucket {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            last_score: 0.0,
        }
    }

    fn add_message(&mut self, message: BucketMessage, score: f64, max_size: usize) {
        self.last_score = score;
        self.messages.push_back(message);
        while self.messages.len() > max_size {
            self.messages.pop_front();
        }
    }

    fn remove_old_messages(&mut self, time_frame: u32) {
        let now = now().as_secs_f64();
        self.messages
            .retain(|m| ((now - m.timestamp) as u32) < time_frame);
    }

    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Display for Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "  Bucket ({:.5}): \n{}",
            self.last_score,
            self.messages
                .iter()
                .map(|m| {
                    let secs = m.timestamp.trunc() as i64;
                    let nanos = (m.timestamp.fract() * 1e9) as u32;
                    let datetime = DateTime::from_timestamp(secs, nanos).unwrap();
                    let timestamp = datetime.format("%d/%m/%Y %H:%M:%S").to_string();
                    format!("    {} | {} | {}", m.message_id, timestamp, m.content)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

struct Pool {
    guild_id: u64,
    buckets: HashMap<u64, Vec<Bucket>>,
    config: sea_entity::anti_spam_config::Model,
    store: Arc<Store>,
}

impl Pool {
    pub async fn new(store: Arc<Store>, guild_id: u64) -> Result<Self, BotError> {
        let config = Self::get_config(&store, guild_id).await?;
        Ok(Self {
            guild_id,
            buckets: HashMap::new(),
            config,
            store,
        })
    }

    pub async fn update_config(&mut self) -> Result<(), BotError> {
        self.config = Self::get_config(&self.store, self.guild_id).await?;
        Ok(())
    }

    async fn get_config(
        store: &Arc<Store>,
        guild_id: u64,
    ) -> Result<sea_entity::anti_spam_config::Model, BotError> {
        sea_entity::anti_spam_config::Entity::find_by_id(guild_id as i64)
            .one(&store.db.sea)
            .await?
            .ok_or(BotError::new(format!(
                "Anti-Spam config for guild {} not found",
                guild_id
            )))
    }

    pub async fn punish_message(&mut self, message: NewMessage) {
        let (is_spam, bucket, confidence) = self.add_message(&message).await;

        if !is_spam {
            return;
        }
        info!(
            "Detected spam by user {} ({}) with confidence {:.5}",
            message.author_name, message.author_id, confidence
        );

        let channel_id = ChannelId::new(message.channel_id);
        let guild_id = GuildId::new(self.config.id as u64);
        let user_id = UserId::new(message.author_id);

        let channel_name = channel_id
            .name(&self.store.ctx)
            .await
            .unwrap_or("Unknown".to_string());

        let audit_log_reason = format!(
            "Spam detected in #{}. Confidence {:.5}",
            channel_name, confidence
        );

        let success = match self.config.punishment {
            Punishment::Mute => {
                let Ok(guild) = guild_id.to_partial_guild(&self.store.ctx).await else {
                    return;
                };

                let Ok(mut member) = guild.member(&self.store.ctx, user_id).await else {
                    return;
                };

                let timeout_duration = Utc::now()
                    .checked_add_signed(TimeDelta::minutes(self.config.timeout_duration as i64))
                    .expect("Failed to compute timeout duration");

                member
                    .edit(
                        &self.store.ctx,
                        EditMember::default()
                            .disable_communication_until_datetime(timeout_duration.into())
                            .audit_log_reason(&audit_log_reason),
                    )
                    .await
                    .is_ok()
            }
            Punishment::Ban => self
                .store
                .ctx
                .ban_user(guild_id, user_id, 0, Some(&audit_log_reason))
                .await
                .is_ok(),
        };

        let mut report = None;
        if let Some(ref bucket) = bucket {
            report = Some(self.build_report(bucket).await);
        }

        let (success_p, fail_p) = if self.config.punishment == Punishment::Mute {
            ("muted", "mute")
        } else {
            ("banned", "ban")
        };
        if success {
            let success_msg = format!(
                "{} (`{}`) has been {} for spam in {}. Confidence: {}",
                message.author_name,
                message.author_id,
                success_p,
                channel_id.mention(),
                format!("{:.3}", confidence)
                    .trim_end_matches("0")
                    .trim_end_matches(".")
            );
            guild_log(
                self.store.clone(),
                guild_id,
                Emoji::Ban,
                &success_msg,
                report.clone(),
            )
            .await;
            censor_log(
                self.store.clone(),
                guild_id,
                Emoji::Ban,
                &success_msg,
                report,
            )
            .await;
        } else {
            guild_log(
                self.store.clone(),
                guild_id,
                Emoji::Warning,
                format!(
                    "I cannot {} {} (`{}`)",
                    fail_p, message.author_name, message.author_id,
                ),
                None,
            )
            .await;
        }

        if self.config.clean_user {
            self.delete_user_messages(message.author_id).await;
        }
    }

    async fn add_message(&mut self, message: &NewMessage) -> (bool, Option<Bucket>, f64) {
        let buckets = self.buckets.entry(message.author_id).or_default();
        let min_threshold = self
            .config
            .similar_message_threshold
            .clone()
            .into_iter()
            .reduce(f64::min)
            .unwrap_or(0.);

        let (bucket_idx, confidence) = Self::find_closest_bucket(
            message,
            buckets,
            min_threshold,
            self.config.time_frame as u32,
        );
        let bucket = &mut buckets[bucket_idx];
        bucket.add_message(
            BucketMessage {
                content: message.content.clone(),
                message_id: message.id,
                channel_id: message.channel_id,
                timestamp: message.timestamp,
            },
            confidence,
            *self.config.max_messages.iter().max().unwrap() as usize,
        );
        let bucket = bucket.clone();
        let (is_recently_punished, recently_punished_confidence) =
            self.is_recently_punished(&message.content).await;
        if is_recently_punished {
            self.add_recently_punished(&message.content).await;
            return (true, Some(bucket.clone()), recently_punished_confidence);
        }
        for (c_level, max_size) in zip(
            &self.config.similar_message_threshold,
            &self.config.max_messages,
        ) {
            if confidence > *c_level && bucket.messages.len() >= *max_size as usize {
                self.add_recently_punished(&message.content).await;
                return (true, Some(bucket), confidence);
            }
        }
        (false, None, 0.0)
    }

    fn find_closest_bucket(
        message: &NewMessage,
        buckets: &mut Vec<Bucket>,
        min_threshold: f64,
        time_frame: u32,
    ) -> (usize, f64) {
        let mut max_avg = 0.0;
        let mut closest_bucket_idx = None;

        for (idx, bucket) in buckets.iter_mut().enumerate() {
            bucket.remove_old_messages(time_frame);
            let mut avg = 0.0;
            for msg in bucket.messages.iter() {
                avg += rapidfuzz::distance::jaro_winkler::similarity(
                    msg.content.chars(),
                    message.content.chars(),
                );
            }
            avg /= bucket.messages.len() as f64;
            if avg > max_avg {
                max_avg = avg;
                if avg > min_threshold {
                    closest_bucket_idx = Some(idx);
                }
            }
        }

        if let Some(idx) = closest_bucket_idx {
            (idx, max_avg)
        } else {
            info!(
                "Created a new bucket for user {} in {}",
                message.author_id, message.guild_id
            );
            buckets.push(Bucket::new());
            (buckets.len() - 1, 0.0)
        }
    }

    async fn is_recently_punished(&self, content: &str) -> (bool, f64) {
        let Ok(recently_punished) = sea_entity::punished_message::Entity::find()
            .filter(sea_entity::punished_message::Column::AntiSpamConfigId.eq(self.config.id))
            .all(&self.store.db.sea)
            .await
        else {
            return (false, 0.0);
        };

        let mut max_avg = 0.0;
        for msg in recently_punished.into_iter() {
            let avg =
                rapidfuzz::distance::jaro_winkler::similarity(msg.content.chars(), content.chars());
            if avg > max_avg {
                max_avg = avg;
            }
        }
        (
            max_avg > self.config.similar_message_re_punish_threshold,
            max_avg,
        )
    }

    async fn add_recently_punished(&self, content: &str) {
        let Ok(msg) = sea_entity::punished_message::Entity::find()
            .filter(sea_entity::punished_message::Column::Content.eq(content))
            .one(&self.store.db.sea)
            .await
        else {
            warn!("Could not retrieve recently punished messages");
            return;
        };

        if let Some(msg) = msg {
            let mut msg = msg.into_active_model();
            msg.timestamp = Set(now().as_secs_f64());
            if msg.update(&self.store.db.sea).await.is_err() {
                warn!("Could not update recently punished message");
            }
        } else if let Ok(msg) = (sea_entity::punished_message::ActiveModel {
            content: Set(content.to_string()),
            timestamp: Set(now().as_secs_f64()),
            anti_spam_config_id: Set(self.config.id),
            ..Default::default()
        }
        .insert(&self.store.db.sea)
        .await)
        {
            info!(
                "Added recently punished message {} for guild {}",
                msg.id, self.guild_id
            );
        } else {
            warn!("Could not add new punished message");
        }
    }

    async fn delete_user_messages(&mut self, user_id: u64) {
        let Some(buckets) = self.buckets.remove(&user_id) else {
            return;
        };
        let mut messages = HashMap::new();
        for bucket in buckets {
            for message in bucket.messages {
                messages
                    .entry(ChannelId::new(message.channel_id))
                    .or_insert(Vec::new())
                    .push(MessageId::new(message.message_id));
            }
        }
        for (channel_id, messages) in messages.drain() {
            if messages.len() == 1 {
                let message = messages.into_iter().next().unwrap();
                let _ = self
                    .store
                    .ctx
                    .delete_message(channel_id, message, None)
                    .await;
            } else {
                let _ = self
                    .store
                    .ctx
                    .delete_messages(
                        channel_id,
                        &serde_json::to_value(HashMap::from([("messages", messages)])).unwrap(),
                        None,
                    )
                    .await;
            }
        }
    }

    async fn build_report(&self, bucket: &Bucket) -> CreateAttachment {
        let timestamp = timestamp_now(chrono_tz::UTC);
        let mut out = format!("recorded spam message at {timestamp}\n");

        for message in &bucket.messages {
            let channel_id = ChannelId::new(message.channel_id);
            let message_id = MessageId::new(message.message_id);
            let Ok(msg) = self.store.ctx.get_message(channel_id, message_id).await else {
                continue;
            };
            let content = if msg.content.is_empty() {
                "(no content)".to_string()
            } else {
                msg.content
            };
            let reply = msg
                .referenced_message
                .map(|m| {
                    format!(
                        " | In reply to https://discord.com/channels/{}/{}/{}",
                        self.guild_id,
                        m.channel_id.get(),
                        m.id.get()
                    )
                })
                .unwrap_or("".to_string());
            let attachments = if msg.attachments.is_empty() {
                "".to_string()
            } else {
                format!(
                    " | Attachments: {}",
                    msg.attachments
                        .into_iter()
                        .map(|a| a.url)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let timestamp = msg.id.created_at().format("%H:%M:%S").to_string();
            out.push_str(&format!(
                "{timestamp} {} - {} - {} | {} ({}) | {content}{reply}{attachments}\n",
                self.guild_id,
                msg.channel_id.get(),
                msg.id.get(),
                msg.author.name,
                msg.author.id
            ));
        }

        CreateAttachment::bytes(out, "archive.txt")
    }

    pub fn cleanup(&mut self) {
        for (user, buckets) in self.buckets.iter_mut() {
            for bucket in buckets.iter_mut() {
                bucket.remove_old_messages(self.config.time_frame as u32);
            }
            let len_before = buckets.len();
            buckets.retain(|b| !b.is_empty());
            if buckets.len() != len_before {
                info!(
                    "Empty buckets were removed for user {} in {}",
                    user, self.guild_id
                );
            }
        }
    }
}

impl Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.buckets
                .iter()
                .map(|(user_id, buckets)| {
                    format!(
                        "User {user_id}:\n{}",
                        buckets
                            .iter()
                            .map(|b| b.to_string())
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

pub struct PoolManager {
    pools: HashMap<u64, Pool>,
    store: Arc<Store>,
}

impl PoolManager {
    fn new(store: Arc<Store>) -> Self {
        Self {
            pools: HashMap::new(),
            store,
        }
    }

    pub fn schedule(store: Arc<Store>, rx: Receiver<ChannelMessage>) {
        tokio::spawn(async move {
            let pool_manager = PoolManager::new(store);
            pool_manager.schedule_tasks(rx);
        });
    }

    fn schedule_tasks(self, rx: Receiver<ChannelMessage>) {
        schedule_at_interval(
            self.store.clone(),
            Duration::from_secs(60 * 60 * 4),
            Self::expire_old_punished_messages,
        );
        schedule_at_interval(
            self.store.clone(),
            Duration::from_secs(60 * 5),
            Self::send_cleanup_message,
        );
        tokio::spawn(async move {
            self.receive(rx).await;
        });
    }

    pub async fn receive(mut self, mut rx: Receiver<ChannelMessage>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                ChannelMessage::NewMessage(msg) => {
                    if !self.pools.contains_key(&msg.guild_id) {
                        if let Ok(pool) = Pool::new(self.store.clone(), msg.guild_id).await {
                            self.pools.insert(msg.guild_id, pool);
                        } else {
                            warn!("Failed to create pool for guild {}", msg.guild_id);
                        }
                    }
                    if let Some(pool) = self.pools.get_mut(&msg.guild_id) {
                        pool.punish_message(msg).await;
                    } else {
                        warn!("Failed to find pool for guild {}", msg.guild_id);
                    };
                }
                ChannelMessage::UpdateConfig(guild_id, disable) => {
                    if disable {
                        self.pools.remove(&guild_id);
                    } else if let Some(pool) = self.pools.get_mut(&guild_id)
                        && pool.update_config().await.is_err()
                    {
                        warn!("Failed to update pool config for guild {}", guild_id);
                    }
                }
                ChannelMessage::CleanMessages => {
                    for pool in self.pools.values_mut() {
                        pool.cleanup();
                    }
                }
                ChannelMessage::PrintPool(guild_id, channel_id) => {
                    if let Some(pool) = self.pools.get(&guild_id.get()) {
                        let _ = send_message(
                            self.store.clone(),
                            channel_id,
                            "",
                            Some(CreateAttachment::bytes(pool.to_string(), "pool.txt")),
                        )
                        .await;
                    } else {
                        let _ =
                            send_message(self.store.clone(), channel_id, "Pool is empty.", None)
                                .await;
                    }
                }
            }
        }
        warn!("Channel closed unexpectedly");
    }

    async fn expire_old_punished_messages(store: Arc<Store>) {
        if let Ok(punished_messages) = sea_entity::punished_message::Entity::find()
            .all(&store.db.sea)
            .await
        {
            let now = now().as_secs_f64();
            for msg in punished_messages {
                if (now - msg.timestamp) as u64 > RECENTLY_PUNISHED_LIFETIME {
                    let id = msg.id;
                    let guild_id = msg.anti_spam_config_id;
                    if msg.into_active_model().delete(&store.db.sea).await.is_err() {
                        warn!(
                            "Failed to remove punished message {}, for guild {}",
                            id, guild_id
                        );
                    } else {
                        info!("Removed punished message {} for guild {}", id, guild_id);
                    }
                }
            }
        }
    }

    async fn send_cleanup_message(store: Arc<Store>) {
        if store
            .anti_spam_sender
            .send(ChannelMessage::CleanMessages)
            .await
            .is_err()
        {
            warn!("Failed to send cleanup message");
        }
    }
}

#[derive(ChoiceParameter, Clone)]
enum PunishmentChoice {
    Mute,
    Ban,
}

impl From<PunishmentChoice> for Punishment {
    fn from(value: PunishmentChoice) -> Self {
        match value {
            PunishmentChoice::Mute => Punishment::Mute,
            PunishmentChoice::Ban => Punishment::Ban,
        }
    }
}

impl From<Punishment> for PunishmentChoice {
    fn from(value: Punishment) -> Self {
        match value {
            Punishment::Mute => PunishmentChoice::Mute,
            Punishment::Ban => PunishmentChoice::Ban,
        }
    }
}

#[poise::command(
    slash_command,
    subcommands("help", "show", "punishment", "disable", "punished_messages", "pool"),
    guild_only,
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "
        ATTACH_FILES |
        BAN_MEMBERS |
        MANAGE_MESSAGES |
        MODERATE_MEMBERS |
        READ_MESSAGE_HISTORY |
        SEND_MESSAGES |
        VIEW_CHANNEL
    ",
    rename = "anti-spam-config"
)]
pub async fn as_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show the help for the anti-spam module.
#[poise::command(slash_command)]
async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let fields = [
        (
            "max-spam-messages",
            "The number of similar messages to trigger a violation. Multiple comma-separated values are allowed. For example `6, 4, 2`.",
        ),
        (
            "similarity-threshold",
            "The threshold for when a message is considered similar. Multiple comma-separated values are allowed. For example. `.9, .95, .99`.",
        ),
        (
            "Correlation between max-spam-messages and similarity-threshold",
            "The first value of max-spam-messages corresponds to the first value of similarity-threshold, the second to the second, etc. - In the example above, a user would get punished for 6 messages that are .9 similar, but already for 4 if they are .95 similar and so on.",
        ),
        (
            "similarity-re-ban-threshold",
            "All recently punished messages are stored. If a message is similar to a recently punished message with a similarity higher than this threshold, the user is immediately punished. Useful for users that join with new accounts to spam again. Set > 1 to disable this behaviour.",
        ),
    ];

    let embed = default_embed(ctx)
        .title("Anti-Spam Help")
        .description("Help for the anti-spam module.")
        .fields(fields.into_iter().map(|(n, v)| (n, v, false)));

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show the config of the anti-spam module.
#[poise::command(slash_command)]
async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let Some(as_config) = sea_entity::anti_spam_config::Entity::find()
        .filter(sea_entity::anti_spam_config::Column::Id.eq(guild_id))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "Anti-Spam is not enabled for this guild.").await?;
        return Ok(());
    };

    let re_punish = if as_config.similar_message_re_punish_threshold > 1.0 {
        "Disabled".to_string()
    } else {
        as_config.similar_message_re_punish_threshold.to_string()
    };

    let punishment: PunishmentChoice = as_config.punishment.into();

    let mut embed = default_embed(ctx)
        .title("Anti-Spam config")
        .description("Config of the anti-spam module.")
        .fields(
            [
                ("Punishment", &punishment.name().to_string()),
                (
                    "Max spam messages",
                    &as_config
                        .max_messages
                        .into_iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                (
                    "Similarity threshold(s)",
                    &as_config
                        .similar_message_threshold
                        .into_iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                ("Time frame", &as_config.time_frame.to_string()),
                ("Similarity re-ban threshold", &re_punish),
                ("Clean user messages", &as_config.clean_user.to_string()),
            ]
            .into_iter()
            .map(|(n, v)| (n, v, true)),
        );

    if as_config.punishment == Punishment::Mute {
        embed = embed.field(
            "Mute duration",
            as_config.timeout_duration.to_string(),
            true,
        );
    }

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Configure the punishment for the anti-spam module.
#[poise::command(slash_command)]
#[allow(clippy::too_many_arguments)]
async fn punishment(
    ctx: Context<'_>,
    #[description = "The punishment for a violation."] punishment: PunishmentChoice,
    #[description = "The amount of time to mute users, in minutes"]
    #[min = 1]
    #[max = 40320] // 60 * 24 * 28
    #[rename = "mute-duration"]
    timeout_duration: u32,
    #[description = "The number of similar messages to trigger a violation. Multiple comma-separated values are allowed."]
    #[rename = "max-spam-messages"]
    max_spam_messages: String,
    #[description = "The threshold for when a message is considered similar. Multiple comma-separated values are allowed."]
    #[rename = "similarity-threshold"]
    similarity_threshold: String,
    #[description = "The threshold for a message to lead to an immediate re-punish. Set > 1 to disable."]
    #[min = 0.0]
    #[max = 2.0]
    #[rename = "similarity-re-punish-threshold"]
    sim_re_punish_threshold: f64,
    #[description = "For how long a message is taken into account (in seconds)."]
    #[rename = "time-frame"]
    time_frame: u32,
    #[description = "Clean the user's messages after punishment."]
    #[rename = "clean-user"]
    clean_user: bool,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let re = Regex::new("(, )+").unwrap();

    let Ok(max_spam_messages) = re
        .split(&max_spam_messages)
        .map(|s| s.parse::<u32>())
        .collect::<Result<Vec<_>, _>>()
    else {
        eph(
            ctx,
            "Could not parse `max-spam-messages`. Enter it in the format 'x, y, z'.",
        )
        .await?;
        return Ok(());
    };

    let Ok(similarity_threshold) = re
        .split(&similarity_threshold)
        .map(|s| s.parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
    else {
        eph(
            ctx,
            "Could not parse `similarity-threshold`. Enter it in the format 'x, y, z'.",
        )
        .await?;
        return Ok(());
    };

    if max_spam_messages.len() != similarity_threshold.len() {
        eph(
            ctx,
            "The number of max-spam-messages and similarity-thresholds must be the same.",
        )
        .await?;
        return Ok(());
    }

    if max_spam_messages.is_empty() {
        eph(ctx, "You must submit at least one max-spam-message threshold and at least one similarity-threshold.").await?;
        return Ok(());
    }

    if max_spam_messages.iter().any(|m| *m < 2) {
        eph(
            ctx,
            "Each value of max-spam-messages must be greater than 1.",
        )
        .await?;
    }

    if similarity_threshold.iter().any(|s| *s < 0.0 || *s > 1.0) {
        eph(
            ctx,
            "Each value of similarity-threshold must be between 0 and 1.",
        )
        .await?;
        return Ok(());
    }

    let max_spam_messages = max_spam_messages
        .into_iter()
        .map(|v| v as i32)
        .collect::<Vec<_>>();

    if let Some(as_config) = sea_entity::anti_spam_config::Entity::find_by_id(guild_id.get() as i64)
        .one(&ctx.data().db.sea)
        .await?
    {
        let mut as_config = as_config.into_active_model();
        as_config.punishment = Set(punishment.clone().into());
        as_config.timeout_duration = Set(timeout_duration as i32);
        as_config.clean_user = Set(clean_user);
        as_config.max_messages = Set(max_spam_messages);
        as_config.similar_message_threshold = Set(similarity_threshold);
        as_config.similar_message_re_punish_threshold = Set(sim_re_punish_threshold);
        as_config.time_frame = Set(time_frame as i32);
        as_config.update(&ctx.data().db.sea).await?;
    } else {
        sea_entity::anti_spam_config::ActiveModel {
            id: Set(guild_id.get() as i64),
            punishment: Set(punishment.clone().into()),
            timeout_duration: Set(timeout_duration as i32),
            clean_user: Set(clean_user),
            max_messages: Set(max_spam_messages),
            similar_message_threshold: Set(similarity_threshold),
            similar_message_re_punish_threshold: Set(sim_re_punish_threshold),
            time_frame: Set(time_frame as i32),
        }
        .insert(&ctx.data().db.sea)
        .await?;
    }

    trigger_update(ctx, guild_id, false).await;

    ctx.say(format!(
        "Punishment set to {}.",
        punishment.name().to_lowercase()
    ))
    .await?;

    guild_log(
        ctx.data().clone(),
        guild_id,
        Emoji::Info,
        format!(
            "The anti-spam config was updated by {} (`{}`).",
            ctx.author().name,
            ctx.author().id
        ),
        None,
    )
    .await;

    Ok(())
}

/// Disable the anti-spam module for this guild.
#[poise::command(slash_command)]
async fn disable(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    let Some(as_config) = sea_entity::anti_spam_config::Entity::find()
        .filter(sea_entity::anti_spam_config::Column::Id.eq(guild_id.get() as i64))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "Anti-Spam is not enabled for this guild.").await?;
        return Ok(());
    };

    as_config
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    trigger_update(ctx, guild_id, true).await;

    ctx.say("Anti-Spam disabled.").await?;

    Ok(())
}

#[poise::command(
    slash_command,
    subcommands("pm_show", "pm_add", "pm_remove"),
    rename = "punished-messages"
)]
async fn punished_messages(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show the recently punished messages.
#[poise::command(slash_command, rename = "show")]
async fn pm_show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let Some((_, punished_messages)) = sea_entity::anti_spam_config::Entity::find()
        .filter(sea_entity::anti_spam_config::Column::Id.eq(guild_id))
        .find_with_related(sea_entity::punished_message::Entity)
        .all(&ctx.data().db.sea)
        .await?
        .into_iter()
        .next()
    else {
        eph(ctx, "Anti-Spam is not enabled for this guild.").await?;
        return Ok(());
    };

    if punished_messages.is_empty() {
        eph(ctx, "No punished messages found").await?;
        return Ok(());
    }

    let now = now().as_secs_f64();
    let embed = default_embed(ctx)
        .title("Recently punished messages")
        .fields(punished_messages.into_iter().map(|m| {
            (
                format!(
                    "ID: {} | Remaining lifespan: {}",
                    m.id,
                    time_to_text(RECENTLY_PUNISHED_LIFETIME - (now - m.timestamp) as u64)
                ),
                m.content,
                false,
            )
        }));

    ctx.send(CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Add a punished message.
#[poise::command(slash_command, rename = "add")]
async fn pm_add(
    ctx: Context<'_>,
    #[description = "The message to add as a punished message."] content: String,
) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    let Some(as_config) = sea_entity::anti_spam_config::Entity::find()
        .filter(sea_entity::anti_spam_config::Column::Id.eq(guild_id))
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "Anti-Spam is not enabled for this guild.").await?;
        return Ok(());
    };
    let content = preprocess_content(&content)?;
    let now = now().as_secs_f64();

    if let Some(punished_message) = sea_entity::punished_message::Entity::find()
        .filter(
            sea_orm::Condition::all()
                .add(sea_entity::punished_message::Column::Id.eq(guild_id))
                .add(sea_entity::punished_message::Column::Content.eq(&content)),
        )
        .one(&ctx.data().db.sea)
        .await?
    {
        let mut punished_message = punished_message.into_active_model();
        punished_message.timestamp = Set(now);
        punished_message.update(&ctx.data().db.sea).await?;
        ctx.say("Punished message updated.").await?;
    } else {
        sea_entity::punished_message::ActiveModel {
            content: Set(content),
            timestamp: Set(now),
            anti_spam_config_id: Set(as_config.id),
            ..Default::default()
        }
        .insert(&ctx.data().db.sea)
        .await?;

        ctx.say("Punished message added.").await?;
    }

    Ok(())
}

/// Remove a punished message.
#[poise::command(slash_command, rename = "remove")]
async fn pm_remove(
    ctx: Context<'_>,
    #[description = "The ID of the punished message to remove."] id: u32,
) -> Result<(), Error> {
    let guild_id: u64 = ctx.guild_id().ok_or("Expected to be in a guild")?.into();

    if sea_entity::anti_spam_config::Entity::find()
        .filter(sea_entity::anti_spam_config::Column::Id.eq(guild_id))
        .one(&ctx.data().db.sea)
        .await?
        .is_none()
    {
        eph(ctx, "Anti-Spam is not enabled for this guild.").await?;
        return Ok(());
    };

    let Some(punished_message) = sea_entity::punished_message::Entity::find_by_id(id as i32)
        .one(&ctx.data().db.sea)
        .await?
    else {
        eph(ctx, "Punished message not found.").await?;
        return Ok(());
    };

    punished_message
        .into_active_model()
        .delete(&ctx.data().db.sea)
        .await?;

    ctx.say("Punished message removed.").await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn pool(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Expected to be in a guild")?;

    ctx.data()
        .anti_spam_sender
        .send(ChannelMessage::PrintPool(guild_id, ctx.channel_id()))
        .await
        .map_err(|_| BotError::new("Unable to generate pool report."))?;

    eph(
        ctx,
        "Working on generating your report, this may take a moment...",
    )
    .await?;

    Ok(())
}

fn time_to_text(diff: u64) -> String {
    let (days, remainder) = (diff / 86400, diff % 86400);
    let (hours, remainder) = (remainder / 3600, remainder % 3600);
    let minutes = remainder % 60;

    let mut formatted = String::new();
    if days > 0 {
        formatted.push_str(&format!("{} day{}", days, if days > 1 { "s" } else { "" }));
    }
    if hours > 0 {
        formatted.push_str(&format!(
            "{} hour{}",
            hours,
            if hours > 1 { "s" } else { "" }
        ));
    }
    if minutes > 0 || !(days > 0 || hours > 1) {
        formatted.push_str(&format!(
            "{} minute{}",
            minutes,
            if minutes > 1 { "s" } else { "" }
        ));
    }

    formatted
}

fn preprocess_content(content: &str) -> Result<String, BotError> {
    let cursive_start = '𝘈' as u32;

    let mut replacements = HashMap::new();
    for (i, c) in ('A'..='Z').enumerate() {
        replacements.insert(std::char::from_u32(cursive_start + i as u32).unwrap(), c);
    }
    for (i, c) in ('a'..='z').enumerate() {
        replacements.insert(
            std::char::from_u32(cursive_start + 26 + i as u32).unwrap(),
            c,
        );
    }

    let mut msg: String = content
        .to_lowercase()
        .replace('\n', " ")
        .chars()
        .map(|ch| replacements.get(&ch).cloned().unwrap_or(ch))
        .collect();

    msg = msg.nfkd().filter(|c| c.is_ascii()).collect::<String>();

    msg.retain(|c| !c.is_ascii_punctuation() && !c.is_control());

    let mut chars: Vec<char> = msg.chars().collect();
    chars.reverse();
    while let Some(c) = chars.last() {
        if c.is_ascii_digit() {
            chars.pop();
        } else {
            break;
        }
    }
    chars.reverse();
    msg = chars.into_iter().collect::<String>();

    let msg = msg.trim().to_string();

    if msg.is_empty() {
        Err(BotError::new("Message must not contain only punctuation."))
    } else {
        Ok(msg)
    }
}

async fn trigger_update(ctx: Context<'_>, guild_id: GuildId, disable: bool) {
    if ctx
        .data()
        .anti_spam_sender
        .send(ChannelMessage::UpdateConfig(guild_id.get(), disable))
        .await
        .is_err()
    {
        warn!(
            "Failed to send pool update message for guild {}",
            guild_id.get()
        );
    }
}

pub async fn on_message(store: Arc<Store>, message: &Message) -> Result<(), Error> {
    if message.author.bot {
        return Ok(());
    }
    let Some(guild_id) = message.guild_id else {
        return Ok(());
    };
    let Some(ref member) = message.member else {
        return Ok(());
    };

    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), guild_id).await?;

    if !member
        .roles
        .iter()
        .filter(|id| guild_config.trusted_roles.contains(&(id.get() as i64)))
        .collect::<Vec<_>>()
        .is_empty()
    {
        return Ok(());
    }

    let now = now().as_secs_f64();
    store
        .anti_spam_sender
        .send(ChannelMessage::NewMessage(NewMessage {
            id: message.id.get(),
            content: preprocess_content(&message.content)?,
            guild_id: guild_id.get(),
            channel_id: message.channel_id.get(),
            author_id: message.author.id.get(),
            author_name: message.author.name.clone(),
            timestamp: now,
        }))
        .await
        .log("anti_spam::on_message::send");

    Ok(())
}
