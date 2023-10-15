import time
import datetime
import re

import disnake # noqa
from disnake import ApplicationCommandInteraction, Forbidden
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Views import Embed
from Views.Confirm import Confirm
from Util import Utils, Logging
from Util.Emoji import msg_with_emoji


async def convert_to_text_channels(inter: ApplicationCommandInteraction, arg: str) -> list[disnake.TextChannel]:
    channels = []
    for channel in re.split("[, ]+", arg):
        try:
            channel = await commands.TextChannelConverter().convert(inter, channel)
            channels.append(channel)
        except commands.ChannelNotFound:
            pass
    return channels


class ReactRemove(BaseCog):
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
        message_amount:          int = commands.Param(default=None, name="message-amount", description="The amount of messages to search through.", ge=1),
        remove_entire_react:    bool = commands.Param(default=False, name="remove-entire-react", description="Remove the entire reaction instead of removing the user's reaction."),
        greedy:                 bool = commands.Param(default=True, description="Greedily search for reactions even past the specified limit. Defaults to True.")
    ):
        if time_frame and message_amount:
            await inter.response.send_message("You can't specify both a time frame and a message amount.", ephemeral=True)
            return
        elif not time_frame and not message_amount:
            message_amount = 100
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
        guild_config = Utils.get_guild_config(inter.guild_id)
        if all_channels:
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
                if not history:
                    continue
                r_cnt, m_cnt, _ = await self.remove_from_history(user, history, remove_entire_react)
                reaction_cnt += r_cnt
                msg_cnt += m_cnt
                if greedy:
                    r_cnt, m_cnt = await self.greedy_remove(channel, user, remove_entire_react, history[-1], guild_config.react_remove_greedy_limit)
                    reaction_cnt += r_cnt
                    msg_cnt += m_cnt
            except Forbidden:
                await inter.channel.send(content=f"I am unable to read the message history in {channel.mention}.")
        end = time.perf_counter()
        await Logging.guild_log(
            inter.guild_id, msg_with_emoji("REACT", f"{user.name} (`{user.id}`) had {reaction_cnt} reaction(s) removed from {msg_cnt} message(s) by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"Searched through {msg_cnt} messages in {len(channels)} channel(s) and removed {reaction_cnt} reaction(s) from  {user}. Took {end - start} seconds.")
        if not inter.is_expired():
            await inter.followup.send(content=f"I searched through {msg_cnt} messages in {len(channels)} channel(s) and removed {reaction_cnt} reaction(s) from {user.mention}.")
        else:
            try:
                await thinking_id.delete()
            except Exception:
                pass
            await inter.channel.send(content=f"Removed {reaction_cnt} reactions from {user.mention} in {channel.mention}.")

    async def greedy_remove(
        self,
        channel: disnake.TextChannel,
        user: disnake.User,
        remove_entire_react: bool,
        last_msg: disnake.Message,
        greedy_limit: int
    ):
        reaction_cnt = 0
        msg_cnt = 0
        last_message = last_msg
        while True:
            try:
                history = await channel.history(limit=greedy_limit, before=last_message, oldest_first=False).flatten()
                r_cnt, m_cnt, last_message = await self.remove_from_history(user, history, remove_entire_react)
                reaction_cnt += r_cnt
                msg_cnt += m_cnt
                if last_message is None:
                    break
            except Forbidden:
                break
        return reaction_cnt, msg_cnt

    async def remove_from_history(self, user: disnake.User, history: list[disnake.Message], remove_entire_react: bool):
        reaction_cnt = 0
        msg_cnt = 0
        last_message = None
        for message in history:
            for reaction in message.reactions:
                if user in await reaction.users().flatten():
                    if remove_entire_react:
                        await reaction.clear()
                    else:
                        await reaction.remove(user)
                    reaction_cnt += 1
                    last_message = message
            msg_cnt += 1
        return reaction_cnt, msg_cnt, last_message

    @commands.slash_command(name="remove-reacts-config", description="Configure remove-reacts.", dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def rr_config(self, inter: ApplicationCommandInteraction):
        pass

    @rr_config.sub_command(name="show", description="Show the remove-reacts configuration.")
    async def rr_config_show(self, inter: ApplicationCommandInteraction):
        guild_config = Utils.get_guild_config(inter.guild_id)
        embed = Embed.default_embed(title="Remove Reacts Configuration", description="The current configuration for remove-reacts.", author=inter.author, icon_url=inter.author.avatar.url)
        embed.add_field(name="Greedy Limit", value=guild_config.react_remove_greedy_limit)
        if guild_config.react_remove_excluded_channels:
            embed.add_field(name="Excluded Channels", value="\n".join([inter.guild.get_channel(x).mention for x in guild_config.react_remove_excluded_channels]), inline=False)
        else:
            embed.add_field(name="Excluded Channels", value="None")
        await inter.response.send_message(embed=embed)

    @rr_config.sub_command(name="greedy-limit", description="Configure the greedy limit for remove-reacts.")
    async def rr_config_greedy_limit(self, inter: ApplicationCommandInteraction, limit: int = commands.Param(description="The greedy limit.")):
        guild_config = Utils.get_guild_config(inter.guild_id)
        guild_config.react_remove_greedy_limit = limit
        guild_config.save()
        await inter.response.send_message(f"Set the greedy limit to {limit}.")

    @rr_config.sub_command(name="silent-sweep-limit", description="Configure the silent sweep limit for remove-reacts.")
    async def rr_config_silent_sweep_limit(self, inter: ApplicationCommandInteraction, limit: int = commands.Param(description="The silent sweep limit.")):
        guild_config = Utils.get_guild_config(inter.guild_id)
        guild_config.react_remove_silent_sweep_limit = limit
        guild_config.save()
        await inter.response.send_message(f"Set the silent sweep limit to {limit}.")

    @rr_config.sub_command_group(name="channels", description="Configure the channels for remove-reacts.")
    async def rr_config_channels(self, inter: ApplicationCommandInteraction):
        pass

    @rr_config_channels.sub_command(name="exclude", description="Exclude channels from remove-reacts.")
    async def rr_config_channels_exclude(self, inter: ApplicationCommandInteraction, channels: str = commands.Param(description="The channels to exclude.", converter=convert_to_text_channels)):
        guild_config = Utils.get_guild_config(inter.guild_id)
        excluded = []
        for channel in channels:
            channel: disnake.TextChannel
            if channel.id not in guild_config.react_remove_excluded_channels:
                guild_config.react_remove_excluded_channels.append(channel.id)
                excluded.append(channel.mention)
        guild_config.save()
        if excluded:
            await inter.response.send_message(f"Excluded {', '.join(excluded)} from remove-reacts.")
        else:
            await inter.response.send_message("No further channels were excluded from remove-reacts.")

    @rr_config_channels.sub_command(name="include", description="Re-include channels in remove-reacts.")
    async def rr_config_channels_include(self, inter: ApplicationCommandInteraction, channels: str = commands.Param(description="The channels to exclude.", converter=convert_to_text_channels)):
        guild_config = Utils.get_guild_config(inter.guild_id)
        included = []
        for channel in channels:
            channel: disnake.TextChannel
            if channel.id in guild_config.react_remove_excluded_channels:
                guild_config.react_remove_excluded_channels.remove(channel.id)
                included.append(channel.mention)
        guild_config.save()
        if included:
            await inter.response.send_message(f"Re-included {', '.join(included)} in remove-reacts.")
        else:
            await inter.response.send_message("All channels are already included in remove-reacts.")

    async def silent_sweep(self, guild: disnake.Guild, user: disnake.User):
        guild_config = Utils.get_guild_config(guild.id)
        channels = [x for x in guild.text_channels if x.id not in guild_config.react_remove_excluded_channels]
        reaction_map = {}
        msg_cnt = 0
        reaction_cnt = 0
        for channel in channels:
            try:
                history = await channel.history(limit=guild_config.react_remove_silent_sweep_limit, oldest_first=False).flatten()
                if not history:
                    continue
                reaction_map[channel] = {}
                for message in history:
                    msg_cnt += 1
                    for reaction in message.reactions:
                        if user in await reaction.users().flatten():
                            if reaction.emoji not in reaction_map[channel]:
                                reaction_map[channel][reaction.emoji] = []
                            reaction_map[channel][reaction.emoji].append(reaction)
                            reaction_cnt += 1
            except Forbidden:
                pass
        reaction_map = {x: reaction_map[x] for x in reaction_map if reaction_map[x]}
        return reaction_map, msg_cnt, reaction_cnt

    @commands.Cog.listener()
    async def on_member_ban(self, guild: disnake.Guild, user: disnake.User):
        channel = guild.get_channel(Utils.get_guild_config(guild.id).guild_log)
        if not channel:
            return
        reaction_map, msg_cnt, reaction_cnt = await self.silent_sweep(guild, user)
        if not reaction_map:
            return

        async def yes(inter: disnake.Interaction):
            await inter.response.send_message(content="Okay, I will remove the reactions.")
            for c in reaction_map:
                for reaction in reaction_map[c]:
                    for react in reaction_map[c][reaction]:
                        await react.remove(user)
            await Logging.guild_log(
                inter.guild_id, msg_with_emoji("REACT", f"{user.name} (`{user.id}`) had {reaction_cnt} reaction(s) removed from {msg_cnt} message(s) by {inter.author.name} (`{inter.author.id}`)"))

        async def no(inter: disnake.Interaction):
            await inter.response.send_message(content="Okay, I won't remove the reactions.")

        embed = Embed.default_embed()
        for c in reaction_map:
            embed.add_field(name=c.mention, value="\n".join([f"{reaction} - {len(reaction_map[c][reaction])} reaction(s)" for reaction in reaction_map[c]]), inline=False)
        await channel.send(
            content=msg_with_emoji("BAN", f"{user.mention} was banned. Would you like to remove their reactions?"),
            embed=embed,
            view=Confirm(guild.id, yes, no, lambda *args: None, None)
        )


def setup(bot: commands.Bot):
    bot.add_cog(ReactRemove(bot))
