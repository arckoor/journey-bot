import disnake  # noqa
from disnake.ext import commands


class BaseCog(commands.Cog):
    def __init__(self, bot: commands.Bot):
        self.bot = bot
