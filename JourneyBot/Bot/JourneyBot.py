import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands
from disnake.ext.commands import ExtensionAlreadyLoaded, errors

from Util import Configuration, Logging


class JourneyBot(commands.Bot):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.loaded = False
        self.shutting_down = False

    async def on_ready(self):
        Logging.BOT_LOG_CHANNEL = self.get_channel(Configuration.get_master_var("BOT_LOG_CHANNEL"))
        if not self.loaded:
            for extension in Configuration.get_master_var("COGS", []):
                try:
                    Logging.info(f"Loading {extension} cog.")
                    self.load_extension(f"Cogs.{extension}")
                except ExtensionAlreadyLoaded:
                    pass
                except Exception as e:
                    Logging.error(f"Failed to load cog {extension}: {e}")
            Logging.info("Successfully logged in and ready.")
            self.loaded = True
            await Logging.bot_log("Successfully logged in and ready.")

    async def close(self):
        if not self.shutting_down:
            self.shutting_down = True
            Logging.info("Shutting down.")
            t = []
            for cog in self.cogs:
                t.append(cog)
            for cog in t:
                c = self.get_cog(cog)
                if hasattr(c, "close"):
                    await c.close()
                self.unload_extension(f"Cogs.{cog}")
        return await super().close()

    async def on_slash_command_error(self, inter: ApplicationCommandInteraction, exception: errors.CommandError) -> None:
        if isinstance(exception, errors.NotOwner):
            await inter.response.send_message("You are not the owner of this bot.", ephemeral=True)
        elif isinstance(exception, errors.BotMissingPermissions):
            await inter.response.send_message("I'm missing permissions needed to run this command: " + str(exception))
        elif isinstance(exception, errors.MissingPermissions):
            await inter.response.send_message("You don't have permission to use this command.", ephemeral=True)
        return await super().on_slash_command_error(inter, exception)
