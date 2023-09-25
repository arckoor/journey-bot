import time

import disnake # noqa
from disnake import ApplicationCommandInteraction, Forbidden
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Util import Logging


class Basic(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(description="Ping the bot.")
    async def ping(self, inter: ApplicationCommandInteraction):
        latency = round(self.bot.latency * 1000, 2)
        t1 = time.perf_counter()
        await inter.response.send_message(f"Websocket ping is {latency} ms", ephemeral=True)
        t2 = time.perf_counter()
        rest = round((t2 - t1) * 1000)
        if not inter.is_expired():
            await inter.followup.send(content=f"REST API ping is {rest} ms", ephemeral=True)

    @commands.slash_command(dm_permission=False, description="Send a message as the bot.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def echo(self, inter: ApplicationCommandInteraction, message: str = commands.Param(description="The message to send.")):
        try:
            await inter.channel.send(message.replace("\\n", "\n"))
        except Forbidden:
            await inter.response.send_message("I don't have permission to send messages in that channel.", ephemeral=True)
            return
        except Exception as e:
            Logging.error(f"Failed to echo message: {e}")
            await inter.response.send_message("Something went wrong while trying to send the message.", ephemeral=True)
            return
        await inter.response.send_message("Message sent.", ephemeral=True)


def setup(bot: commands.Bot):
    bot.add_cog(Basic(bot))
