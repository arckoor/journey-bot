use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, ActivityData, ActivityType, AutocompleteChoice, ChannelId,
        CreateAttachment, CreateAutocompleteResponse, CreateMessage, GuildId, Http, Member,
        Message, Role, RoleId,
    },
};

#[cfg(feature = "reddit-api")]
use roux::util::RouxError;
use sea_orm::{DbErr, SqlErr};
use tracing::{error, info};

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

#[cfg(feature = "reddit-api")]
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

async fn get_guild_tz(store: Arc<Store>, guild_id: GuildId) -> Tz {
    if let Ok(guild_config) =
        get_config_from_id::<sea_entity::guild_config::Entity>(store.clone(), guild_id).await
    {
        guild_config.time_zone.parse().unwrap_or(chrono_tz::UTC)
    } else {
        chrono_tz::UTC
    }
}

pub async fn timestamp_now(store: Arc<Store>, guild_id: GuildId) -> String {
    let tz = get_guild_tz(store, guild_id).await;
    let now = Utc::now().with_timezone(&tz);
    now.format("%H:%M:%S").to_string()
}

fn extract_datetime(ts: f64) -> DateTime<Utc> {
    let secs = ts.trunc() as i64;
    let nanos = (ts.fract() * 1e9) as u32;
    DateTime::from_timestamp(secs, nanos).unwrap()
}

pub async fn timestamp_from_f64_with_tz(ts: f64, store: Arc<Store>, guild_id: GuildId) -> String {
    let datetime = extract_datetime(ts);
    let datetime = datetime.with_timezone(&get_guild_tz(store, guild_id).await);
    datetime.format("%d/%m/%Y %H:%M:%S").to_string()
}

pub fn timestamp_from_f64(ts: f64) -> String {
    let datetime = extract_datetime(ts);
    datetime.format("%d/%m/%Y %H:%M:%S").to_string()
}

pub fn time_to_text(diff: u64) -> String {
    let (days, remainder) = (diff / 86400, diff % 86400);
    let (hours, remainder) = (remainder / 3600, remainder % 3600);
    let minutes = remainder / 60;

    let mut formatted = String::new();
    if days > 0 {
        formatted.push_str(&format!("{} day{}", days, if days > 1 { "s" } else { "" }));
    }
    if hours > 0 {
        if days > 0 {
            formatted.push(' ');
        }
        formatted.push_str(&format!(
            "{} hour{}",
            hours,
            if hours > 1 { "s" } else { "" }
        ));
    }
    if minutes > 0 || !(days > 0 || hours > 0) {
        if days > 0 || hours > 0 {
            formatted.push(' ');
        }
        formatted.push_str(&format!(
            "{} minute{}",
            minutes,
            if minutes != 1 { "s" } else { "" }
        ));
    }

    formatted
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

pub fn create_autocomplete(iter: impl Iterator<Item = String>) -> CreateAutocompleteResponse {
    CreateAutocompleteResponse::new().set_choices(iter.map(AutocompleteChoice::from).collect())
}

pub async fn eph(ctx: Context<'_>, msg: impl Into<String>) -> Result<(), Error> {
    ctx.send(CreateReply::default().content(msg).ephemeral(true))
        .await?;
    Ok(())
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
    let timestamp = timestamp_now(store.clone(), guild_id).await;
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
        .ok_or(BotError::new("Expected a guild channel"))?
        .send_message(&store.ctx, message)
        .await?)
}

pub fn member_is_valid_target(member: &Member) -> bool {
    !member.user.bot
}

pub async fn message_can_be_censored(
    store: Arc<Store>,
    message: &Message,
    guild_id: GuildId,
) -> Result<bool, Error> {
    if message.author.bot {
        return Ok(false);
    }
    let Some(ref member) = message.member else {
        return Ok(false);
    };

    let guild_config =
        get_config_from_id::<sea_entity::guild_config::Entity>(store, guild_id).await?;

    Ok(member
        .roles
        .iter()
        .find(|id| guild_config.trusted_roles.contains(&(id.get() as i64)))
        .is_none())
}

fn filter_roles(
    roles: &[u64],
    guild_roles: &HashMap<RoleId, Role>,
    predicate: &impl Fn(&(RoleId, (u64, String))) -> bool,
) -> (Vec<RoleId>, Vec<(u64, String)>) {
    let (role_ids, role_names) = roles
        .iter()
        .map(|role| {
            guild_roles
                .get(&(*role).into())
                .map(|r| (r.id, (r.id.get(), r.name.clone())))
        })
        .filter(|opt| opt.as_ref().is_some_and(predicate))
        .flatten()
        .unzip::<_, _, Vec<_>, Vec<_>>();

    (role_ids, role_names)
}

pub async fn add_roles_to_member(
    ctx: impl AsRef<Http>,
    roles: &[u64],
    member: &Member,
    guild_roles: &HashMap<RoleId, Role>,
    guild_id: u64,
    dry: bool,
) -> Result<Vec<(u64, String)>, Error> {
    let (role_ids, role_names) =
        filter_roles(roles, guild_roles, &|r: &(RoleId, (u64, String))| {
            !member.roles.contains(&r.0)
        });

    if !role_ids.is_empty() {
        if !dry {
            member.add_roles(&ctx, &role_ids).await?;
            info!(
                "{}",
                format!(
                    "Added roles {} to {} in {}.",
                    role_ids
                        .into_iter()
                        .map(|r| r.get().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    member.user.id,
                    guild_id
                )
            );
        }
        return Ok(role_names);
    }
    Ok(vec![])
}

pub async fn remove_roles_from_member(
    ctx: impl AsRef<Http>,
    roles: &[u64],
    member: &Member,
    guild_roles: &HashMap<RoleId, Role>,
    guild_id: u64,
    dry: bool,
) -> Result<Vec<(u64, String)>, Error> {
    let (role_ids, role_names) =
        filter_roles(roles, guild_roles, &|r: &(RoleId, (u64, String))| {
            member.roles.contains(&r.0)
        });

    if !role_ids.is_empty() {
        if !dry {
            member.remove_roles(&ctx, &role_ids).await?;
            info!(
                "{}",
                format!(
                    "Remove roles {} from {} in {}.",
                    role_ids
                        .into_iter()
                        .map(|r| r.get().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    member.user.id,
                    guild_id
                )
            );
        }
        return Ok(role_names);
    }
    Ok(vec![])
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

pub trait LogError<T, E> {
    #[track_caller]
    fn log(self) -> Result<T, E>;
}

impl<T, E: std::fmt::Display> LogError<T, E> for Result<T, E> {
    #[track_caller]
    fn log(self) -> Result<T, E> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::error!("error at {}: {e}", std::panic::Location::caller());
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::time_to_text;

    #[test]
    fn test_timestamp_conversion() {
        assert_eq!(&time_to_text(0), "0 minutes");
        assert_eq!(&time_to_text(1), "0 minutes");
        assert_eq!(&time_to_text(59), "0 minutes");
        assert_eq!(&time_to_text(60), "1 minute");
        assert_eq!(&time_to_text(61), "1 minute");

        assert_eq!(&time_to_text(3599), "59 minutes");
        assert_eq!(&time_to_text(3600), "1 hour");
        assert_eq!(&time_to_text(3601), "1 hour");
        assert_eq!(&time_to_text(3660), "1 hour 1 minute");
        assert_eq!(&time_to_text(3661), "1 hour 1 minute");

        assert_eq!(&time_to_text(7199), "1 hour 59 minutes");
        assert_eq!(&time_to_text(7200), "2 hours");

        assert_eq!(&time_to_text(86399), "23 hours 59 minutes");
        assert_eq!(&time_to_text(86400), "1 day");
        assert_eq!(&time_to_text(86460), "1 day 1 minute");
        assert_eq!(&time_to_text(90000), "1 day 1 hour");

        assert_eq!(&time_to_text(90060), "1 day 1 hour 1 minute");
        assert_eq!(&time_to_text(172800), "2 days");
        assert_eq!(&time_to_text(176460), "2 days 1 hour 1 minute");
    }
}
