from mongoengine import Document, StringField, IntField, FloatField, BooleanField, ListField, DateTimeField, connect, disconnect_all

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
    latest_post = DateTimeField(required=False)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+url"]
    }


class GuildConfig(Document):
    guild = IntField(required=True)
    guild_log = IntField(required=False)
    react_remove_excluded_channels = ListField(IntField(), required=False)
    react_remove_greedy_limit = IntField(required=False, default=25)
    react_remove_silent_sweep_limit = IntField(required=False, default=25)
    new_user_threshold = IntField(required=False, default=14)
    anti_spam_enabled = BooleanField(required=False, default=False)
    anti_spam_punishment = StringField(required=False, default="mute")
    anti_spam_max_messages = IntField(required=False, default=5)
    anti_spam_similar_message_threshold = FloatField(required=False, default=0.95)
    anti_spam_time_frame = IntField(required=False, default=300)
    trusted_users = ListField(IntField(), required=False)
    trusted_roles = ListField(IntField(), required=False)
    mute_role = IntField(required=False)
    meta = {
        "auto_create_index_on_save": False,
        "indexes": ["+guild"]
    }


SupportedDocumentType = StickyMessage | RSSFeed


def init():
    connect(host=Configuration.get_master_var("MONGO_URI", ""))


def disconnect():
    disconnect_all()
