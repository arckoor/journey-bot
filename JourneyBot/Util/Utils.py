import io
import datetime

from dataclasses import dataclass

import disnake # noqa
from disnake.ext.commands import InteractionBot


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


def get_alternate_channel(id: int = None, name: str = None, mention: str = None, guild: dict = None) -> Channel:
    id = coalesce(id, 0)
    name = coalesce(name, "Unknown")
    mention = coalesce(mention, "<#Unknown>")
    if not guild:
        guild = Guild(id=0, name="Unknown")
    else:
        guild = Guild(**guild)
    return Channel(id=id, name=name, mention=mention, guild=guild)


def make_file(bot: InteractionBot, channel_name, messages) -> disnake.File:
    timestamp = datetime.datetime.strftime(datetime.datetime.now(tz=datetime.timezone.utc), "%H:%M:%S")
    out = f"recorded spam messages at {timestamp} in {channel_name}\n"
    for msg in messages:
        message = bot.get_message(msg.id)
        if not message:
            continue
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


def time_to_text(t1: float, t2: float):
    diff = t2 - t1
    days, remainder = divmod(diff, 86400)
    hours, remainder = divmod(remainder, 3600)
    minutes, _ = divmod(remainder, 60)
    formatted = ""
    if days:
        formatted += f"{days:.0f} day{'s' if days > 1 else ''}, "
    if hours or days:
        formatted += f"{hours:02.0f} hour{'s' if hours > 1 else ''}, "
    if minutes or not (days or hours):
        formatted += f"{minutes:02.0f} minute{'s' if minutes > 1 else ''}"

    return formatted
