import io
import sys

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
        await Logging.bot_log(f"Restart requested by {inter.author.name}.")
        await inter.response.send_message("Shutting down.", ephemeral=True)
        await self.bot.close()

    @commands.slash_command(description="Upgrade the bot.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def upgrade(self, inter: ApplicationCommandInteraction):
        file = open("upgradeRequest", "w")
        file.close()
        Logging.info(f"Upgrade requested by {inter.author.name}.")
        await Logging.bot_log(f"Upgrade requested by {inter.author.name}.")
        await inter.response.send_message("Upgrading.", ephemeral=True)
        await self.bot.close()

    @commands.slash_command(description="Cog management.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def cog(self, inter: ApplicationCommandInteraction):
        pass

    @cog.sub_command(description="Reload a cog.")
    async def reload(
        self,
        inter: ApplicationCommandInteraction,
        cog: str = commands.Param(description="The cog to reload."),
        now: bool = commands.Param(description="Reload immediately.", default=False)
    ):
        cogs = []
        for c in self.bot.cogs:
            cogs.append(c.replace("Cog", ""))

        if cog in cogs:
            await inter.response.defer(ephemeral=True)
            c = self.bot.get_cog(cog)
            if hasattr(c, "close") and not now:
                await c.close()
            self.bot.unload_extension(f"Cogs.{cog}")
            self.bot.load_extension(f"Cogs.{cog}")
            await inter.edit_original_response(f"**{cog}** has been reloaded.")
            await Logging.bot_log(f"**{cog}** has been reloaded by {inter.author.name}.")
        else:
            await inter.response.send_message("I can't find that cog.", ephemeral=True)

    @commands.slash_command(description="Run any code")
    @commands.is_owner()
    @commands.default_member_permissions(manage_guild=True)
    async def eval(self, inter: ApplicationCommandInteraction, code: str = commands.Param(description="The code to run.")):
        try:
            exec(f"async def __ex(self, inter): {code}")
            stdout_buffer = io.StringIO()
            sys.stdout = stdout_buffer

            try:
                output = await locals()["__ex"](self, inter)
                stdout_output = stdout_buffer.getvalue()
                await inter.response.send_message(f"```STDOUT:\n{stdout_output}```\n```OUTPUT:\n{output}```")
            except Exception as e:
                await inter.response.send_message(f"```{e}```", ephemeral=True)

            sys.stdout = sys.__stdout__
        except Exception as e:
            await inter.response.send_message(f"```{e}```", ephemeral=True)


def setup(bot: commands.Bot):
    bot.add_cog(Administration(bot))
