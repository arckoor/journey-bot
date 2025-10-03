use poise::serenity_prelude::ActivityType;
use serde::Deserialize;

fn deserialize_activity<'de, D>(deserializer: D) -> Result<ActivityType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let activity = String::deserialize(deserializer)?;

    let kind = match activity.as_str() {
        "Playing" => ActivityType::Playing,
        "Streaming" => ActivityType::Streaming,
        "Listening" => ActivityType::Listening,
        "Watching" => ActivityType::Watching,
        "Competing" => ActivityType::Competing,
        "Custom" => ActivityType::Custom,
        _ => {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&activity),
                &"a valid activity type",
            ));
        }
    };

    Ok(kind)
}

#[derive(Debug, Deserialize)]
pub struct JourneyBotConfig {
    pub bot: BotConfig,
    pub store: StoreConfig,
}

#[derive(Debug, Deserialize)]
pub struct BotConfig {
    pub bot_token: String,
    pub activity: Option<ActivityConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ActivityConfig {
    #[serde(deserialize_with = "deserialize_activity")]
    pub kind: ActivityType,
    pub message: String,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StoreConfig {
    pub setup: SetupConfig,
    pub emoji: EmojiConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Deserialize)]
pub struct SetupConfig {
    pub postgres_url: String,
    pub admin_guild: u64,
    pub embed_color: (u8, u8, u8),
}

#[derive(Debug, Deserialize)]
pub struct EmojiConfig {
    pub ban: u64,
    pub feed: u64,
    pub info: u64,
    pub join: u64,
    pub react: u64,
    pub sticky: u64,
    pub twitch: u64,
    pub warn: u64,
}

#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub reddit: RedditConfig,
    pub twitch: TwitchConfig,
}

#[derive(Debug, Deserialize)]
pub struct RedditConfig {
    pub id: String,
    pub secret: String,
    pub username: String,
    pub password: String,
    pub user_agent: String,
}

#[derive(Debug, Deserialize)]
pub struct TwitchConfig {
    pub id: String,
    pub secret: String,
    pub filter_words: Vec<String>,
    pub new_threshold: f64,
    pub disappear_threshold: f64,
    pub offline_threshold: f64,
    pub max_concurrent_streams: usize,
}

pub fn config() -> JourneyBotConfig {
    let config = config::Config::builder()
        .add_source(config::File::with_name("config"))
        .build()
        .expect("Failed to load config");

    config
        .try_deserialize::<JourneyBotConfig>()
        .expect("Failed to deserialize config")
}
