from disnake import utils

from Util import Configuration, Logging

emojis = dict()

BACKUPS = {
    "BAN": "🚪",
    "JOIN": "📥",
    "REACT": "❌",
    "RSS":  "⇶",
    "STICKY": "📧",
    "WARN": "⚠"
}


async def initialize(bot):
    emoji_guild = await bot.fetch_guild(Configuration.get_master_var("EMOJI_GUILD"))
    failed = []
    for name, eid in Configuration.get_master_var("EMOJI", {}).items():
        e = utils.get(emoji_guild.emojis, id=eid)
        if e is not None:
            emojis[name] = e
        else:
            failed.append(name)

    if len(failed) > 0:
        await Logging.bot_log("Failed to load the following emoji: " + ",".join(failed))


def get_chat_emoji(name):
    return str(get_emoji(name))


def get_emoji(name):
    if name in emojis:
        return emojis[name]
    else:
        return BACKUPS[name]


def msg_with_emoji(name, msg):
    return f"{get_chat_emoji(name)} {msg}"
