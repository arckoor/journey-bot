import signal
import asyncio

from disnake import Intents

from Bot.JourneyBot import JourneyBot
from Database import DBConnector
from Util import Configuration, Logging, Reddit, Twitch


async def startup():
    Logging.setup_logging()
    await DBConnector.connect()
    Reddit.initialize()
    await Twitch.initialize()


async def shutdown():
    await Reddit.shutdown()
    await Twitch.shutdown()
    await DBConnector.disconnect()


if __name__ == "__main__":
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    loop.run_until_complete(startup())
    Logging.info("--------------")
    Logging.info("Starting up.")

    intents = Intents(
        guilds=True,
        members=True,
        emojis=True,
        messages=True,
        reactions=True,
        message_content=True,
        moderation=True
    )

    args = {
        "intents": intents,
    }

    journeyBot = JourneyBot(**args)
    journeyBot.run(Configuration.get_master_var("BOT_TOKEN", ""))

    try:
        for sig_name in ("SIGINT", "SIGTERM"):
            loop.add_signal_handler(getattr(signal, sig_name), lambda: asyncio.ensure_future(journeyBot.close()))
    except Exception:
        pass
    asyncio.run(shutdown())
    loop.close()
