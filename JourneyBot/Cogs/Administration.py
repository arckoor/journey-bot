import io
import sys
import textwrap

import disnake
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import *  # noqa, needed for eval
from Util import Configuration, Logging


class Administration(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(
        description="Change the bot's presence.",
        guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)],
    )
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def presence(
        self,
        inter: ApplicationCommandInteraction,
        type: str = commands.Param(
            description="The type of activity",
            choices=["Playing", "Listening", "Watching", "Competing"],
        ),
        message: str = commands.Param(description="The message to display"),
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

    @commands.slash_command(
        description="Restart the bot.",
        guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)],
    )
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def restart(self, inter: ApplicationCommandInteraction):
        Logging.info(f"Restart requested by {inter.author.name}.")
        await Logging.bot_log(f"Restart requested by {inter.author.name}.")
        await inter.response.send_message("Shutting down.", ephemeral=True)
        await self.bot.close()

    @commands.slash_command(
        description="Cog management.",
        guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)],
    )
    @commands.is_owner()
    @commands.default_member_permissions(administrator=True)
    async def cog(self, inter: ApplicationCommandInteraction):
        pass

    @cog.sub_command(description="Reload a cog.")
    async def reload(
        self,
        inter: ApplicationCommandInteraction,
        cog: str = commands.Param(description="The cog to reload."),
        now: bool = commands.Param(description="Reload immediately.", default=False),
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

    @commands.slash_command(description="Run any code.")
    @commands.is_owner()
    @commands.default_member_permissions(manage_guild=True)
    async def eval(
        self,
        inter: ApplicationCommandInteraction,
        code: str = commands.Param(description="The code to run.", default=None),
        message_id: str = commands.Param(description="ID of the message with the code to run.", default=None),
    ):
        Logging.info(f"Eval requested by {inter.author.name} ({inter.author.id}).")
        await Logging.bot_log(f"Eval requested by {inter.author.name} ({inter.author.id}).")
        if not code and not message_id:
            await inter.response.send_message("You must provide either code or a message ID.", ephemeral=True)
            return
        elif message_id and code:
            await inter.response.send_message("You can't provide both code and message ID.", ephemeral=True)
            return

        if message_id:
            try:
                message_id = int(message_id)
                message = await inter.channel.fetch_message(message_id)
            except Exception as e:
                await inter.response.send_message(f"Invalid message ID: {e}", ephemeral=True)
                return
            if not message:
                await inter.response.send_message("Message not found.", ephemeral=True)
            code = message.content
        if code.startswith("```") and code.endswith("```"):
            code = "\n".join(code.split("\n")[1:-1])

        to_compile = f"async def __ex(self, inter):\n{textwrap.indent(code, '  ')}"
        try:
            exec(to_compile)
        except Exception as e:
            await inter.response.send_message(f"Could not compile: {e.__class__.__name__}: {e}", ephemeral=True)
            return

        stdout_buffer = io.StringIO()
        sys.stdout = stdout_buffer

        try:
            output = await locals()["__ex"](self, inter)
            stdout_output = stdout_buffer.getvalue()

            res = ""
            if output is not None:
                res = f"OUTPUT:\n```{output}```"
            if stdout_output:
                res += f"STDOUT:\n```{stdout_output}```"

            if not res:
                res = "Done."

            await inter.response.send_message(res)
        except Exception as e:
            await inter.response.send_message(f"{e.__class__.__name__}: {e}", ephemeral=True)

        sys.stdout = sys.__stdout__


def setup(bot: commands.Bot):
    bot.add_cog(Administration(bot))
