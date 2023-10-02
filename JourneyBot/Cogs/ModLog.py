import disnake # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database import DBUtils
from Util import Logging


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
        guild_config = DBUtils.get_guild_config(inter.guild_id)
        guild_config.guild_log = channel.id
        guild_config.save()
        await inter.response.send_message(f"Mod-Log channel set to {channel.mention}.")

    @commands.Cog.listener()
    async def on_member_join(self, member: disnake.Member):
        await Logging.guild_log(member.guild.id, f"{member.mention} (`{member.id}`) has joined the server.")


def setup(bot: commands.Bot):
    bot.add_cog(ModLog(bot))
