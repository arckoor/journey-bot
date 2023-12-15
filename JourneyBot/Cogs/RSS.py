import asyncio
import re
import datetime

import feedparser

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands, tasks

from Cogs.BaseCog import BaseCog
from prisma.models import RSSFeed
from Database.DBConnector import db
from Views import Embed
from Util import Configuration, Utils, Logging
from Util.Emoji import msg_with_emoji


class RSS(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.locks = {}

    async def cog_load(self):
        self.update.start()

    async def close(self):
        self.update.stop()

    @commands.slash_command(dm_permission=False, description="RSS feed management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def rss(self, inter: ApplicationCommandInteraction):
        pass

    @rss.sub_command(name="template-help", description="RSS feed help.")
    async def template_help(self, inter: ApplicationCommandInteraction):
        embed = Embed.default_embed(
            title="RSS Feed Help",
            description="Explanation of the template syntax.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="Line breaks", value="Line breaks are represented by `\\n`.", inline=False)
        embed.add_field(name="Variables", value="Variables are replaced with the corresponding value from the RSS feed.", inline=False)
        embed.add_field(name="{{title}}", value="The title of the post.", inline=False)
        embed.add_field(name="{{link}}", value="The link to the post.", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @rss.sub_command(description="List all RSS feeds in the server.")
    async def list(self, inter: ApplicationCommandInteraction):
        feeds = await db.rssfeed.find_many(
            where={
                "guild": inter.guild_id
            }
        )
        if not feeds:
            await inter.response.send_message("No feeds found.", ephemeral=True)
            return
        embed = Embed.default_embed(
            title="RSS Feeds",
            description="All RSS feeds in this server.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        for feed in feeds:
            channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(feed.channel), Utils.get_alternate_channel(feed.channel))
            embed.add_field(name=f"#{channel.name} | ID: {feed.id}", value=f"{feed.url}", inline=False)
        await inter.response.send_message(embed=embed)

    @rss.sub_command(description="Add an RSS feed to the current channel.")
    async def add(
        self,
        inter: ApplicationCommandInteraction,
        url:      str = commands.Param(name="url", description="The URL of the RSS feed."),
        template: str = commands.Param(default=None, name="template", description="The template for new posts."),
    ):
        regex = r"http(s)?://(www\.)?reddit\.com/r/[a-zA-Z0-9_]{1,21}/(new/)?\.rss"
        if not re.match(regex, url):
            await inter.response.send_message("Invalid reddit rss feed url.", ephemeral=True)
            return
        if "new" not in url:
            url = url.replace(".rss", "new/.rss")
        url = url.replace("http://", "https://")

        if not template:
            template = "{{title}}\n{{link}}"
        else:
            template = template.replace("\\n", "\n")

        feed = await db.rssfeed.create(
            data={
                "guild": inter.guild_id,
                "url": url,
                "template": template,
                "channel": inter.channel.id
            }
        )
        await inter.response.send_message(f"Feed added. ID: `{feed.id}`")
        await self.initialize_feed(feed)
        await Logging.guild_log(inter.guild_id, msg_with_emoji("RSS", f"An RSS feed (`{feed.id}`, <{url}>) was added to {inter.channel.mention} by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"RSS feed ({feed.id}, {url}) was added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @rss.sub_command(description="Remove an RSS feed from the server.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(name="id", description="The ID of the RSS feed.", min_length=36, max_length=36)
    ):
        feed = await db.rssfeed.find_first(
            where={
                "id": id
            }
        )
        if not feed:
            await inter.response.send_message("No RSS feed found with that ID.", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(feed.channel), Utils.get_alternate_channel(feed.channel))
        url = feed.url
        id = feed.id
        await db.rssfeed.delete(
            where={
                "id": id
            }
        )
        await inter.response.send_message("Feed removed.")
        await Logging.guild_log(inter.guild_id, msg_with_emoji("RSS", f"An RSS feed (`{id}`, <{url}>) was removed from {channel.mention} by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"RSS feed ({id}, {url}) removed from channel {channel.name if channel and channel.name else 'unknown'} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @tasks.loop(seconds=Configuration.get_master_var("RSS", {"update_interval_seconds": 300}).get("update_interval_seconds"))
    async def update(self):
        asyncio.ensure_future(asyncio.gather(*(self.update_feed(feed) for feed in await db.rssfeed.find_many(
            where={
                "initialized": True
            }
        ))))

    async def update_feed(self, feed: RSSFeed):
        if feed.id not in self.locks:
            self.locks[feed.id] = asyncio.Lock()
        if self.locks[feed.id].locked():
            return
        async with self.locks[feed.id]:
            try:
                channel = self.bot.get_channel(feed.channel)
                if not channel or not channel.permissions_for(channel.guild.me).send_messages:
                    await Logging.guild_log(feed.guild, msg_with_emoji("WARN", f"I can't access the channel ({feed.channel}) for feed {feed.id})"))
                    Logging.warning(f"Channel {feed.channel} not found for feed {feed.id} in guild {feed.guild}.")
                    return
                f = feedparser.parse(feed.url)
                latest_post = feed.latest_post
                if latest_post:
                    latest_post = latest_post.replace(tzinfo=datetime.timezone.utc)
                max_latest_post = None
                sent = 0
                for entry in f.entries:
                    d = datetime.datetime.strptime(entry.published, "%Y-%m-%dT%H:%M:%S%z").replace(tzinfo=datetime.timezone.utc)
                    if max_latest_post is None or d > max_latest_post:
                        max_latest_post = d
                    if latest_post is None or d > latest_post:
                        await channel.trigger_typing()
                        await asyncio.sleep(3)
                        message = feed.template.replace("{{title}}", entry.title)
                        message = message.replace("{{link}}", entry.link)
                        await channel.send(message)
                        sent += 1
                skipped = len(f.entries) - sent
                if skipped < 5:
                    Logging.warning(f"Skipped only {skipped} entries for feed {feed.id} in channel {channel.name} ({channel.guild.name}). Is the update interval too high?")
                    await Logging.bot_log(msg_with_emoji("WARN", f"Skipped only {skipped} entries for feed `{feed.id}` in channel {channel.name} ({channel.guild.name})."))
                if latest_post is None or max_latest_post > latest_post:
                    await db.rssfeed.update(
                        where={
                            "id": feed.id
                        },
                        data={
                            "latest_post": Utils.coalesce(max_latest_post, feed.latest_post)
                        }
                    )
            except asyncio.CancelledError:
                pass

    async def populate_ids(self, feed: RSSFeed):
        f = feedparser.parse(feed.url)
        max_latest_post = None
        for entry in f.entries:
            d = datetime.datetime.strptime(entry.published, "%Y-%m-%dT%H:%M:%S%z")
            if max_latest_post is None or d > max_latest_post:
                max_latest_post = d
        await db.rssfeed.update(
            where={
                "id": feed.id
            },
            data={
                "latest_post": max_latest_post
            }
        )

    async def initialize_feed(self, feed: RSSFeed):
        await self.populate_ids(feed)
        await db.rssfeed.update(
            where={
                "id": feed.id
            },
            data={
                "initialized": True
            }
        )


def setup(bot: commands.Bot):
    bot.add_cog(RSS(bot))
