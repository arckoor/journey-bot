import asyncio
import re
import typing

import feedparser

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands, tasks

from Cogs.BaseCog import BaseCog
from Database.DBConnector import RSSFeed
from Database import DBUtils
from Views import Embed
from Util import Configuration, Logging


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
        feeds = RSSFeed.objects(guild=inter.guild_id)
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
        await inter.response.send_message(f"Feed added. ID: `{feed.id}`", ephemeral=True)
        await self.initialize_feed(feed)
        Logging.info(f"RSS feed ({feed.id}, {url}) was added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await Logging.guild_log(inter.guild_id, f"An RSS feed (`{feed.id}`, <{url}>) was added to {inter.channel.mention} by {inter.author.name} (`{inter.author.id}`)")

    @rss.sub_command(description="Remove an RSS feed from the server.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(name="id", description="The ID of the RSS feed.", min_length=24, max_length=24)
    ):
        feed = await self.get_feed(inter, id)
        if not feed:
            return
        url = feed.url
        id = feed.id
        feed.delete()
        await inter.response.send_message("Feed removed.", ephemeral=True)
        Logging.info(f"RSS feed ({id}, {url}) removed from channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await Logging.guild_log(inter.guild_id, f"An RSS feed (`{id}`, <{url}>) was removed from {inter.channel.mention} by {inter.author.name} (`{inter.author.id}`)")

    @tasks.loop(seconds=Configuration.get_master_var("RSS", {"update_interval_seconds": 300}).get("update_interval_seconds"))
    async def update(self):
        asyncio.ensure_future(asyncio.gather(*(self.update_feed(feed) for feed in RSSFeed.objects(initialized=True))))

    async def update_feed(self, feed: RSSFeed):
        if feed.id not in self.locks:
            self.locks[feed.id] = asyncio.Lock()
        if self.locks[feed.id].locked():
            return
        async with self.locks[feed.id]:
            try:
                channel = self.bot.get_channel(feed.channel)
                if not channel:
                    Logging.warning(f"Channel {feed.channel} not found for feed {feed.id} in guild {feed.guild}.")
                    await Logging.guild_log(feed.guild, f"I can't access the channel ({feed.channel}) for feed {feed.id})")
                    return
                feed.save()
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
                if len(ids) < 25:
                    Logging.warning(f"ID count for feed {feed.id} is less than 25. Already sent: {feed.already_sent}, new: {ids}, union: {list(set(ids + feed.already_sent))}")
                    ids = list(set(ids + feed.already_sent))
                feed.already_sent = ids
                if skipped_ids < 5:
                    Logging.warning(f"Skipped only {skipped_ids} entries for feed {feed.id} in channel {channel.name} ({channel.guild.name}). Is the update interval too high?")
                    await Logging.bot_log(f"Skipped only {skipped_ids} entries for feed {feed.id} in channel {channel.name} ({channel.guild.name}).")
                feed.save()
            except asyncio.CancelledError:
                pass

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
            DBUtils.ValidationType.INVALID_ID,
            DBUtils.ValidationType.ID_NOT_FOUND,
        ]
    ) -> RSSFeed | None:
        feed: RSSFeed
        feed, type = DBUtils.get_from_id_or_channel(RSSFeed, inter, id)
        response = {
            DBUtils.ValidationType.INVALID_ID:   "Invalid ID.",
            DBUtils.ValidationType.ID_NOT_FOUND: "No RSS feed found with that ID."
        }
        if type in respond_to:
            await inter.response.send_message(response[type], ephemeral=True)
            return None
        return feed


def setup(bot: commands.Bot):
    bot.add_cog(RSS(bot))
