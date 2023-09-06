import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Util import Configuration


class Administration(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(guild_ids=[Configuration.get_master_var("ADMIN_GUILD")])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def presence(self, inter: ApplicationCommandInteraction, type: int, status: str):
        """
        Change the bot's presence.

        Parameters
        ----------
        type: int
            1 - Playing, 2 - Listening, 3 - Watching, 5 - Competing
        status: str
            The status to display.
        """
        await self.bot.change_presence(activity=disnake.Activity(type=type, name=status))
        await inter.response.send_message("Presence changed.", ephemeral=True)

    @commands.slash_command(guild_ids=[Configuration.get_master_var("ADMIN_GUILD")])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def restart(self, inter: ApplicationCommandInteraction):
        """
        Restart the bot.
        """
        await inter.response.send_message("Shutting down.", ephemeral=True)
        await self.bot.close()


def setup(bot: commands.Bot):
    bot.add_cog(Administration(bot))
