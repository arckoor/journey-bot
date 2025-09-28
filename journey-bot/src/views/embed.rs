use chrono::Utc;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedFooter};

use crate::Context;

pub fn default_embed(ctx: Context<'_>) -> CreateEmbed {
    let author = ctx.author();

    let mut footer = CreateEmbedFooter::new(format!("Requested by {}", author.name));

    if let Some(icon_url) = author.static_avatar_url() {
        footer = footer.icon_url(icon_url);
    }

    CreateEmbed::default()
        .footer(footer)
        .timestamp(Utc::now())
        .colour(ctx.data().embed_color)
}
