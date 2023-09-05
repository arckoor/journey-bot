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
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+channel", "+current_id"]
    }


class RSSFeed(Document):
    guild = IntField(required=True)
    url = StringField(required=True)
    channel = IntField(required=True)
    initialized = BooleanField(required=True, default=False)
    already_sent = ListField(StringField(), required=False)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+url"]
    }


def init():
    connect(host=Configuration.get_master_var("MONGO_URI"))


def disconnect():
    disconnect_all()
