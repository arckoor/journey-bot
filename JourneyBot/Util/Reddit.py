import asyncpraw

from Util import Configuration, Logging

reddit: asyncpraw.Reddit = None


def initialize():
    global reddit
    if reddit:
        return
    config = Configuration.get_master_var("REDDIT_API")
    reddit = asyncpraw.Reddit(
        client_id=config.get("CLIENT_ID"),
        client_secret=config.get("CLIENT_SECRET"),
        user_agent=config.get("USER_AGENT"),
        username=config.get("USERNAME"),
        password=config.get("PASSWORD"),
    )
    Logging.info("asyncpraw has been initialized.")


async def shutdown():
    global reddit
    if not reddit:
        return
    await reddit.close()


def get_reddit():
    global reddit
    if not reddit:
        initialize()
    return reddit
