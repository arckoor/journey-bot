from time import time
import asyncio

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import db
from prisma.models import StickyMessage
from Views import Embed
from Util import Configuration, Utils, Logging
from Util.Emoji import msg_with_emoji


class Sticky(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        config = Configuration.get_master_var("STICKY", {"MAX_MESSAGES": 5, "MIN_TIME": 15})
        self.max_messages = config.get("MAX_MESSAGES")
        self.min_time = config.get("MIN_TIME")
        self.locks = {}

    @commands.slash_command(dm_permission=False, description="Sticky message management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def stick(self, inter: ApplicationCommandInteraction):
        pass

    @stick.sub_command(description="List all stickies in the server.")
    async def list(self, inter: ApplicationCommandInteraction):
        stickies = await db.stickymessage.find_many(
            where={
                "guild": inter.guild_id
            }
        )
        if not stickies:
            await inter.response.send_message("No stickies found.", ephemeral=True)
            return
        embed = Embed.default_embed(
            title="Stickies",
            description="All stickies in this server.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        for stickyMessage in stickies:
            channel = self.bot.get_channel(stickyMessage.channel)
            if not channel:
                channel = Utils.get_alternate_channel(stickyMessage.channel)
            user = self.bot.get_user(stickyMessage.author)
            if user and user.name:
                user_name = user.name
            else:
                user_name = "Unknown"
            stopped = "" if stickyMessage.active else " (stopped)"
            embed.add_field(name=f"#{channel.name} by @{user_name}{stopped} | ID: {stickyMessage.id}", value=f"{stickyMessage.content}", inline=False)
        await inter.response.send_message(embed=embed)

    @stick.sub_command(description="Show some info about the sticky in this channel.")
    async def info(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=36, max_length=36)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            await inter.response.send_message("The specified sticky message was not found.", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(stickyMessage.channel), Utils.get_alternate_channel(stickyMessage.channel))
        embed = Embed.default_embed(
            title="Sticky Info",
            description="Info about the sticky in a channel.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="ID", value=f"{stickyMessage.id}")
        embed.add_field(name="Content", value=f"{stickyMessage.content}", inline=False)
        embed.add_field(name="Author", value=f"<@{stickyMessage.author}>", inline=True)
        embed.add_field(name="Active", value=f"{stickyMessage.active}", inline=True)
        embed.add_field(name="Delete old Sticky", value=f"{stickyMessage.delete_old_sticky}", inline=True)
        embed.add_field(name="Message Limit", value=f"{stickyMessage.message_limit}{'' if stickyMessage.message_limit > 0 else ' (disabled)'}", inline=True)
        embed.add_field(name="Time Limit", value=f"{stickyMessage.time_limit}{'' if stickyMessage.time_limit > 0 else ' (disabled)'}", inline=True)
        embed.add_field(name="Channel", value=f"{channel.mention}", inline=True)
        await inter.response.send_message(embed=embed)

    @stick.sub_command(description="Stick a message to the channel or modify the currently active one.")
    async def set(
        self,
        inter: ApplicationCommandInteraction,
        message:            str = commands.Param(default=None, name="message",           description="The message to stick. Use \\n for newlines!"),
        message_limit:      int = commands.Param(default=None, name="message-limit",     description="Number of messages to ignore before the sticky is sent again. 0 for no limit.", ge=0),
        time_limit:         int = commands.Param(default=None, name="time-limit",        description="Number of seconds required to pass before the sticky is sent again. 0 for no limit.", ge=0),
        delete_old_sticky: bool = commands.Param(default=None, name="delete-old-sticky", description="Whether to delete the old sticky message after a new one is sent. Defaults to True."),
        id:                 str = commands.Param(default=None, name="id",                description="The ID of a sticky message.", min_length=36, max_length=36)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage and id:
            await inter.response.send_message("The specified sticky message was not found.", ephemeral=True)
            return
        if message:
            message = message.replace("\\n", "\n")
        if stickyMessage:
            channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(stickyMessage.channel), Utils.get_alternate_channel(stickyMessage.channel))
            await self.set_stick_data(stickyMessage, content=message, message_limit=message_limit, time_limit=time_limit, delete_old_sticky=delete_old_sticky)
            await inter.response.send_message("Sticky message updated.")
            await Logging.guild_log(inter.guild_id, msg_with_emoji("STICKY", f"A sticky message (`{stickyMessage.id}`) in {channel.mention} was updated by {inter.author.name} (`{inter.author.id}`)"))
            Logging.info(f"Sticky message updated in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        else:
            if not message:
                await inter.response.send_message("You need to specify a message to create a new sticky.", ephemeral=True)
                return
            using_defaults = False
            channel = inter.channel
            if message_limit is None and time_limit is None:
                using_defaults = True
                message_limit = self.max_messages
                time_limit = self.min_time

            if delete_old_sticky is None:
                delete_old_sticky = True

            stickyMessage = await db.stickymessage.create(
                data={
                    "author":            inter.author.id,
                    "channel":           channel.id,
                    "guild":             inter.guild_id,
                    "content":           message,
                    "last_sent":         time(),
                    "messages_since":    0,
                    "message_limit":     message_limit,
                    "time_limit":        time_limit,
                    "delete_old_sticky": delete_old_sticky
                }
            )

            msg = "Sticky message added."
            if using_defaults:
                msg += f"\nYou didn't specify a value for both message-limit and time-limit, so I'm using the defaults of {message_limit} messages and {time_limit} seconds."
            await inter.response.send_message(msg)
            await Logging.guild_log(inter.guild_id, msg_with_emoji(
                "STICKY",
                f"A sticky message (`{stickyMessage.id}`) in {channel.mention} was created by {inter.author.name} (`{inter.author.id}`)")
            )
            Logging.info(f"Sticky message created in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await self.send_stick(stickyMessage.channel, override=True)

    @stick.sub_command(description="Start a previously stopped sticky message.")
    async def start(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=36, max_length=36)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            await inter.response.send_message("The specified sticky message was not found.", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(stickyMessage.channel), Utils.get_alternate_channel(stickyMessage.channel))
        if stickyMessage.active:
            await inter.response.send_message("Sticky message already active!", ephemeral=True)
            return
        await db.stickymessage.update(
            where={
                "id": stickyMessage.id
            },
            data={
                "active": True
            }
        )
        await inter.response.send_message("Sticky message started.")
        await Logging.guild_log(inter.guild_id, msg_with_emoji("STICKY", f"A sticky message (`{stickyMessage.id}`) in {channel.mention} was started by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"Sticky message started in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await self.send_stick(stickyMessage.channel, override=True)

    @stick.sub_command(description="Stop a currently active sticky message without deleting it.")
    async def stop(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=36, max_length=36)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            await inter.response.send_message("The specified sticky message was not found.", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(stickyMessage.channel), Utils.get_alternate_channel(stickyMessage.channel))
        if not stickyMessage.active:
            await inter.response.send_message("Sticky message already inactive!", ephemeral=True)
            return
        await db.stickymessage.update(
            where={
                "id": stickyMessage.id
            },
            data={
                "active": False
            }
        )
        await inter.response.send_message("Sticky message stopped.")
        await Logging.guild_log(inter.guild_id, msg_with_emoji("STICKY", f"A sticky message (`{stickyMessage.id}`) in {channel.mention} was stopped by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"Sticky message stopped in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @stick.sub_command(description="Unstick a message from the channel.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=36, max_length=36)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            await inter.response.send_message("The specified sticky message was not found.", ephemeral=True)
            return
        channel = self.bot.get_channel(stickyMessage.channel)
        if channel:
            await self.delete_current_stick(stickyMessage, channel)
        else:
            channel = Utils.get_alternate_channel(stickyMessage.channel)
        await db.stickymessage.delete(
            where={
                "id": stickyMessage.id
            }
        )
        await inter.response.send_message("Sticky message removed.")
        await Logging.guild_log(inter.guild_id, msg_with_emoji("STICKY", f"A sticky message (`{stickyMessage.id}`) in {channel.mention} was removed by {inter.author.name} (`{inter.author.id}`)"))
        Logging.info(f"Sticky message deleted in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @commands.Cog.listener()
    @commands.guild_only()
    async def on_message(self, message: disnake.Message):
        if message.author == self.bot.user:
            return
        stickyMessage = await db.stickymessage.find_unique(
            where={
                "channel": message.channel.id
            }
        )
        if stickyMessage:
            if stickyMessage.id not in self.locks:
                self.locks[stickyMessage.id] = asyncio.Lock()
            if self.locks[stickyMessage.id].locked():
                return
            try:
                async with self.locks[stickyMessage.id]:
                    await self.send_stick(message.channel.id)
            except asyncio.CancelledError:
                pass

    async def send_stick(self, channelId: int, override: bool = False):
        stickyMessage = await db.stickymessage.find_unique(
            where={
                "channel": channelId
            }
        )
        if not stickyMessage:
            Logging.warning(f"No sticky message found for channel {channelId}.")
            return
        if not stickyMessage.active:
            return
        channel = self.bot.get_channel(channelId)
        if not channel or not channel.permissions_for(channel.guild.me).send_messages:
            c = f"channel `{channelId}`" if not channel else channel.mention
            await Logging.guild_log(channel.guild.id, msg_with_emoji("WARN", f"I could not send a sticky message (`{stickyMessage.id}`) for {c}, because I don't have access to the channel."))
            Logging.warning(f"Could not send sticky. Channel {channelId} not found.")
            return

        if override or (stickyMessage.time_limit and abs(time() - stickyMessage.last_sent) >= stickyMessage.time_limit):
            await self.delete_current_stick(stickyMessage, channel)
            msg = await channel.send(stickyMessage.content)
            await self.set_stick_data(stickyMessage, current_id=msg.id)
        elif stickyMessage.message_limit:
            stickyMessage = await db.stickymessage.update(
                where={
                    "id": stickyMessage.id
                },
                data={
                    "messages_since": stickyMessage.messages_since + 1
                }
            )
            if stickyMessage.messages_since >= stickyMessage.message_limit:
                await self.delete_current_stick(stickyMessage, channel)
                msg = await channel.send(stickyMessage.content)
                await self.set_stick_data(stickyMessage, current_id=msg.id)

    async def set_stick_data(
        self,
        stickyMessage: StickyMessage,
        author: int = None,
        content: str = None,
        current_id: int = None,
        active: bool = None,
        message_limit: int = None,
        time_limit: int = None,
        delete_old_sticky: bool = None
    ):
        return await db.stickymessage.update(
            where={
                "id": stickyMessage.id
            },
            data={
                "last_sent":         time(),
                "messages_since":    0,
                "author":            Utils.coalesce(author, stickyMessage.author),
                "content":           Utils.coalesce(content, stickyMessage.content),
                "current_id":        Utils.coalesce(current_id, stickyMessage.current_id),
                "active":            Utils.coalesce(active, stickyMessage.active),
                "message_limit":     Utils.coalesce(message_limit, stickyMessage.message_limit),
                "time_limit":        Utils.coalesce(time_limit, stickyMessage.time_limit),
                "delete_old_sticky": Utils.coalesce(delete_old_sticky, stickyMessage.delete_old_sticky)
            }
        )

    async def delete_current_stick(self, stickyMessage: StickyMessage, channel: disnake.TextChannel):
        if stickyMessage.current_id and stickyMessage.delete_old_sticky:
            try:
                message = await channel.fetch_message(stickyMessage.current_id)
                await message.delete()
                await db.stickymessage.update(
                    where={
                        "id": stickyMessage.id
                    },
                    data={
                        "current_id": None
                    }
                )
            except Exception:
                pass

    async def get_sticky(
        self,
        inter: ApplicationCommandInteraction,
        id: str = None
    ) -> StickyMessage | None:
        id = Utils.coalesce(id, "-1")
        stickyMessage = await db.stickymessage.find_first(
            where={
                "OR": [
                    {
                        "id": {"equals": id}
                    },
                    {
                        "channel": {"equals": inter.channel.id}
                    }
                ]
            }
        )
        return stickyMessage


def setup(bot: commands.Bot):
    bot.add_cog(Sticky(bot))
