import signal
import asyncio

from disnake import Intents, ApplicationInstallTypes, InteractionContextTypes

from aerich import Command

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


async def run_migrations():
    try:
        command = Command(
            tortoise_config=DBConnector.TORTOISE_ORM,
            app=DBConnector.app,
        )
        await command.init()
        result = await command.upgrade()
        if result:
            Logging.info("Migration completed successfully.")
            Logging.info(f"{result}")
        else:
            Logging.info("No migration needed.")
    except Exception as e:
        Logging.error(f"Error running migrations: {e}")
        exit()


if __name__ == "__main__":
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    loop.run_until_complete(run_migrations())
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
        moderation=True,
    )

    args = {
        "intents": intents,
        "default_install_types": ApplicationInstallTypes(guild=True, user=False),
        "default_contexts": InteractionContextTypes(bot_dm=False, guild=True, private_channel=False),
    }

    journeyBot = JourneyBot(**args)
    journeyBot.run(Configuration.get_master_var("BOT_TOKEN", ""))

    try:
        for sig_name in ("SIGINT", "SIGTERM"):
            loop.add_signal_handler(
                getattr(signal, sig_name),
                lambda: asyncio.ensure_future(journeyBot.close()),
            )
    except Exception:
        pass
    asyncio.run(shutdown())
    loop.close()
