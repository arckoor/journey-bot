import asyncio
import datetime
import re

import feedparser

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands, tasks

from Cogs.BaseCog import BaseCog
from Database.DBConnector import RSSFeed
from Util import Configuration, Logging


class RSS(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    async def cog_load(self):
        self.update.start()

    async def close(self):
        self.update.stop()

    @commands.slash_command(dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    async def rss(self, inter: ApplicationCommandInteraction):
        """
        RSS feed management.
        """
        pass

    @rss.sub_command()
    async def list(self, inter: ApplicationCommandInteraction):
        """
        List all RSS feeds in the server.
        """
        feeds = RSSFeed.objects(guild=inter.channel.guild.id)
        now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
        if not feeds:
            await inter.response.send_message("No feeds found.", ephemeral=True)
            return
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
            embed.add_field(name=f"#{self.bot.get_channel(feed.channel).name}", value=f"{feed.url}", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @rss.sub_command()
    async def add(self, inter: ApplicationCommandInteraction, url: str):
        """
        Add an RSS feed to the current channel.

        Parameters
        ----------
        url: str
            The URL of the RSS feed.
        """
        if RSSFeed.objects(url=url, guild=inter.channel.guild.id):
            await inter.response.send_message("This feed is already registered.", ephemeral=True)
            return
        regex = r"((http(s)?://)?www\.)?reddit\.com/r/[a-zA-Z0-9_]{1,21}/(new/)?\.rss"
        if not re.match(regex, url):
            await inter.response.send_message("This is not a valid Reddit RSS feed.", ephemeral=True)
            return
        if "new" not in url:
            url = url.replace(".rss", "new/.rss")
        feed = RSSFeed(
            guild=inter.channel.guild.id,
            url=url,
            channel=inter.channel.id
        )
        feed.save()
        await inter.response.send_message("Feed added.", ephemeral=True)
        await self.initialize_feed(feed)
        Logging.info(f"RSS feed added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id}): {url}")

    @rss.sub_command()
    async def remove(self, inter: ApplicationCommandInteraction, url: str):
        """
        Remove an RSS feed from the server.

        Parameters
        ----------
        url: str
            The URL of the RSS feed.
        """
        if not RSSFeed.objects(url=url, guild=inter.channel.guild.id):
            await inter.response.send_message("This feed is not registered.", ephemeral=True)
            return
        feed = RSSFeed.objects(url=url, guild=inter.channel.guild.id).first()
        feed.delete()
        await inter.response.send_message("Feed removed.", ephemeral=True)
        Logging.info(f"RSS feed removed from channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id}): {url}")

    @tasks.loop(seconds=Configuration.get_master_var("RSS").get("update_interval_seconds"))
    async def update(self):
        for feed in RSSFeed.objects():
            if feed.initialized:
                await self.update_feed(feed)

    async def update_feed(self, feed: RSSFeed):
        f = feedparser.parse(feed.url)
        ids = []
        channel = self.bot.get_channel(feed.channel)
        skipped_ids = 0
        sent = 0
        for entries in f.entries:
            ids.append(entries.id)
            if entries.id in feed.already_sent:
                skipped_ids += 1
                continue
            message = f"<:banjNote:881229397305212978>  |  **{entries.title}**\n\n{entries.link}"
            if sent % 5 == 1:
                await asyncio.sleep(15)
            sent += 1
            await channel.send(message)
        feed.already_sent = ids
        if skipped_ids < 5:
            Logging.warning(f"Skipped only {skipped_ids} entries for feed {feed.url} in channel {channel.name} ({channel.guild.name}). Is the update interval too high?")
        feed.save()

    async def populate_ids(self, feed: RSSFeed):
        f = feedparser.parse(feed.url)
        ids = []
        for entries in f.entries:
            ids.append(entries.id)
        feed.already_sent = ids
        feed.save()

    async def initialize_feed(self, feed: RSSFeed):
        await self.populate_ids(feed)
        feed.initialized = True
        feed.save()


def setup(bot: commands.Bot):
    bot.add_cog(RSS(bot))
