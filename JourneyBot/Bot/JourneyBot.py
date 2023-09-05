import disnake  # noqa
from disnake.ext import commands
from disnake.ext.commands import ExtensionAlreadyLoaded

from Util import Configuration, Logging


class JourneyBot(commands.Bot):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.loaded = False
        self.shutting_down = False

    async def on_ready(self):
        if not self.loaded:
            for extension in Configuration.get_master_var("COGS"):
                try:
                    Logging.info(f"Loading {extension} cog.")
                    self.load_extension(f"Cogs.{extension}")
                except ExtensionAlreadyLoaded:
                    pass
                except Exception as e:
                    Logging.error(f"Failed to load cog {extension}: {e}")
            Logging.info("Successfully logged in and ready.")
            self.loaded = True

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
