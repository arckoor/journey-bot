import io
import datetime
import typing
from dataclasses import dataclass

import disnake # noqa
from disnake import ApplicationCommandInteraction

from enum import Enum
from Database.DBConnector import SupportedDocumentType, GuildConfig


class ValidationType(Enum):
    INVALID_ID = 0
    ID_NOT_FOUND = 1
    NOT_IN_CHANNEL = 2
    OK = 3


@dataclass
class Guild:
    id: int = 0
    name: str = ""


@dataclass
class Channel:
    id: int = 0
    name: str = ""
    mention: str = ""
    guild: Guild = None


def is_hex(hex_string: str):
    try:
        int(hex_string, 16)
        return True
    except ValueError:
        return False


def is_object_id(object_id: str):
    return len(object_id) == 24 and is_hex(object_id)


def get_document_from_id_or_channel(
    type: SupportedDocumentType,
    inter: ApplicationCommandInteraction,
    id: str = None
) -> tuple[SupportedDocumentType, typing.Literal[ValidationType.OK]] | tuple[None, typing.Literal[ValidationType.INVALID_ID, ValidationType.ID_NOT_FOUND, ValidationType.NOT_IN_CHANNEL]]:
    if id:
        if not is_object_id(id):
            return None, ValidationType.INVALID_ID
        if not type.objects(id=id, guild=inter.guild_id):
            return None, ValidationType.ID_NOT_FOUND
        document = type.objects(id=id).first()
    else:
        if not type.objects(channel=inter.channel.id):
            return None, ValidationType.NOT_IN_CHANNEL
        document = type.objects(channel=inter.channel.id).first()
    return document, ValidationType.OK


def get_guild_config(id: int) -> GuildConfig:
    if not GuildConfig.objects(guild=id):
        guild = GuildConfig(guild=id)
        guild.save()
        return guild
    return GuildConfig.objects(guild=id).first()


def get_alternate_channel(id: int = None, name: str = None, mention: str = None, guild: dict = None) -> Channel:
    id = coalesce(id, 0)
    name = coalesce(name, "Unknown")
    mention = coalesce(mention, "<#Unknown>")
    if not guild:
        guild = Guild(id=0, name="Unknown")
    else:
        guild = Guild(**guild)
    return Channel(id=id, name=name, mention=mention, guild=guild)


def make_file(bot, channel_name, messages) -> disnake.File:
    timestamp = datetime.datetime.strftime(datetime.datetime.now(tz=datetime.timezone.utc), "%H:%M:%S")
    out = f"recorded spam messages at {timestamp} in {channel_name}\n"
    for msg in messages:
        message = bot.get_message(msg.id)
        name = message.author.name
        reply = ""
        if message.reference is not None:
            reply = f" | In reply to https://discord.com/channels/{message.guild.id}/{message.channel.id}/{message.reference.message_id}"
        timestamp = datetime.datetime.strftime(disnake.Object(message.id).created_at.astimezone(tz=datetime.timezone.utc), "%H:%M:%S")
        out += f"{timestamp} {message.guild.id} - {message.channel.id} - {message.id} | {name} ({message.author.id}) | {message.content}{reply}\r\n"
    buffer = io.BytesIO()
    buffer.write(out.encode("utf-8"))
    buffer.seek(0)
    return disnake.File(buffer, filename="Spam messages archive.txt")


# https://stackoverflow.com/a/16247152/12203337
def coalesce(*args):
    return next((a for a in args if a is not None), None)
