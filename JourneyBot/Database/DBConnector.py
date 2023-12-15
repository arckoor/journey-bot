from prisma import Prisma, models


db = None


async def connect():
    global db
    db = Prisma()
    await db.connect()


async def disconnect():
    global db
    await db.disconnect()


async def get_guild_config(id: int) -> models.GuildConfig:
    global db
    return await db.guildconfig.upsert(
        where={
            "guild": id
        },
        data={
            "create": {
                "guild": id
            },
            "update": {
            }
        },
    )


async def get_anti_spam_config(id: int) -> models.AntiSpamConfig:
    global db
    return await db.antispamconfig.upsert(
        where={
            "guild": id
        },
        include={
            "recently_punished": True
        },
        data={
            "create": {
                "guild": id
            },
            "update": {
            }
        }
    )
