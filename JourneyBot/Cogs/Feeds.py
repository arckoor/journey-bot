import asyncio
import re
import datetime

import disnake
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

import tortoise.exceptions

from Cogs.BaseCog import BaseCog
from Database.DBConnector import RedditFeed
from Views import Embed
from Util import Utils, Logging, Reddit
from Util.Emoji import msg_with_emoji


class Feeds(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.reddit_api = Reddit.get_reddit()
        # actually UUID, but str is good enough
        self.sleep_times: dict[str, int] = {}
        self.reported_fails: dict[str, bool] = {}
        self.tasks: dict[str, asyncio.Task] = {}
        self.sleep_start = 15
        self.sleep_stop = 7680

    async def cog_load(self):
        for feed in await RedditFeed.all():
            self.spawn_task(feed)

    def spawn_task(self, feed: RedditFeed):
        if self.sleep_times.get(feed.id) is None:
            self.sleep_times[feed.id] = 0
        if self.reported_fails.get(feed.id) is None:
            self.reported_fails[feed.id] = False
        task = self.bot.loop.create_task(self.update_reddit_feed(feed))
        self.tasks[feed.id] = task

    async def close(self):
        for task in self.tasks.values():
            task.cancel()
        timer = 0
        while timer <= 60:
            to_del = []
            for key, task in self.tasks.items():
                if task.done():
                    to_del.append(key)
            for item in to_del:
                del self.tasks[item]
            if len(self.tasks) == 0:
                break
            await asyncio.sleep(1)
            timer += 1

    @commands.slash_command(description="Feed management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def feed(self, inter: ApplicationCommandInteraction):
        pass

    @feed.sub_command(name="template-help", description="Feed template help.")
    async def template_help(self, inter: ApplicationCommandInteraction):
        embed = Embed.default_embed(
            title="Feed Help",
            description="Explanation of the template syntax.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url,
        )
        embed.add_field(
            name="Line breaks",
            value="Line breaks are represented by `\\n`.",
            inline=False,
        )
        embed.add_field(
            name="Variables",
            value="Variables are replaced with the corresponding value from the feed.",
            inline=False,
        )
        embed.add_field(name="{{title}}", value="The title of the content.", inline=False)
        embed.add_field(name="{{link}}", value="The link to the content.", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @feed.sub_command(description="List all feeds in the server.")
    async def list(self, inter: ApplicationCommandInteraction):
        reddit_feeds = await RedditFeed.filter(guild=inter.guild_id).all()
        if not reddit_feeds:
            await inter.response.send_message("No feeds found.", ephemeral=True)
            return
        embed = Embed.default_embed(
            title="Feeds",
            description="List of all feeds in the server.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url,
        )
        for feed in reddit_feeds:
            channel: disnake.abc.GuildChannel = Utils.coalesce(
                self.bot.get_channel(feed.channel),
                Utils.get_alternate_channel(feed.channel),
            )
            embed.add_field(
                name=f"#{channel.name} | ID: {feed.id}",
                value=f"r/{feed.subreddit}",
                inline=False,
            )
        await inter.response.send_message(embed=embed)

    @feed.sub_command_group(description="Add a feed to the server.")
    async def add(self, inter: ApplicationCommandInteraction):
        pass

    @add.sub_command(description="Add a reddit feed to the server.")
    async def reddit(
        self,
        inter: ApplicationCommandInteraction,
        subreddit_name: str = commands.Param(
            name="subreddit-name",
            description="The name of the subreddit.",
            min_length=1,
            max_length=21,
        ),
        template: str = commands.Param(default=None, name="template", description="The template for new posts."),
    ):
        regex = r"[a-zA-Z0-9_]{1,21}"
        if not re.match(regex, subreddit_name):
            await inter.response.send_message("Invalid subreddit name.", ephemeral=True)
            return
        if not template:
            template = "{{title}}\n{{link}}"
        else:
            template = template.replace("\\n", "\n")

        try:
            subreddit = await self.reddit_api.subreddit(subreddit_name, fetch=True)
            subreddit.id
        except Exception:
            await inter.response.send_message("Subreddit not found.", ephemeral=True)
            return
        feed = await RedditFeed.create(
            guild=inter.guild_id,
            channel=inter.channel_id,
            subreddit=subreddit_name,
            template=template,
        )
        self.spawn_task(feed)
        await inter.response.send_message(f"Added feed for r/{subreddit_name}.")
        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji(
                "FEED",
                f"A feed for r/{subreddit_name} (`{feed.id}`) was added to {inter.channel.mention} by {inter.author.name} (`{inter.author.id}`)",
            ),
        )
        Logging.info(
            f"A feed for r/{subreddit_name} ({feed.id}) was added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})"
        )

    @feed.sub_command(description="Remove a feed from the server.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(
            name="feed-id",
            description="The ID of the feed to remove.",
            min_length=36,
            max_length=36,
        ),
    ):
        try:
            feed = await RedditFeed.get(id=id, guild=inter.guild_id)
        except tortoise.exceptions.DoesNotExist:
            await inter.response.send_message("No feed found with that ID.", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(
            self.bot.get_channel(feed.channel),
            Utils.get_alternate_channel(feed.channel),
        )
        await feed.delete()
        self.tasks[feed.id].cancel()
        del self.tasks[feed.id]
        await inter.response.send_message("Feed removed.")
        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji(
                "FEED",
                f"A feed for r/{feed.subreddit} (`{feed.id}`) was removed from {channel.mention} by {inter.author.name} (`{inter.author.id}`)",
            ),
        )
        Logging.info(
            f"A feed for r/{feed.subreddit} ({feed.id}) removed from channel {channel.name}"
            + f" ({inter.channel.guild.name}) by {inter.author.name} ({inter.author.id})"
        )

    @feed.sub_command(description="Manually restart a feed.")
    async def restart(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(
            name="feed-id",
            description="The ID of the feed to restart.",
            min_length=36,
            max_length=36,
        ),
    ):
        try:
            feed = await RedditFeed.get(id=id, guild=inter.guild_id)
        except tortoise.exceptions.DoesNotExist:
            await inter.response.send_message("No feed found with that ID.", ephemeral=True)
            return
        if feed.id not in self.restarts_available:
            await inter.response.send_message("This feed is not available for restart.", ephemeral=True)
            return
        self.restarts_available.remove(feed.id)
        self.bot.loop.create_task(self.update_reddit_feed(feed))
        await Logging.guild_log(
            feed.guild,
            msg_with_emoji(
                "FEED",
                f"A feed for {feed.subreddit} (`{feed.id}`) was manually restarted by {inter.author.name} (`{inter.author.id}`)",
            ),
        )
        Logging.info(f"Manually restarted feed {feed.id} ({feed.subreddit})")
        await inter.response.send_message("Attempting to restart feed.")

    async def update_reddit_feed(self, feed: RedditFeed):
        Logging.info(f"Starting feed {feed.id} ({feed.subreddit})")
        try:
            subreddit = await self.reddit_api.subreddit(feed.subreddit)
            async for submission in subreddit.stream.submissions():
                post_time = datetime.datetime.fromtimestamp(submission.created_utc, tz=datetime.timezone.utc)
                feed_latest_post = feed.latest_post
                if post_time > feed_latest_post:
                    await self.post_reddit_feed(feed, submission)
                    feed.latest_post = post_time
                    await feed.save()
            self.sleep_times[feed.id] = 0
            self.reported_fails[feed.id] = False
        except asyncio.CancelledError:
            Logging.info(f"Stopped feed for {feed.id} ({feed.subreddit})")
            return
        except Exception as e:
            Logging.warning(f"Exception in handle_post_error for {feed.id}: {e}")
            await self.handle_post_error(feed, e)

    async def post_reddit_feed(self, feed: RedditFeed, submission):
        channel = self.bot.get_channel(feed.channel)
        if not channel or not channel.permissions_for(channel.guild.me).send_messages:
            raise PermissionError
        await channel.trigger_typing()
        await asyncio.sleep(3)
        if not channel:
            Logging.guild_log(
                feed.guild,
                msg_with_emoji(
                    "WARN",
                    f"Unable to post to channel {feed.channel} for feed `{feed.id}` ({feed.subreddit})",
                ),
            )
            Logging.warning(f"Unable to post to channel {feed.channel} for feed {feed.id} ({feed.subreddit})")
            return
        message = feed.template.replace("{{title}}", submission.title)
        message = message.replace("{{link}}", f"https://www.reddit.com{submission.permalink}")
        await channel.send(message)

    async def handle_post_error(self, feed: RedditFeed, error: Exception):
        if isinstance(error, PermissionError):
            channel = self.bot.get_channel(feed.channel)
            c = f"channel `{feed.channel}`" if not channel else channel.mention
            await Logging.guild_log(
                feed.guild,
                msg_with_emoji(
                    "WARN",
                    f"I could not send a post for feed (`{feed.id}`) in {c}, because I don't have access to the channel. I will try again in 10 minutes.",
                ),
            )
            Logging.warning(f"Could not send post for feed {feed.id}. Channel {feed.channel} not found.")
            await asyncio.sleep(60 * 10)
        else:
            if self.sleep_times[feed.id] == 0:
                self.sleep_times[feed.id] = self.sleep_start
            await asyncio.sleep(self.sleep_times[feed.id])
            if self.sleep_times[feed.id] < self.sleep_stop:
                self.sleep_times[feed.id] *= 2
            else:
                if not self.reported_fails[feed.id]:
                    total_slept = 0
                    delay = self.sleep_start
                    while delay < self.sleep_stop:
                        total_slept += delay
                        delay *= 2

                    await Logging.guild_log(
                        feed.guild,
                        msg_with_emoji(
                            "WARN",
                            f"A feed for {feed.subreddit} (`{feed.id}`) has been failing for {int((total_slept / 60))} minutes."
                            + f" It will try to continue restarting every {int((self.sleep_stop / 60))} minutes.",
                        ),
                    )
                    Logging.error(
                        f"A feed for {feed.subreddit} ({feed.id}) has been failing for {int((total_slept / 60))} minutes."
                        + f" It will try to continue restarting every {int((self.sleep_stop / 60))} minutes."
                    )
                    self.reported_fails[feed.id] = True
        self.spawn_task(feed)


def setup(bot: commands.Bot):
    bot.add_cog(Feeds(bot))
