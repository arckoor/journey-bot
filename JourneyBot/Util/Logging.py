import traceback
import logging
from logging.handlers import TimedRotatingFileHandler
import colorama

import disnake  # noqa
from disnake.ext import commands

from Util import Configuration
from Database import DBUtils

colorama.init()

LOGGER = logging.getLogger("journey-bot")
DISCORD_LOGGER = logging.getLogger("disnake")

BOT: commands.Bot = None
BOT_LOG_CHANNEL = None


class ColoredFormatter(logging.Formatter):
    def __init__(self, fmt, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.colors = {
            "DEBUG":    colorama.Fore.CYAN,
            "INFO":     colorama.Fore.GREEN,
            "WARNING":  colorama.Fore.YELLOW,
            "ERROR":    colorama.Fore.RED,
            "CRITICAL": colorama.Fore.RED,
        }
        self.fmt = fmt

    def format(self, record):
        log_fmt = self.colors[record.levelname] + self.fmt.replace(" -", colorama.Style.RESET_ALL + " -") + colorama.Style.RESET_ALL
        formatter = logging.Formatter(log_fmt)
        return formatter.format(record)


def setup_logging():
    discord_level = logging.DEBUG if Configuration.is_dev_env() else logging.WARNING
    DISCORD_LOGGER.setLevel(discord_level)
    discord_handler = logging.FileHandler(filename="./logs/disnake.log", encoding="utf-8", mode="w")
    discord_handler.setFormatter(ColoredFormatter("[%(asctime)s] [%(levelname)s] [%(name)s] - %(message)s"))
    DISCORD_LOGGER.addHandler(discord_handler)

    LOGGER.setLevel(logging.DEBUG)
    bot_handler = logging.FileHandler(filename="./logs/journey-bot.log", encoding="utf-8", mode="a")
    bot_handler = TimedRotatingFileHandler(filename="logs/journey-bot.log", when="midnight", backupCount=30, encoding="utf-8")
    bot_handler.setFormatter(ColoredFormatter("[%(asctime)s] [%(levelname)s] - %(message)s"))
    LOGGER.addHandler(bot_handler)


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


async def guild_log(guild_id: int, message: str = None, embed: disnake.Embed = None):
    global BOT
    guild_config = DBUtils.get_guild_config(guild_id)
    if guild_config.guild_log is not None:
        channel = BOT.get_channel(guild_config.guild_log)
        if channel is not None:
            return await channel.send(content=message, embed=embed)


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
    BOT.loop.create_task(bot_log(embed=disnake.Embed(title="Exception", description=f"```Traceback:\n{trace}\n{str(error)}```", color=disnake.Color.red())))
