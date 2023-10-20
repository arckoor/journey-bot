import datetime
import time

import disnake # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Util import Emoji, Utils, Logging


class ModLog(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(name="mod-log", description="Mod-Log management", dm_permission=False)
    @commands.guild_only()
    @commands.bot_has_permissions(read_message_history=True, embed_links=True, send_messages=True, view_channel=True)
    @commands.default_member_permissions(ban_members=True)
    async def mod_log(self, inter: ApplicationCommandInteraction):
        pass

    @mod_log.sub_command_group(name="configure", description="Configure the mod-log.")
    async def ml_configure(self, inter: ApplicationCommandInteraction):
        pass

    @ml_configure.sub_command(name="channel", description="Set the mod-log channel.")
    async def ml_configure_channel(self, inter: ApplicationCommandInteraction, channel: disnake.TextChannel = commands.Param(description="The channel to set as the Mod-Log channel.")):
        perms = channel.permissions_for(inter.guild.me)
        if not perms.view_channel:
            await inter.response.send_message("I don't have permission to view that channel.", ephemeral=True)
            return
        elif not perms.send_messages:
            await inter.response.send_message("I don't have permission to send messages in that channel.", ephemeral=True)
            return
        elif not perms.embed_links:
            await inter.response.send_message("I don't have permission to embed links in that channel.", ephemeral=True)
            return
        guild_config = Utils.get_guild_config(inter.guild_id)
        guild_config.guild_log = channel.id
        guild_config.save()
        await inter.response.send_message(f"Mod-Log channel set to {channel.mention}.")

    @ml_configure.sub_command(name="new-threshold", description="Set the threshold for new users.")
    async def ml_configure_new_threshold(self, inter: ApplicationCommandInteraction, threshold: int = commands.param(description="The new user threshold (in days)", ge=1)):
        guild_config = Utils.get_guild_config(inter.guild_id)
        guild_config.new_user_threshold = threshold
        guild_config.save()
        await inter.response.send_message(f"New user threshold set to {threshold} days.")

    @commands.Cog.listener()
    async def on_member_join(self, member: disnake.Member):
        guild_config = Utils.get_guild_config(member.guild.id)
        dif = (datetime.datetime.utcfromtimestamp(time.time()).replace(
            tzinfo=datetime.timezone.utc) - member.created_at)
        new_user_threshold = datetime.timedelta(days=guild_config.new_user_threshold)
        minutes, _ = divmod(dif.days * 86400 + dif.seconds, 60)
        hours, minutes = divmod(minutes, 60)
        if dif.days > 0:
            age = f"{dif.days} days"
        else:
            age = f"{hours} hours, {minutes} minutes"
        await Logging.guild_log(
            member.guild.id, Emoji.msg_with_emoji("JOIN", f"{member.mention} (`{member.id}`) has joined the server, account created {age} ago. {':new:' if new_user_threshold > dif else ''}")
        )


def setup(bot: commands.Bot):
    bot.add_cog(ModLog(bot))
