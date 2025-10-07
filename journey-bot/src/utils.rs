use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use chrono_tz::Tz;
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, ActivityData, ActivityType, ChannelId, CreateAttachment, CreateMessage,
        GuildId, Message,
    },
};
use roux::util::RouxError;
use sea_orm::{DbErr, SqlErr};
use tracing::error;

use crate::{Context, Error, db::get_config_from_id, emoji::Emoji, store::Store};

#[derive(Debug)]
pub struct BotError {
    msg: String,
}

impl BotError {
    pub fn new<S>(msg: S) -> Self
    where
        S: Into<String> + std::fmt::Display,
    {
        BotError { msg: msg.into() }
    }
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for BotError {}

impl From<DbErr> for BotError {
    #[track_caller]
    fn from(error: DbErr) -> Self {
        if let Some(SqlErr::UniqueConstraintViolation(_)) = error.sql_err() {
            return BotError::new("Record already exists");
        } else if let DbErr::RecordNotFound(_) = error {
            return BotError::new("Record not found");
        }
        error!("{}", format!("DB Error: {error}"));
        BotError::new("Something went wrong while querying the database.")
    }
}

impl From<RouxError> for BotError {
    #[track_caller]
    fn from(value: RouxError) -> Self {
        match value {
            RouxError::Status(response) => error!("{}", format!("roux error: {:?}", response)),
            RouxError::Network(_) | RouxError::Parse(_) => error!("roux error: parsing"),
            RouxError::Auth(_) | RouxError::CredentialsNotSet | RouxError::OAuthClientRequired => {
                error!("roux error: credentials/auth")
            }
        }
        BotError::new("Error while calling roux.")
    }
}

pub fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Getting the time must work")
}

pub fn create_activity(
    kind: ActivityType,
    message: &str,
    url: Option<&str>,
) -> Result<ActivityData, Error> {
    match kind {
        serenity::ActivityType::Playing => Ok(ActivityData::playing(message)),
        serenity::ActivityType::Streaming => Ok(ActivityData::streaming(
            message,
            url.ok_or(BotError::new("Missing URL for streaming activity"))?,
        )?),
        serenity::ActivityType::Listening => Ok(ActivityData::listening(message)),
        serenity::ActivityType::Watching => Ok(ActivityData::watching(message)),
        serenity::ActivityType::Competing => Ok(ActivityData::competing(message)),
        serenity::ActivityType::Custom => Ok(ActivityData::custom(message)),
        _ => Err(BotError::new("Unknown activity!").into()),
    }
}

pub async fn eph(ctx: Context<'_>, msg: impl Into<String>) -> Result<(), Error> {
    ctx.send(CreateReply::default().content(msg).ephemeral(true))
        .await?;
    Ok(())
}

pub fn timestamp_now(tz: Tz) -> String {
    let now = Utc::now().with_timezone(&tz);
    now.format("%H:%M:%S").to_string()
}

pub async fn guild_log(
    store: Arc<Store>,
    guild_id: GuildId,
    category: Emoji,
    msg: impl Into<String> + std::fmt::Display,
    attachment: Option<CreateAttachment>,
) {
    let Ok(guild_config) =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), guild_id).await
    else {
        return;
    };

    let Some(guild_log) = guild_config.guild_log else {
        return;
    };

    log_to(
        store,
        guild_id,
        ChannelId::new(guild_log as u64),
        category,
        msg,
        attachment,
    )
    .await;
}

pub async fn censor_log(
    store: Arc<Store>,
    guild_id: GuildId,
    category: Emoji,
    msg: impl Into<String> + std::fmt::Display,
    attachment: Option<CreateAttachment>,
) {
    let Ok(censor_config) =
        get_config_from_id::<sea_entity::censor_config::Entity>(store.clone(), guild_id).await
    else {
        return;
    };

    let Some(censor_log) = censor_config.log_channel else {
        return;
    };

    log_to(
        store,
        guild_id,
        ChannelId::new(censor_log as u64),
        category,
        msg,
        attachment,
    )
    .await;
}

async fn log_to(
    store: Arc<Store>,
    guild_id: GuildId,
    channel_id: ChannelId,
    category: Emoji,
    msg: impl Into<String> + std::fmt::Display,
    attachment: Option<CreateAttachment>,
) {
    let Ok(guild_config) =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), guild_id).await
    else {
        return;
    };

    let tz = guild_config.time_zone.parse().unwrap_or(chrono_tz::UTC);
    let timestamp = timestamp_now(tz);
    let message = format!("[`{timestamp}`]  {} {msg}", store.emoji.get(category));

    let _ = send_message(store, channel_id, message, attachment).await;
}

pub async fn send_message(
    store: Arc<Store>,
    channel_id: ChannelId,
    msg: impl Into<String> + std::fmt::Display,
    attachment: Option<CreateAttachment>,
) -> Result<Message, Error> {
    let channel = store.ctx.get_channel(channel_id).await?;

    let mut message = CreateMessage::new().content(msg);
    if let Some(attachment) = attachment {
        message = message.add_file(attachment);
    }

    Ok(channel
        .guild()
        .expect("Expected a guild channel")
        .send_message(&store.ctx, message)
        .await?)
}

pub async fn fetch_sheet(id: &str) -> Result<csv::Reader<Cursor<Vec<u8>>>, BotError> {
    let url = format!("https://docs.google.com/spreadsheets/d/{id}/export?format=csv");
    let resp = reqwest::get(url)
        .await
        .map_err(|_| BotError::new("Failed to fetch google sheet"))?
        .error_for_status()
        .map_err(|_| BotError::new("Failed to fetch google sheet"))?
        .text()
        .await
        .map_err(|_| BotError::new("Failed to serialize sheet to text"))?;

    let cursor = Cursor::new(resp.as_bytes().to_vec());
    let reader = csv::Reader::from_reader(cursor);

    Ok(reader)
}

pub async fn fetch_sheet_columns(
    mut reader: csv::Reader<Cursor<Vec<u8>>>,
    column_names: &[&String],
) -> Result<HashMap<String, Vec<String>>, BotError> {
    let headers = reader
        .headers()
        .map_err(|_| BotError::new("Failed to read headers"))?;

    let mut indices = HashMap::new();
    for &col in column_names {
        if let Some(idx) = headers.iter().position(|h| h == col) {
            indices.insert(col.to_string(), idx);
        } else {
            return Err(BotError::new(format!("Column {col} not found")));
        }
    }

    let mut data: HashMap<String, Vec<String>> = column_names
        .iter()
        .map(|&c| (c.to_string(), Vec::new()))
        .collect();

    for result in reader.records() {
        let record = result.map_err(|_| BotError::new("Failed to parse record"))?;
        for (col, &idx) in &indices {
            if let Some(val) = record.get(idx)
                && !val.is_empty()
            {
                data.get_mut(col).unwrap().push(val.to_string());
            }
        }
    }

    Ok(data)
}

pub fn schedule_at_interval<F, Fut>(store: Arc<Store>, interval: Duration, f: F)
where
    F: Fn(Arc<Store>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            f(store.clone()).await;
            interval.tick().await;
        }
    });
}

pub trait LogError<T> {
    fn log(self, from: &str);
}

impl<T, E: std::fmt::Display> LogError<T> for Result<T, E> {
    fn log(self, from: &str) {
        if let Err(err) = self {
            tracing::error!("error in {from}: {err}")
        }
    }
}
