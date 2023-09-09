import asyncio
import datetime
import re
import typing

import feedparser

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands, tasks

from Cogs.BaseCog import BaseCog
from Database.DBConnector import RSSFeed
from Util import Configuration, Logging, Validation


class RSS(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    async def cog_load(self):
        self.update.start()

    async def close(self):
        self.update.stop()

    @commands.slash_command(dm_permission=False, description="RSS feed management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    async def rss(self, inter: ApplicationCommandInteraction):
        pass

    @rss.sub_command(name="template-help", description="RSS feed help.")
    async def template_help(self, inter: ApplicationCommandInteraction):
        embed = disnake.Embed(
            title="RSS Feed Help",
            description="Explanation of the template syntax.",
            color=disnake.Color.from_rgb(**Configuration.get_master_var("EMBED_COLOR"))
        )
        embed.add_field(name="Variables", value="Variables are replaced with the corresponding value from the RSS feed.", inline=False)
        embed.add_field(name="Line breaks", value="Line breaks are represented by `\\n`.", inline=False)
        embed.add_field(name="{{title}}", value="The title of the post.", inline=False)
        embed.add_field(name="{{link}}", value="The link to the post.", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @rss.sub_command(description="List all RSS feeds in the server.")
    async def list(self, inter: ApplicationCommandInteraction):
        feeds = RSSFeed.objects(guild=inter.guild_id)
        if not feeds:
            await inter.response.send_message("No feeds found.", ephemeral=True)
            return
        now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
        embed = disnake.Embed(
            title="RSS Feeds",
            description="All RSS feeds in this server.",
            timestamp=now,
            color=disnake.Color.from_rgb(**Configuration.get_master_var("EMBED_COLOR"))
        )
        embed.set_footer(
            text=f"Requested by {inter.author.name}",
            icon_url=inter.author.avatar.url
        )
        for feed in feeds:
            channel = self.bot.get_channel(feed.channel)
            if channel and channel.name:
                channel_name = channel.name
            else:
                channel_name = "Unknown"
            embed.add_field(name=f"#{channel_name} | ID: {feed.id}", value=f"{feed.url}", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @rss.sub_command(description="Add an RSS feed to the current channel.")
    async def add(
        self,
        inter: ApplicationCommandInteraction,
        url:      str = commands.Param(name="url", description="The URL of the RSS feed."),
        template: str = commands.Param(default=None, name="template", description=""),
    ):
        regex = r"http(s)?://(www\.)?reddit\.com/r/[a-zA-Z0-9_]{1,21}/(new/)?\.rss"
        if not re.match(regex, url):
            await inter.response.send_message("This is not a valid Reddit RSS feed.", ephemeral=True)
            return
        if "new" not in url:
            url = url.replace(".rss", "new/.rss")
        url = url.replace("http://", "https://")

        if not template:
            template = "{{title}}\n{{link}}"
        else:
            template = template.replace("\\n", "\n")

        feed = RSSFeed(
            guild=inter.guild_id,
            url=url,
            template=template,
            channel=inter.channel.id
        )
        feed.save()
        await inter.response.send_message("Feed added.", ephemeral=True)
        await self.initialize_feed(feed)
        Logging.info(f"RSS feed added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id}): {url}")

    @rss.sub_command(description="Remove an RSS feed from the server.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(name="id", description="The ID of the RSS feed.", min_length=24, max_length=24)
    ):
        rssFeed = await self.get_feed(inter, id)
        if not rssFeed:
            return
        url = rssFeed.url
        rssFeed.delete()
        await inter.response.send_message("Feed removed.", ephemeral=True)
        Logging.info(f"RSS feed removed from channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id}): {url}")

    @tasks.loop(seconds=Configuration.get_master_var("RSS").get("update_interval_seconds"))
    async def update(self):
        await asyncio.gather(*(self.update_feed(feed) for feed in RSSFeed.objects(initialized=True)))

    async def update_feed(self, feed: RSSFeed):
        if feed.in_progress:
            return
        feed.in_progress = True
        feed.save()
        channel = self.bot.get_channel(feed.channel)
        if not channel:
            Logging.warning(f"Channel {feed.channel} not found for feed {feed.id} in guild {feed.guild}.")
            return
        f = feedparser.parse(feed.url)
        ids = [x.id for x in f.entries]
        to_send = [x for x in f.entries if x.id not in feed.already_sent]
        skipped_ids = len(ids) - len(to_send)
        for entry in to_send:
            await channel.trigger_typing()
            await asyncio.sleep(3)
            message = feed.template.replace("{{title}}", entry.title)
            message = message.replace("{{link}}", entry.link)
            await channel.send(message)
        feed.already_sent = ids
        if skipped_ids < 5:
            Logging.warning(f"Skipped only {skipped_ids} entries for feed {feed.id} in channel {channel.name} ({channel.guild.name}). Is the update interval too high?")
        feed.in_progress = False
        feed.save()

    async def populate_ids(self, feed: RSSFeed):
        f = feedparser.parse(feed.url)
        ids = [x.id for x in f.entries]
        feed.already_sent = ids
        feed.save()

    async def initialize_feed(self, feed: RSSFeed):
        await self.populate_ids(feed)
        feed.initialized = True
        feed.save()

    async def get_feed(
        self,
        inter: ApplicationCommandInteraction,
        id: str,
        respond_to: [typing.Literal] = [
            Validation.ValidationType.INVALID_ID,
            Validation.ValidationType.ID_NOT_FOUND,
        ]
    ) -> RSSFeed | None:
        rssFeed: RSSFeed
        rssFeed, type = await Validation.get_from_id_or_channel(RSSFeed, inter, id)
        response = {
            Validation.ValidationType.INVALID_ID:   "Invalid ID.",
            Validation.ValidationType.ID_NOT_FOUND: "No RSS feed found with that ID."
        }
        if type in respond_to:
            await inter.response.send_message(response[type], ephemeral=True)
            return None
        return rssFeed


def setup(bot: commands.Bot):
    bot.add_cog(RSS(bot))
