import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Util import Configuration, Logging


class Administration(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(description="Change the bot's presence.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def presence(
        self,
        inter: ApplicationCommandInteraction,
        type:    str = commands.Param(description="The type of activity", choices=["Playing", "Listening", "Watching", "Competing"]),
        message: str = commands.Param(description="The message to display")
    ):
        match type:
            case "Playing":
                activity = disnake.Activity(type=disnake.ActivityType.playing, name=message)
            case "Listening":
                activity = disnake.Activity(type=disnake.ActivityType.listening, name=message)
            case "Watching":
                activity = disnake.Activity(type=disnake.ActivityType.watching, name=message)
            case "Competing":
                activity = disnake.Activity(type=disnake.ActivityType.competing, name=message)
        await self.bot.change_presence(activity=activity)
        await inter.response.send_message("Presence changed.", ephemeral=True)

    @commands.slash_command(description="Restart the bot.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def restart(self, inter: ApplicationCommandInteraction):
        Logging.info(f"Restart requested by {inter.author.name}.")
        Logging.bot_log(f"Restart requested by {inter.author.name}.")
        await inter.response.send_message("Shutting down.", ephemeral=True)
        await self.bot.close()

    @commands.slash_command(description="Upgrade the bot.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def upgrade(self, inter: ApplicationCommandInteraction):
        file = open("upgradeRequest", "w")
        file.close()
        Logging.info(f"Upgrade requested by {inter.author.name}.")
        Logging.bot_log(f"Upgrade requested by {inter.author.name}.")
        await inter.response.send_message("Upgrading.", ephemeral=True)
        await self.bot.close()


def setup(bot: commands.Bot):
    bot.add_cog(Administration(bot))
