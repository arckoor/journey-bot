import time
import datetime
import asyncio

import disnake # noqa
from disnake import ApplicationCommandInteraction, Forbidden
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Util import Logging


class Moderation(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(name="remove-reactions", description="Remove reactions by a user.", dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True, manage_messages=True, read_message_history=True)
    async def remove_reactions(
        self,
        inter: ApplicationCommandInteraction,
        user:           disnake.User = commands.Param(description="The user to remove reactions from."),
        channel: disnake.TextChannel = commands.Param(default=None, description="The channel to remove reactions from."),
        time_frame:              str = commands.Param(default="5 Minutes", name="time-frame", description="The time frame to remove reactions from.",
                                                      choices=["10 Minutes", "30 Minutes", "1 Hour", "6 Hours", "12 Hours", "24 Hours", "3 Days", "7 Days"]),
    ):
        time_frame_to_delta = {
            "10 Minutes": datetime.timedelta(minutes=10),
            "30 Minutes": datetime.timedelta(minutes=30),
            "1 Hour": datetime.timedelta(hours=1),
            "6 Hours": datetime.timedelta(hours=6),
            "12 Hours": datetime.timedelta(hours=12),
            "24 Hours": datetime.timedelta(hours=24),
            "3 Days": datetime.timedelta(days=3),
            "7 Days": datetime.timedelta(days=7),
        }
        start = time.perf_counter()
        delta_time = disnake.utils.utcnow() - time_frame_to_delta.get(time_frame, datetime.timedelta(minutes=5))
        if channel is None:
            channel = inter.channel
        thinking_id = await inter.response.defer(with_message=True, ephemeral=False)
        try:
            history = await channel.history(after=delta_time, oldest_first=False).flatten()
        except Forbidden:
            await inter.followup.send(content="It seems like I can't read the message history in this channel.")
            return
        reaction_cnt = 0
        for message in history:
            try:
                for reaction in message.reactions:
                    if user in await reaction.users().flatten():
                        await reaction.remove(user)
                        reaction_cnt += 1
                if reaction_cnt % 80 == 0:
                    await asyncio.sleep(3)
            except Forbidden:
                pass
        end = time.perf_counter()
        Logging.info(f"Removed {reaction_cnt} reactions from {user} in {channel}. Took {end - start} seconds.")
        if not inter.is_expired():
            await inter.followup.send(content=f"Removed {reaction_cnt} reactions from {user.mention} in {channel.mention}.")
        else:
            try:
                await thinking_id.delete()
            except Exception:
                pass
            await inter.channel.send(content=f"Removed {reaction_cnt} reactions from {user.mention} in {channel.mention}.")


def setup(bot: commands.Bot):
    bot.add_cog(Moderation(bot))
