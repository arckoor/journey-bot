from time import time
import datetime
import typing

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import StickyMessage
from Util import Configuration, Logging, Validation


class Sticky(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        config = Configuration.get_master_var(self.__class__.__name__)
        self.max_messages = config.get("max_messages")
        self.min_time = config.get("min_time")

    @commands.slash_command(dm_permission=False, description="Sticky message management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    async def stick(self, inter: ApplicationCommandInteraction):
        pass

    @stick.sub_command(description="List all stickies in the server.")
    async def list(self, inter: ApplicationCommandInteraction):
        stickies = StickyMessage.objects(guild=inter.guild_id)
        if not stickies:
            await inter.response.send_message("No stickies found.", ephemeral=True)
            return
        now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
        embed = disnake.Embed(
            title="Stickies",
            description="All stickies in this server.",
            timestamp=now,
            color=disnake.Color.from_rgb(**Configuration.get_master_var("EMBED_COLOR"))
        )
        embed.set_footer(
            text=f"Requested by {inter.author.name}",
            icon_url=inter.author.avatar.url
        )
        for sticky in stickies:
            channel = self.bot.get_channel(sticky.channel)
            user = self.bot.get_user(sticky.author)
            if channel and channel.name:
                channel_name = channel.name
            else:
                channel_name = "Unknown"
            if user and user.name:
                user_name = user.name
            else:
                user_name = "Unknown"
            stopped = "" if sticky.active else " (stopped)"
            embed.add_field(name=f"#{channel_name} by @{user_name}{stopped} | ID: {sticky.id}", value=f"{sticky.content}", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @stick.sub_command(description="Show some info about the sticky in this channel.")
    async def info(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=24, max_length=24)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            return
        now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
        embed = disnake.Embed(
            title="Sticky Info",
            description="Info about the sticky in this channel.",
            timestamp=now,
            color=disnake.Color.from_rgb(**Configuration.get_master_var("EMBED_COLOR"))
        )
        embed.set_footer(
            text=f"Requested by {inter.author.name}",
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="ID", value=f"{stickyMessage.id}")
        embed.add_field(name="Content", value=f"{stickyMessage.content}", inline=False)
        embed.add_field(name="Author", value=f"<@{stickyMessage.author}>", inline=True)
        embed.add_field(name="Active", value=f"{stickyMessage.active}", inline=True)
        embed.add_field(name="Delete old Sticky", value=f"{stickyMessage.delete_old_sticky}", inline=True)
        embed.add_field(name="Message Limit", value=f"{stickyMessage.message_limit}{'' if stickyMessage.message_limit > 0 else ' (disabled)'}", inline=True)
        embed.add_field(name="Time Limit", value=f"{stickyMessage.time_limit}{'' if stickyMessage.time_limit > 0 else ' (disabled)'}", inline=True)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @stick.sub_command(description="Stick a message to the channel or modify the currently active one.")
    async def set(
        self,
        inter: ApplicationCommandInteraction,
        message:            str = commands.Param(default=None, name="message",           description="The message to stick. Use \\n for newlines!"),
        message_limit:      int = commands.Param(default=None, name="message-limit",     description="Number of messages to ignore before the sticky is sent again. 0 for no limit.", ge=0),
        time_limit:         int = commands.Param(default=None, name="time-limit",        description="Number of seconds required to pass before the sticky is sent again. 0 for no limit.", ge=0),
        delete_old_sticky: bool = commands.Param(default=None, name="delete-old-sticky", description="Whether to delete the old sticky message after a new one is sent. Defaults to True."),
        id:                 str = commands.Param(default=None, name="id",                description="The ID of a sticky message.", min_length=24, max_length=24)
    ):
        stickyMessage = await self.get_sticky(inter, id, respond_to=[Validation.ValidationType.INVALID_ID, Validation.ValidationType.ID_NOT_FOUND])
        if inter.response.is_done():
            return
        channel = inter.channel
        if message:
            message = message.replace("\\n", "\n")
        if stickyMessage:
            self.set_stick_data(stickyMessage, content=message, message_limit=message_limit, time_limit=time_limit, delete_old_sticky=delete_old_sticky)
            stickyMessage.save()
            await inter.response.send_message("Sticky message updated.", ephemeral=True)
            Logging.info(f"Sticky message updated in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        else:
            if not message:
                await inter.response.send_message("You need to specify a message to create a new sticky.", ephemeral=True)
                return
            using_defaults = False
            if message_limit is None and time_limit is None:
                using_defaults = True
                message_limit = self.max_messages
                time_limit = self.min_time

            if delete_old_sticky is None:
                delete_old_sticky = True

            stickyMessage = StickyMessage(
                author=inter.author.id,
                channel=channel.id,
                guild=inter.guild_id,
                content=message,
                last_sent=time(),
                messages_since=0,
                message_limit=message_limit,
                time_limit=time_limit,
                delete_old_sticky=delete_old_sticky
            )
            stickyMessage.save()

            msg = "Sticky message added."
            if using_defaults:
                msg += f"\nYou didn't specify a value for both message-limit and time-limit, so I'm using the defaults of {message_limit} messages and {time_limit} seconds."
            await inter.response.send_message(msg, ephemeral=True)
            Logging.info(f"Sticky message created in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await self.send_stick(stickyMessage.channel, override=True)

    @stick.sub_command(description="Start a previously stopped sticky message.")
    async def start(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=24, max_length=24)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            return
        channel = inter.channel
        if stickyMessage.active:
            await inter.response.send_message("Sticky message already active!", ephemeral=True)
            return
        stickyMessage.active = True
        stickyMessage.save()
        await inter.response.send_message("Sticky message started.", ephemeral=True)
        Logging.info(f"Sticky message started in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await self.send_stick(channel.id, True)

    @stick.sub_command(description="Stop a currently active sticky message without deleting it.")
    async def stop(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=24, max_length=24)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            return
        channel = inter.channel
        if not stickyMessage.active:
            await inter.response.send_message("Sticky message already inactive!", ephemeral=True)
            return
        stickyMessage.active = False
        stickyMessage.save()
        await inter.response.send_message("Sticky message stopped.", ephemeral=True)
        Logging.info(f"Sticky message stopped in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @stick.sub_command(description="Unstick a message from the channel.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(default=None, name="id", description="The ID of a sticky message.", min_length=24, max_length=24)
    ):
        stickyMessage = await self.get_sticky(inter, id)
        if not stickyMessage:
            return
        channel = inter.channel
        await self.delete_current_stick(stickyMessage, channel)
        stickyMessage.delete()
        await inter.response.send_message("Sticky message removed.", ephemeral=True)
        Logging.info(f"Sticky message deleted in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @commands.Cog.listener()
    @commands.guild_only()
    async def on_message(self, message: disnake.Message):
        if message.author == self.bot.user:
            return
        stickies = StickyMessage.objects(channel=message.channel.id)
        if stickies:
            stickyMessage = stickies.first()
            if stickyMessage.in_progress:
                return
            stickyMessage.in_progress = True
            stickyMessage.save()
            await self.send_stick(message.channel.id)
            stickyMessage.in_progress = False
            stickyMessage.save()

    async def send_stick(self, channelId: int, override: bool = False):
        stickyMessage = StickyMessage.objects(channel=channelId).first()
        if not stickyMessage:
            Logging.warning(f"No sticky message found for channel {channelId}.")
            return
        if not stickyMessage.active:
            return
        channel = self.bot.get_channel(channelId)
        if not channel:
            Logging.warning(f"Could not send stick. Channel {channelId} not found.")
            return

        if override or (stickyMessage.time_limit and abs(time() - stickyMessage.last_sent) >= stickyMessage.time_limit):
            await self.delete_current_stick(stickyMessage, channel)
            msg = await channel.send(stickyMessage.content)
            self.set_stick_data(stickyMessage, current_id=msg.id)
            stickyMessage.save()
        elif stickyMessage.message_limit:
            stickyMessage.messages_since += 1
            stickyMessage.save()
            if stickyMessage.messages_since >= stickyMessage.message_limit:
                await self.delete_current_stick(stickyMessage, channel)
                msg = await channel.send(stickyMessage.content)
                self.set_stick_data(stickyMessage, current_id=msg.id)
                stickyMessage.save()

    def set_stick_data(
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
        stickyMessage.last_sent = time()
        stickyMessage.messages_since = 0
        if author:
            stickyMessage.author = author
        if content:
            stickyMessage.content = content
        if current_id:
            stickyMessage.current_id = current_id
        if active is not None:
            stickyMessage.active = active
        if message_limit is not None:
            stickyMessage.message_limit = message_limit
        if time_limit is not None:
            stickyMessage.time_limit = time_limit
        if delete_old_sticky is not None:
            stickyMessage.delete_old_sticky = delete_old_sticky

    async def delete_current_stick(self, stickyMessage: StickyMessage, channel: disnake.TextChannel):
        if stickyMessage.current_id and stickyMessage.delete_old_sticky:
            try:
                message = await channel.fetch_message(stickyMessage.current_id)
                await message.delete()
                stickyMessage.current_id = None
                stickyMessage.save()
            except Exception:
                pass

    async def get_sticky(
        self,
        inter: ApplicationCommandInteraction,
        id: str = None,
        respond_to: [typing.Literal] = [
            Validation.ValidationType.INVALID_ID,
            Validation.ValidationType.ID_NOT_FOUND,
            Validation.ValidationType.NOT_IN_CHANNEL
        ]
    ) -> StickyMessage | None:
        stickyMessage: StickyMessage
        stickyMessage, type = await Validation.get_from_id_or_channel(StickyMessage, inter, id)
        responses = {
            Validation.ValidationType.INVALID_ID:     "Invalid ID.",
            Validation.ValidationType.ID_NOT_FOUND:   "No sticky message found with that ID.",
            Validation.ValidationType.NOT_IN_CHANNEL: "No sticky message found in this channel."
        }
        if type in respond_to:
            await inter.response.send_message(responses.get(type), ephemeral=True)
            return None
        return stickyMessage


def setup(bot: commands.Bot):
    bot.add_cog(Sticky(bot))
