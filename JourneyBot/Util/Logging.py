import sys
import traceback
import datetime
import zoneinfo
import logging
from logging.handlers import TimedRotatingFileHandler
import colorama

import disnake
from disnake import Forbidden
from disnake.ext import commands

from Util import Configuration, Utils
from Database.DBConnector import get_guild_config

colorama.init()

LOGGER = logging.getLogger("journey-bot")
POOL_LOGGER = logging.getLogger("pool")
DISCORD_LOGGER = logging.getLogger("disnake")

BOT: commands.Bot = None
BOT_LOG_CHANNEL = None


class ColoredFormatter(logging.Formatter):
    def __init__(self, fmt, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.colors = {
            "DEBUG": colorama.Fore.CYAN,
            "INFO": colorama.Fore.GREEN,
            "WARNING": colorama.Fore.YELLOW,
            "ERROR": colorama.Fore.RED,
            "CRITICAL": colorama.Fore.RED,
        }
        self.fmt = fmt

    def format(self, record):
        log_fmt = (
            self.colors[record.levelname]
            + self.fmt.replace(" -", colorama.Style.RESET_ALL + " -")
            + colorama.Style.RESET_ALL
        )
        formatter = logging.Formatter(log_fmt)
        return formatter.format(record)


def setup_logging():
    discord_level = logging.DEBUG if Configuration.is_dev_env() else logging.WARNING
    DISCORD_LOGGER.setLevel(discord_level)
    discord_handler = logging.FileHandler(filename="./logs/disnake.log", encoding="utf-8", mode="w")
    discord_handler.setFormatter(ColoredFormatter("[%(asctime)s] [%(levelname)s] [%(name)s] - %(message)s"))
    DISCORD_LOGGER.addHandler(discord_handler)

    LOGGER.setLevel(logging.DEBUG)
    bot_handler = TimedRotatingFileHandler(
        filename="logs/journey-bot.log",
        when="midnight",
        backupCount=30,
        encoding="utf-8",
    )
    bot_handler.setFormatter(ColoredFormatter("[%(asctime)s] [%(levelname)s] - %(message)s"))
    LOGGER.addHandler(bot_handler)
    if Configuration.is_dev_env():
        stdout_handler = logging.StreamHandler(stream=sys.stdout)
        stdout_handler.setLevel(logging.WARNING)
        LOGGER.addHandler(stdout_handler)

    POOL_LOGGER.setLevel(logging.DEBUG)
    pool_handler = TimedRotatingFileHandler(filename="logs/pool.log", when="midnight", backupCount=7, encoding="utf-8")
    pool_handler.setFormatter(ColoredFormatter("[%(asctime)s] [%(levelname)s] - %(message)s"))
    POOL_LOGGER.addHandler(pool_handler)


async def initialize(bot: commands.Bot, log_channel_id: str):
    global BOT_LOG_CHANNEL, BOT
    BOT = bot
    BOT_LOG_CHANNEL = bot.get_channel(int(log_channel_id))
    if BOT_LOG_CHANNEL is None:
        LOGGER.error("-----Failed to get logging channel, aborting startup!-----")
        await bot.close()


async def bot_log(message: str = None, embed: disnake.Embed = None):
    global BOT_LOG_CHANNEL
    if BOT_LOG_CHANNEL is not None:
        return await BOT_LOG_CHANNEL.send(content=message, embed=embed)


async def guild_log(
    guild_id: int,
    message: str = None,
    embed: disnake.Embed = None,
    file: disnake.File = None,
):
    global BOT
    guild_config = await get_guild_config(guild_id)
    timezone = zoneinfo.ZoneInfo(Utils.coalesce(guild_config.time_zone, "UTC"))
    timestamp = datetime.datetime.strftime(datetime.datetime.now(tz=timezone), "%H:%M:%S")
    if guild_config.guild_log is not None:
        channel = BOT.get_channel(guild_config.guild_log)
        if channel is not None:
            try:
                return await channel.send(content=f"[`{timestamp}`]  " + message, embed=embed, file=file)
            except Forbidden:
                LOGGER.error(f"Failed to send guild log message to {channel.id} in guild {guild_id}.")


def debug(message: str):
    LOGGER.debug(message)


def info(message: str):
    LOGGER.info(message)


def warning(message: str):
    LOGGER.warning(message)


def error(message: str):
    LOGGER.error(message)


def exception(message: str, error: Exception):
    global BOT
    LOGGER.error(message)
    trace = ""
    LOGGER.error(str(error))
    for line in traceback.format_tb(error.__traceback__):
        line = line.replace("\t", "", 1)
        trace = f"{trace}\n{line}"
    LOGGER.error(trace)
    BOT.loop.create_task(
        bot_log(
            embed=disnake.Embed(
                title="Exception",
                description=f"```Traceback:\n{trace}\n{str(error)}```",
                color=disnake.Color.red(),
            )
        )
    )


def pool_log(message: str):
    POOL_LOGGER.info(message)
