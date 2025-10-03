use poise::{
    CreateReply,
    serenity_prelude::{self as serenity, CreateEmbed},
};

use crate::{Context, Error, views::embed::default_embed};

fn build_embed(
    ctx: Context<'_>,
    title: &str,
    pages: &[(String, String, bool)],
    page: usize,
    per_page: usize,
) -> CreateEmbed {
    default_embed(ctx)
        .title(title)
        .fields(pages.iter().skip(page * per_page).take(per_page).cloned())
}

pub async fn paginate(
    ctx: Context<'_>,
    pages: Vec<(String, String)>,
    per_page: usize,
    title: String,
    inline: bool,
    ephemeral: bool,
) -> Result<(), Error> {
    // unique IDs for this interaction
    let ctx_id = ctx.id();
    let prev_button_id = format!("{}prev", ctx_id);
    let next_button_id = format!("{}next", ctx_id);
    let max_pages = (pages.len() - 1) / per_page;

    let pages = pages
        .into_iter()
        .map(|(key, value)| (key, value, inline))
        .collect::<Vec<_>>();

    // first page
    let reply = {
        let components = serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(&prev_button_id)
                .style(serenity::ButtonStyle::Secondary)
                .label("Previous"),
            serenity::CreateButton::new(&next_button_id)
                .style(serenity::ButtonStyle::Secondary)
                .label("Next"),
        ]);

        let embed = build_embed(ctx, &title, &pages, 0, per_page);

        CreateReply::default()
            .embed(embed)
            .ephemeral(ephemeral)
            .components(vec![components])
    };

    ctx.send(reply).await?;

    let mut current_page = 0;
    while let Some(press) = serenity::collector::ComponentInteractionCollector::new(ctx)
        // filter for our IDs
        .filter(move |press| press.data.custom_id.starts_with(&ctx_id.to_string()))
        .timeout(std::time::Duration::from_secs(60 * 60 * 4))
        .await
    {
        if press.data.custom_id == next_button_id {
            current_page += 1;
            if current_page >= max_pages {
                current_page = 0;
            }
        } else if press.data.custom_id == prev_button_id {
            current_page = current_page.checked_sub(1).unwrap_or(max_pages - 1);
        } else {
            continue;
        }

        press
            .create_response(
                ctx.serenity_context(),
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(build_embed(ctx, &title, &pages, current_page, per_page))
                        .ephemeral(ephemeral),
                ),
            )
            .await?;
    }

    Ok(())
}
