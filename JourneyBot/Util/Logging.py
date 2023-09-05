import logging
import disnake  # noqa

from Util import Configuration


LOGGER = logging.getLogger('journey-bot')
DISCORD_LOGGER = logging.getLogger('disnake')


def setup_logging():
    discord_level = logging.DEBUG if Configuration.is_dev_env() else logging.WARNING
    DISCORD_LOGGER.setLevel(discord_level)
    discord_handler = logging.FileHandler(filename='./logs/disnake.log', encoding='utf-8', mode='w')
    discord_handler.setFormatter(logging.Formatter('%(asctime)s:%(levelname)s:%(name)s: %(message)s'))
    DISCORD_LOGGER.addHandler(discord_handler)

    LOGGER.setLevel(logging.DEBUG)
    bot_handler = logging.FileHandler(filename='./logs/journey-bot.log', encoding='utf-8', mode='a')
    bot_handler.setFormatter(logging.Formatter('%(asctime)s:%(levelname)s: %(message)s'))
    LOGGER.addHandler(bot_handler)


def debug(message: str):
    LOGGER.debug(message)


def info(message: str):
    LOGGER.info(message)


def warning(message: str):
    LOGGER.warning(message)


def error(message: str):
    LOGGER.error(message)
