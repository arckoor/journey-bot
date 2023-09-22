import time
import datetime
import re

import disnake # noqa
from disnake import ApplicationCommandInteraction, Forbidden
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database import DBUtils
from Util import Logging


async def convert_to_text_channels(inter: ApplicationCommandInteraction, arg: str) -> list[disnake.TextChannel]:
    channels = []
    for channel in re.split("[, ]+", arg):
        try:
            channel = await commands.TextChannelConverter().convert(inter, channel)
            channels.append(channel)
        except commands.ChannelNotFound:
            pass
    return channels


class Moderation(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(name="remove-reacts", description="Remove reactions by a user.", dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True, manage_messages=True, read_message_history=True)
    async def remove_reacts(
        self,
        inter: ApplicationCommandInteraction,
        user:           disnake.User = commands.Param(description="The user to remove reactions from."),
        channel: disnake.TextChannel = commands.Param(default=None, description="The channel to remove reactions from."),
        all_channels:           bool = commands.Param(default=False, name="all-channels", description="Search for and remove reactions in all channels."),
        time_frame:              str = commands.Param(default=None, name="time-frame", description="The time frame to remove reactions from.",
                                                      choices=["10 Minutes", "30 Minutes", "1 Hour", "6 Hours", "12 Hours", "24 Hours", "3 Days", "7 Days", "14 Days", "30 Days"]),
        message_amount:          int = commands.Param(default=None, name="message-amount", description="The amount of messages to search through."),
        remove_entire_react:    bool = commands.Param(default=False, name="remove-entire-react", description="Remove the entire reaction instead of removing the user's reaction."),
    ):
        if time_frame and message_amount:
            await inter.response.send_message("You can't specify both a time frame and a message amount.", ephemeral=True)
            return
        elif not time_frame and not message_amount:
            time_frame = "10 Minutes"
            await inter.channel.send(content="You didn't provide a time frame or a message amount, defaulting to 10 minutes.")
        time_frame_to_delta = {
            "10 Minutes": datetime.timedelta(minutes=10),
            "30 Minutes": datetime.timedelta(minutes=30),
            "1 Hour":     datetime.timedelta(hours=1),
            "6 Hours":    datetime.timedelta(hours=6),
            "12 Hours":   datetime.timedelta(hours=12),
            "24 Hours":   datetime.timedelta(hours=24),
            "3 Days":     datetime.timedelta(days=3),
            "7 Days":     datetime.timedelta(days=7),
            "14 Days":    datetime.timedelta(days=14),
            "30 Days":    datetime.timedelta(days=30)
        }
        start = time.perf_counter()
        delta_time = disnake.utils.utcnow() - time_frame_to_delta.get(time_frame, datetime.timedelta(minutes=10))
        if all_channels:
            guild_config = DBUtils.get_guild_config(inter.guild_id)
            channels = [x for x in inter.guild.text_channels if x.id not in guild_config.react_remove_excluded_channels]
            await inter.channel.send(content="Searching through all channels. This may take a while.")
        else:
            channels = [channel if channel is not None else inter.channel]
        thinking_id = await inter.response.defer(with_message=True, ephemeral=False)
        reaction_cnt = 0
        msg_cnt = 0
        for channel in channels:
            if not isinstance(channel, disnake.TextChannel):
                continue
            try:
                if message_amount:
                    history = await channel.history(limit=message_amount, oldest_first=False).flatten()
                else:
                    history = await channel.history(after=delta_time, oldest_first=False).flatten()
                for message in history:
                    for reaction in message.reactions:
                        if user in await reaction.users().flatten():
                            if remove_entire_react:
                                await reaction.clear()
                            else:
                                await reaction.remove(user)
                            reaction_cnt += 1
                    msg_cnt += 1
            except Forbidden:
                await inter.channel.send(content=f"I am unable to read the message history in {channel.mention}")

        end = time.perf_counter()
        Logging.info(f"Searched through {msg_cnt} messages and removed {reaction_cnt} reactions from  {user} in {len(channels)} channels. Took {end - start} seconds.")
        if not inter.is_expired():
            await inter.followup.send(content=f"I searched through {msg_cnt} messages and removed {reaction_cnt} reactions from {user.mention} in {len(channels)} channels.")
        else:
            try:
                await thinking_id.delete()
            except Exception:
                pass
            await inter.channel.send(content=f"Removed {reaction_cnt} reactions from {user.mention} in {channel.mention}.")

    @commands.slash_command(name="remove-reacts-config", description="Configure remove-reacts.", dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def remove_reacts_config(inter: ApplicationCommandInteraction, *_):
        pass

    @remove_reacts_config.sub_command_group(name="channels", description="Configure the channels for remove-reacts.")
    async def remove_reacts_channels(inter: ApplicationCommandInteraction, *_):
        pass

    @remove_reacts_channels.sub_command(name="exclude", description="Exclude channels from remove-reacts.")
    async def remove_reacts_channels_exclude(inter: ApplicationCommandInteraction, channels: str = commands.Param(description="The channels to exclude.", converter=convert_to_text_channels)):
        guildConfig = DBUtils.get_guild_config(inter.guild_id)
        excluded = []
        for channel in channels:
            channel: disnake.TextChannel
            if channel.id not in guildConfig.react_remove_excluded_channels:
                guildConfig.react_remove_excluded_channels.append(channel.id)
                excluded.append(channel.mention)
        guildConfig.save()
        if excluded:
            await inter.response.send_message(f"Excluded {', '.join(excluded)} from remove-reacts.")
        else:
            await inter.response.send_message("No further channels were excluded from remove-reacts.")

    @remove_reacts_channels.sub_command(name="include", description="Re-include channels in remove-reacts.")
    async def remove_reacts_channels_include(inter: ApplicationCommandInteraction, channels: str = commands.Param(description="The channels to exclude.", converter=convert_to_text_channels)):
        guildConfig = DBUtils.get_guild_config(inter.guild_id)
        included = []
        for channel in channels:
            channel: disnake.TextChannel
            if channel.id in guildConfig.react_remove_excluded_channels:
                guildConfig.react_remove_excluded_channels.remove(channel.id)
                included.append(channel.mention)
        guildConfig.save()
        if included:
            await inter.response.send_message(f"Re-included {', '.join(included)} in remove-reacts.")
        else:
            await inter.response.send_message("All channels are already included in remove-reacts.")

    @remove_reacts_channels.sub_command(name="list-excluded", description="List the channels excluded from remove-reacts.")
    async def remove_reacts_list_excluded(inter: ApplicationCommandInteraction):
        guildConfig = DBUtils.get_guild_config(inter.guild_id)
        excluded = []
        for channel_id in guildConfig.react_remove_excluded_channels:
            channel = inter.guild.get_channel(channel_id)
            if channel:
                excluded.append(channel.mention)
        if excluded:
            await inter.response.send_message(f"Excluded channels: {', '.join(excluded)}")
        else:
            await inter.response.send_message("No channels are excluded from remove-reacts.")


def setup(bot: commands.Bot):
    bot.add_cog(Moderation(bot))
