import asyncio
import re
import datetime


import disnake
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

import asyncpraw.models
import asyncprawcore.exceptions

from Cogs.BaseCog import BaseCog
from Database.DBConnector import RedditAutoReply, RedditFlair
from Views import Embed
from Util import Logging, Reddit
from Util.Emoji import msg_with_emoji


class RedditReply(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.reddit_api = Reddit.get_reddit()
        self.stop_request = False

    async def cog_load(self):
        auto_reply = await RedditAutoReply.first()
        if auto_reply:
            self.bot.loop.create_task(self.process_posts(auto_reply))

    async def close(self):
        self.stop_request = True
        timer = 0
        while self.stop_request and timer <= 60:
            await asyncio.sleep(1)
            timer += 1

    @commands.slash_command(description="Reddit reply management.")
    @commands.guild_only()
    @commands.bot_has_permissions(send_messages=True)
    async def reddit(self, inter: ApplicationCommandInteraction):
        pass

    @commands.is_owner()
    @reddit.sub_command(name="configure-subreddit", description="Configure a subreddit to auto-reply to.")
    async def configure_subreddit(
        self,
        inter: ApplicationCommandInteraction,
        subreddit_name: str = commands.Param(
            name="subreddit-name",
            description="The name of the subreddit.",
            min_length=1,
            max_length=21,
        ),
        mgmt_role: disnake.Role = commands.Param(
            name="management-role", description="The role allowed to manage flairs and their responses."
        ),
    ):
        if await RedditAutoReply.first() is not None:
            await inter.response.send_message("Subreddit already configured.", ephemeral=True)
            return

        if self.stop_request:
            await inter.response.send_message(
                "Old subreddit process still running. Try again later, or reload the cog.", ephemeral=True
            )

        regex = r"[a-zA-Z0-9_]{1,21}"
        if not re.match(regex, subreddit_name):
            await inter.response.send_message("Invalid subreddit name.", ephemeral=True)
            return

        try:
            subreddit = await self.reddit_api.subreddit(subreddit_name, fetch=True)
            subreddit.id
        except Exception:
            await inter.response.send_message("Subreddit not found.", ephemeral=True)
            return

        auto_reply = await RedditAutoReply.create(
            guild=inter.guild.id,
            subreddit=subreddit_name,
            management_role=mgmt_role.id,
        )
        self.bot.loop.create_task(self.process_posts(auto_reply))
        await inter.response.send_message(f"Subreddit `{subreddit_name}` configured.", ephemeral=True)
        await Logging.bot_log(
            f"Subreddit `{subreddit_name}` for {inter.guild.name} (`{inter.guild_id}`) configured by {inter.user.name} (`{inter.user.id}`)"
        )

    @commands.is_owner()
    @reddit.sub_command(name="remove-subreddit", description="Configure a subreddit to auto-reply to.")
    async def remove_subreddit(self, inter: ApplicationCommandInteraction):
        auto_reply = await RedditAutoReply.first()
        if auto_reply is None:
            await inter.response.send_message("No subreddit configured.", ephemeral=True)
            return

        await auto_reply.delete()
        self.stop_request = True
        await inter.response.send_message("Subreddit deleted.", ephemeral=True)
        await Logging.bot_log(
            f"Subreddit {auto_reply.subreddit} for {inter.guild.name} (`{inter.guild_id}`) deleted by {inter.user.name} (`{inter.user.id}`)"
        )

    @reddit.sub_command_group(name="flairs", description="Flair management.")
    async def flairs(self, inter: ApplicationCommandInteraction):
        pass

    @flairs.sub_command(name="list", description="List all configured flairs.")
    async def list_flairs(self, inter: ApplicationCommandInteraction):
        if not await self.check_user(inter):
            return

        auto_reply = await RedditAutoReply.filter(guild=inter.guild.id).prefetch_related("flairs").first()
        if auto_reply is None:
            await inter.response.send_message("No subreddit configured.", ephemeral=True)
            return

        if not auto_reply.flairs:
            await inter.response.send_message("No flairs configured.", ephemeral=True)
            return

        embed = Embed.default_embed(
            title="Configured flairs",
            description="List of configured flairs and their responses.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url,
        )

        for flair in auto_reply.flairs:
            embed.add_field(
                name=flair.flair_name,
                value=flair.flair_reply,
                inline=False,
            )
        await inter.response.send_message(embed=embed)

    @flairs.sub_command(name="add", description="Add a response to a flair.")
    async def add_flair(
        self,
        inter: ApplicationCommandInteraction,
        flair_name: str = commands.Param(
            name="flair-name", description="The exact name/text of the flair.", min_length=1
        ),
        reply: str = commands.Param(
            name="reply", description="The reply to send to a post with this flair.", min_length=1
        ),
    ):
        if not await self.check_user(inter):
            return

        auto_reply = await RedditAutoReply.filter(guild=inter.guild.id).first()
        if auto_reply is None:
            await inter.response.send_message("No subreddit configured.", ephemeral=True)
            return

        # I'd love to just declare `flair_name` unique in the db, but I'm too lazy to use anything but `TextField`s
        flair = await RedditFlair.filter(guild=inter.guild.id, flair_name=flair_name).first()
        if flair is not None:
            await inter.response.send_message("Flair already configured.", ephemeral=True)
            return

        reply = reply.replace("\\n", "\n")

        flair = await RedditFlair.create(
            guild=inter.guild.id,
            flair_name=flair_name,
            flair_reply=reply,
            auto_reply=auto_reply,
        )

        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji(
                "FEED",
                f"A new reply for the `{flair.flair_name}` flair was added by {inter.author.name} (`{inter.author.id}`)",
            ),
        )
        Logging.info(
            f"A new reply for the `{flair.flair_name}` flair was added by {inter.author.name} (`{inter.author.id}`)"
        )
        await inter.response.send_message(f"Reply for flair `{flair_name}` added.")

    @flairs.sub_command(name="remove", description="Remove a previously configured response to a flair.")
    async def remove_flair(
        self,
        inter: ApplicationCommandInteraction,
        flair_name: str = commands.Param(
            name="flair-name", description="The exact name/text of the flair.", min_length=1
        ),
    ):
        if not await self.check_user(inter):
            return

        auto_reply = await RedditAutoReply.filter(guild=inter.guild.id).prefetch_related("flairs").first()
        if auto_reply is None:
            await inter.response.send_message("No subreddit configured.", ephemeral=True)
            return

        if not auto_reply.flairs:
            await inter.response.send_message("No flairs configured.", ephemeral=True)
            return

        flair = await RedditFlair.filter(guild=inter.guild.id, flair_name=flair_name).first()
        if flair is None:
            await inter.response.send_message("Flair not found.", ephemeral=True)
            return

        await flair.delete()
        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji(
                "FEED",
                f"The reply for the `{flair.flair_name}` flair was removed by {inter.author.name} (`{inter.author.id}`)",
            ),
        )
        Logging.info(
            f"The reply for the `{flair.flair_name}` flair was removed by {inter.author.name} (`{inter.author.id}`)"
        )
        await inter.response.send_message(f"Reply for flair `{flair_name}` removed.")

    async def process_posts(self, auto_reply: RedditAutoReply):
        Logging.info(f"Started auto-reply for {auto_reply.id} ({auto_reply.subreddit})")
        while True:
            try:
                feed = await self.reddit_api.subreddit(auto_reply.subreddit)
                async for submission in feed.stream.submissions():
                    if self.stop_request:
                        Logging.info(f"Stopped auto-reply for {auto_reply.id} ({auto_reply.subreddit})")
                        self.stop_request = False
                        return
                    submission: asyncpraw.models.Submission
                    post_time = datetime.datetime.fromtimestamp(submission.created_utc, tz=datetime.timezone.utc)
                    reply_latest_post = auto_reply.latest_post
                    if post_time > reply_latest_post:
                        flair_reply = await RedditFlair.filter(
                            auto_reply=auto_reply, flair_name=submission.link_flair_text
                        ).first()
                        if flair_reply is not None:
                            await self.send_reply(submission, flair_reply)
                            Logging.info(f"Sent reply for {submission.id} ({auto_reply.subreddit})")
                        auto_reply.latest_post = post_time
                        await auto_reply.save()

            except Exception as e:
                Logging.error(
                    f"Error processing posts for auto reply (`{auto_reply.id}`) ({auto_reply.subreddit}): {e}"
                )
                await Logging.bot_log(
                    f"Error processing posts for auto reply {auto_reply.id} ({auto_reply.subreddit}): {e}"
                )
                await asyncio.sleep(600)
                return

    async def send_reply(self, submission: asyncpraw.models.Submission, flair_reply: RedditFlair):
        try:
            await submission.reply(flair_reply.flair_reply)
            Logging.info(f"Sent reply for {submission.id} ({submission.subreddit})")
        except asyncprawcore.exceptions.Forbidden:
            await Logging.guild_log(
                msg_with_emoji(
                    "WARN",
                    f"Failed to send reply for {submission.id} ({submission.subreddit})",
                )
            )
            Logging.error(f"Failed to send reply for {submission.id} ({submission.subreddit}) - Forbidden")
        except Exception as e:
            Logging.error(f"Failed to send reply for {submission.id} ({submission.subreddit}) - {e}")
            await Logging.bot_log(
                f"Failed to send reply for {submission.id} ({submission.subreddit}) - {e}",
            )

    async def check_user(self, inter: ApplicationCommandInteraction):
        auto_reply = await RedditAutoReply.filter(guild=inter.guild.id).first()
        if auto_reply is None:
            await inter.response.send_message("No subreddit configured.", ephemeral=True)
            return False
        if auto_reply.management_role not in [x.id for x in inter.user.roles]:
            await inter.response.send_message("You do not have permission to use this command.", ephemeral=True)
            return False
        return True


def setup(bot: commands.Bot):
    bot.add_cog(RedditReply(bot))
