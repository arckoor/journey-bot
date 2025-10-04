use std::{collections::HashMap, path::PathBuf};

use binary_heap_plus::BinaryHeap;
use poise::serenity_prelude::futures::{self, Stream};
use regex::Regex;
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::{
    Context, Error,
    utils::{BotError, eph},
    views::paginator::paginate,
};

type LinkData = HashMap<String, String>;

pub struct Links {
    map: RwLock<HashMap<String, Vec<String>>>,
    rev_map: RwLock<LinkData>,
    path: PathBuf,
}

impl Links {
    pub async fn new() -> Self {
        let path = PathBuf::from("./links.json");
        let data = tokio::fs::read_to_string(&path)
            .await
            .expect("Failed to read links file");
        let map = serde_json::from_str(&data).expect("Failed to deserialize links file");

        let reverse_map = Self::map_to_reverse_map(&map);
        Self {
            map: RwLock::new(map),
            rev_map: RwLock::new(reverse_map),
            path,
        }
    }

    pub async fn get(&'_ self) -> Option<RwLockReadGuard<'_, LinkData>> {
        self.rev_map.try_read().ok()
    }

    pub async fn get_blocking(&'_ self) -> RwLockReadGuard<'_, LinkData> {
        self.rev_map.read().await
    }

    pub async fn get_map(&self) -> HashMap<String, Vec<String>> {
        self.map.read().await.clone()
    }

    pub async fn add(&self, link: String, aliases: Vec<String>) -> Result<(), BotError> {
        let mut map = self.map.write().await;
        if map.contains_key(&link) {
            let mut old = map.get(&link).unwrap().clone();
            old.extend_from_slice(&aliases);
            map.insert(link, old);
        } else {
            map.insert(link, aliases);
        }
        self.serialize(&map).await
    }

    pub async fn remove(&self, link: &str) -> Result<(), BotError> {
        let mut map = self.map.write().await;
        map.remove(link);
        self.serialize(&map).await
    }

    pub async fn serialize(&self, map: &HashMap<String, Vec<String>>) -> Result<(), BotError> {
        tokio::fs::write(
            &self.path,
            serde_json::to_string_pretty(map).expect("Serializing links must work"),
        )
        .await
        .map_err(|_| BotError::new("Writing links file failed"))?;

        let mut rev_map = self.rev_map.write().await;
        *rev_map = Self::map_to_reverse_map(map);

        Ok(())
    }

    fn map_to_reverse_map(map: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
        let mut reverse_map = HashMap::new();

        for (link, aliases) in map.iter() {
            for alias in aliases {
                reverse_map.insert(alias.to_string(), link.clone());
            }
        }

        reverse_map
    }
}

#[derive(PartialEq, Eq)]
struct LinkHeapData {
    score: u64,
    key: String,
}

impl Ord for LinkHeapData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score.cmp(&other.score)
    }
}

impl PartialOrd for LinkHeapData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[poise::command(
    slash_command,
    subcommands("find", "browse"),
    guild_only,
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn link(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn autocomplete_link<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let Some(links) = ctx.data().links.get().await else {
        return futures::stream::iter(vec![]);
    };
    if partial.is_empty() {
        return futures::stream::iter(vec![]);
    }

    let keys = links.keys().collect::<Vec<_>>();

    let comparator =
        rapidfuzz::distance::jaro_winkler::BatchComparator::new(partial.to_lowercase().chars());
    let mut heap = BinaryHeap::with_capacity_min(6);

    for key in keys {
        let score = (comparator.similarity(key.chars()) * 10000.0) as u64;
        if heap.len() < 5 {
            heap.push(LinkHeapData {
                score,
                key: key.to_owned(),
            })
        } else if score > heap.peek().expect("Heap must be of size 5").score {
            heap.pop();
            heap.push(LinkHeapData {
                score,
                key: key.to_owned(),
            });
        }
    }

    let keys = heap
        .drain()
        .filter(|d| d.score > 8000)
        .map(|d| d.key)
        .rev()
        .collect::<Vec<_>>();

    futures::stream::iter(keys)
}

/// Find a link to a topic.
#[poise::command(slash_command)]
async fn find(
    ctx: Context<'_>,
    #[description = "The topic to find a link to."]
    #[autocomplete = "autocomplete_link"]
    topic: String,
) -> Result<(), Error> {
    let rev_map = ctx.data().links.get_blocking().await;

    if let Some(link) = rev_map.get(&topic) {
        ctx.say(format!("<{link}>")).await?;
    } else {
        eph(ctx, "I don't know that topic.").await?;
    }

    Ok(())
}

/// Browse all available links.
#[poise::command(slash_command)]
async fn browse(ctx: Context<'_>) -> Result<(), Error> {
    let map = ctx.data().links.get_map().await;

    let mut pages = map
        .into_iter()
        .map(|(lnk, aliases)| {
            (
                aliases
                    .into_iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                lnk,
            )
        })
        .collect::<Vec<_>>();
    pages.sort_by(|(a, _), (b, _)| a.cmp(b));

    paginate(ctx, pages, 10, "Available links".to_string(), false, true).await?;

    Ok(())
}

#[poise::command(
    slash_command,
    subcommands("add", "remove"),
    rename = "link-config",
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn link_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a link.
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "The link to add."] link: String,
    #[description = "The shorthands that point to the link. Comma separated."] shorthands: String,
) -> Result<(), Error> {
    let re = Regex::new("(, )+").unwrap();
    let shorthands = re
        .split(&shorthands)
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>();

    ctx.data().links.add(link, shorthands).await?;
    ctx.say("Link added.").await?;

    Ok(())
}

/// Remove a link.
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "The link to remove."] link: String,
) -> Result<(), Error> {
    ctx.data().links.remove(&link).await?;
    ctx.say("Link removed.").await?;
    Ok(())
}
