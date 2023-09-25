from mongoengine import Document, StringField, IntField, BooleanField, ListField, connect, disconnect_all

from Util import Configuration


class StickyMessage(Document):
    author = IntField(required=True)
    channel = IntField(required=True)
    guild = IntField(required=True)
    content = StringField(required=True)
    last_sent = IntField(required=True)
    messages_since = IntField(required=True)
    active = BooleanField(required=True, default=True)
    current_id = IntField(required=False)
    message_limit = IntField(required=False, default=0)
    time_limit = IntField(required=False, default=0)
    delete_old_sticky = BooleanField(required=False, default=True)
    in_progress = BooleanField(required=False, default=False)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+channel", "+current_id"]
    }


class RSSFeed(Document):
    guild = IntField(required=True)
    url = StringField(required=True)
    channel = IntField(required=True)
    template = StringField(required=True)
    initialized = BooleanField(required=True, default=False)
    already_sent = ListField(StringField(), required=False)
    in_progress = BooleanField(required=False, default=False)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+url"]
    }


class GuildConfig(Document):
    guild = IntField(required=True)
    guild_log = IntField(required=False)
    react_remove_excluded_channels = ListField(IntField(), required=False)
    react_remove_greedy_limit = IntField(required=False, default=25)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+guild"]
    }


def init():
    connect(host=Configuration.get_master_var("MONGO_URI", ""))


def disconnect():
    disconnect_all()
