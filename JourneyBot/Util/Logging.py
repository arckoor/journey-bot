import logging
from logging.handlers import TimedRotatingFileHandler
import colorama
import disnake  # noqa

from Util import Configuration

colorama.init()

LOGGER = logging.getLogger("journey-bot")
DISCORD_LOGGER = logging.getLogger("disnake")

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


async def bot_log(message: str = None, embed: disnake.Embed = None):
    if BOT_LOG_CHANNEL is not None:
        await BOT_LOG_CHANNEL.send(content=message, embed=embed)


def debug(message: str):
    LOGGER.debug(message)


def info(message: str):
    LOGGER.info(message)


def warning(message: str):
    LOGGER.warning(message)


def error(message: str):
    LOGGER.error(message)
