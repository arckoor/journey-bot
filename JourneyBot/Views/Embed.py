import datetime

import disnake  # noqa

from Util import Configuration


def default_embed(title=None, description=None, author=None, icon_url=None):
    now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
    embed = disnake.Embed(
        title=title,
        description=description,
        timestamp=now,
        color=disnake.Color(int(Configuration.get_master_var("EMBED_COLOR"), 16)),
    )
    if author and icon_url:
        embed.set_footer(text=f"Requested by {author}", icon_url=icon_url)
    return embed
