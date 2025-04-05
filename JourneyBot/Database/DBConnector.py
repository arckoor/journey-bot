from tortoise import Tortoise, connections
from tortoise.models import Model
from tortoise.fields import (
    BooleanField,
    UUIDField,
    IntField,
    BigIntField,
    TextField,
    FloatField,
    DatetimeField,
    ForeignKeyField,
    ForeignKeyRelation,
    ReverseRelation,
)
from tortoise.contrib.postgres.fields import ArrayField

import datetime

from Util import Configuration


db_model = "Database.DBConnector"
app = "journey-bot"

TORTOISE_ORM = {
    "connections": {
        "default": {
            "engine": "tortoise.backends.asyncpg",
            "credentials": {
                "host": Configuration.get_master_var("DATABASE_HOST"),
                "port": "5432",
                "user": "journey-bot",
                "password": Configuration.get_master_var("DATABASE_PASSWORD"),
                "database": "journey-bot-db",
            },
        },
    },
    "apps": {app: {"models": [db_model, "aerich.models"]}},
    "use_tz": False,
    "timezone": "UTC",
}


async def connect():
    await Tortoise.init(TORTOISE_ORM)


async def disconnect():
    await connections.close_all()


class AbstractUUIDModel(Model):
    id = UUIDField(pk=True)

    class Meta:
        abstract = True


class AbstractIncrModel(Model):
    id = IntField(pk=True)

    class Meta:
        abstract = True


class GuildIDMixin:
    guild = BigIntField(db_index=True)


class UniqueGuildIDMixin:
    guild = BigIntField(db_index=True, unique=True)


class StickyMessage(AbstractUUIDModel, GuildIDMixin):
    channel = BigIntField(db_index=True, unique=True)
    author = BigIntField()
    content = TextField()
    last_sent = FloatField()
    messages_since = IntField()
    active = BooleanField(default=True)
    current_id = BigIntField(null=True)
    message_limit = IntField(default=0)
    time_limit = IntField(default=0)
    delete_old_sticky = BooleanField(default=True)


class RedditFeed(AbstractUUIDModel, GuildIDMixin):
    channel = BigIntField()
    subreddit = TextField()
    template = TextField()
    latest_post = DatetimeField(auto_now_add=True)


class StreamObserver(AbstractUUIDModel, GuildIDMixin):
    channel = BigIntField()
    game_id = TextField()
    game_name = TextField()
    template = TextField()
    end_template = TextField()
    blacklist = ArrayField(element_type="text", default=[])

    known_streams: ReverseRelation["KnownStream"]


class KnownStream(AbstractIncrModel):
    stream_id = TextField()
    user_id = TextField()
    user_login = TextField()
    last_seen = DatetimeField(auto_now_add=True)
    message_id = BigIntField(null=True)
    stream_observer: ForeignKeyRelation[StreamObserver] = ForeignKeyField(
        f"{app}.StreamObserver", related_name="known_streams"
    )

    class Meta:
        unique_together = (("stream_id", "stream_observer"),)


class EnsuredRole(AbstractIncrModel, GuildIDMixin):
    role = BigIntField(db_index=True)

    class Meta:
        unique_together = (("guild", "role"),)


class GuildConfig(AbstractIncrModel, UniqueGuildIDMixin):
    guild_log = BigIntField(null=True)
    time_zone = TextField(default="UTC")
    onboarding_active_since = DatetimeField(default=datetime.datetime.fromtimestamp(0))
    react_remove_excluded_channels = ArrayField(element_type="bigint", default=[])
    react_remove_greedy_limit = IntField(default=25)
    new_user_threshold = IntField(default=14)


class AntiSpamConfig(AbstractIncrModel, UniqueGuildIDMixin):
    enabled = BooleanField(default=False)
    punishment = TextField(default="mute")
    timeout_duration = IntField(null=True)
    clean_user = BooleanField(default=False)
    max_messages = ArrayField(element_type="int", default=[5])
    similar_message_threshold = ArrayField(element_type="float", default=[0.95])
    similar_message_re_ban_threshold = FloatField(default=0.95)
    time_frame = IntField(default=300)
    trusted_users = ArrayField(element_type="bigint", default=[])
    trusted_roles = ArrayField(element_type="bigint", default=[])
    ignored_channels = ArrayField(element_type="bigint", default=[])

    recently_punished: ReverseRelation["PunishedMessage"]


class PunishedMessage(AbstractIncrModel, GuildIDMixin):
    content = TextField()
    timestamp = FloatField()
    anti_spam_config: ForeignKeyRelation[AntiSpamConfig] = ForeignKeyField(
        f"{app}.AntiSpamConfig", related_name="recently_punished"
    )

    class Meta:
        unique_together = (("guild", "content"),)


class RedditAutoReply(AbstractIncrModel, GuildIDMixin):
    subreddit = TextField()
    latest_post = DatetimeField(auto_now_add=True)
    management_role = BigIntField()

    flairs: ReverseRelation["RedditFlair"]


class RedditFlair(AbstractIncrModel, GuildIDMixin):
    flair_name = TextField()
    flair_reply = TextField()
    auto_reply: ForeignKeyRelation[RedditAutoReply] = ForeignKeyField(f"{app}.RedditAutoReply", related_name="flairs")


async def get_guild_config(id: int) -> GuildConfig:
    config, _ = await GuildConfig.get_or_create(guild=id)
    return config


async def get_anti_spam_config(id: int) -> AntiSpamConfig:
    config, _ = await AntiSpamConfig.get_or_create(guild=id)
    await config.fetch_related("recently_punished")
    return config
