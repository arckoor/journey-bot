from twitchAPI.twitch import Twitch

from Util import Configuration, Logging

twitch: Twitch = None


async def initialize():
    global twitch
    if twitch:
        return
    config = Configuration.get_master_var("TWITCH_API")
    twitch = await Twitch(config.get("CLIENT_ID"), config.get("CLIENT_SECRET"))
    Logging.info("Twitch has been initialized.")


async def shutdown():
    global twitch
    if not twitch:
        return
    await twitch.close()


def get_twitch():
    global twitch
    if not twitch:
        initialize()
    return twitch
