from time import time
import datetime

import disnake  # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import StickyMessage
from Util import Configuration, Logging


class Sticky(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        config = Configuration.get_master_var(self.__class__.__name__)
        self.max_messages = config.get("max_messages")
        self.min_time = config.get("min_time")

    @commands.slash_command(dm_permission=False)
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    async def stick(self, inter: ApplicationCommandInteraction):
        """
        Sticky message management.
        """
        pass

    @stick.sub_command()
    async def list(self, inter: ApplicationCommandInteraction):
        """
        List all stickies in the server.
        """
        stickies = StickyMessage.objects(guild=inter.channel.guild.id)
        now = datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)
        if not stickies:
            await inter.response.send_message("No stickies found.", ephemeral=True)
            return
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
            embed.add_field(name=f"#{self.bot.get_channel(sticky.channel).name} by @{self.bot.get_user(sticky.author).name}", value=f"{sticky.content}", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @stick.sub_command()
    async def set(self, inter: ApplicationCommandInteraction, message: str):
        """
        Stick a message to the channel or modify the currently active one.

        Parameters
        ----------
        message : str
            The message to stick.
        """
        channel = inter.channel
        if StickyMessage.objects(channel=channel.id):
            stickyMessage = StickyMessage.objects(channel=channel.id).first()
            self.set_stick_data(stickyMessage, content=message, author=inter.author.id, active=True)
            stickyMessage.save()
            await inter.response.send_message("Sticky message updated.", ephemeral=True)
            Logging.info(f"Sticky message updated in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        else:
            stickyMessage = StickyMessage(
                author=inter.author.id,
                channel=channel.id,
                guild=channel.guild.id,
                content=message,
                last_sent=time(),
                messages_since=0
            )
            stickyMessage.save()
            await inter.response.send_message("Sticky message added.", ephemeral=True)
            Logging.info(f"Sticky message created in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        await self.send_stick(channel.id, True)

    @stick.sub_command()
    async def start(self, inter: ApplicationCommandInteraction):
        """
        Start a previously stopped sticky message.
        """
        channel = inter.channel
        if StickyMessage.objects(channel=channel.id):
            stickyMessage = StickyMessage.objects(channel=channel.id).first()
            if stickyMessage.active:
                await inter.response.send_message("Sticky message already active!", ephemeral=True)
                return
            stickyMessage.active = True
            stickyMessage.save()
            await inter.response.send_message("Sticky message started.", ephemeral=True)
            Logging.info(f"Sticky message started in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
            await self.send_stick(channel.id, True)
        else:
            await inter.response.send_message("No sticky message found in this channel.", ephemeral=True)

    @stick.sub_command()
    async def stop(self, inter: ApplicationCommandInteraction):
        """
        Stop a currently active sticky message without deleting it.
        """
        channel = inter.channel
        if StickyMessage.objects(channel=channel.id):
            stickyMessage = StickyMessage.objects(channel=channel.id).first()
            if not stickyMessage.active:
                await inter.response.send_message("Sticky message already inactive!", ephemeral=True)
                return
            stickyMessage.active = False
            stickyMessage.save()
            await inter.response.send_message("Sticky message stopped.", ephemeral=True)
            Logging.info(f"Sticky message stopped in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")
        else:
            await inter.response.send_message("No sticky message found in this channel.", ephemeral=True)

    @stick.sub_command()
    async def remove(self, inter: ApplicationCommandInteraction):
        """
        Unstick a message from the channel.
        """
        channel = inter.channel
        if StickyMessage.objects(channel=channel.id):
            stickyMessage = StickyMessage.objects(channel=channel.id).first()
            await self.delete_current_stick(stickyMessage, channel)
            stickyMessage.delete()
            await inter.response.send_message("Sticky message removed.", ephemeral=True)
            Logging.info(f"Sticky message deleted in channel {channel.name} ({channel.guild.name}) by {inter.author.name} ({inter.author.id})")

    @commands.Cog.listener()
    @commands.guild_only()
    async def on_message(self, message: disnake.Message):
        if message.author == self.bot.user:
            return
        if StickyMessage.objects(channel=message.channel.id):
            await self.send_stick(message.channel.id)

    async def send_stick(self, channelId: int, override: bool = False):
        stickyMessage = StickyMessage.objects(channel=channelId).first()
        if not stickyMessage:
            Logging.warning(f"No sticky message found for channel {channelId}")
            return
        if not stickyMessage.active:
            return
        channel = self.bot.get_channel(channelId)
        if not channel:
            Logging.warning(f"Could not send stick. Channel {channelId} not found.")
            return

        if override or abs(time() - stickyMessage.last_sent) >= self.min_time:
            await self.delete_current_stick(stickyMessage, channel)
            msg = await channel.send(stickyMessage.content)
            self.set_stick_data(stickyMessage, current_id=msg.id)
            stickyMessage.save()
        else:
            stickyMessage.messages_since += 1
            stickyMessage.save()
            if stickyMessage.messages_since >= self.max_messages:
                await self.delete_current_stick(stickyMessage, channel)
                msg = await channel.send(stickyMessage.content)
                self.set_stick_data(stickyMessage, current_id=msg.id)
                stickyMessage.save()

    def set_stick_data(self, stickyMessage: StickyMessage, author: int = None, content: str = None, current_id: int = None, active: bool = None):
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

    async def delete_current_stick(self, stickyMessage: StickyMessage, channel: disnake.TextChannel):
        if stickyMessage.current_id:
            try:
                message = await channel.fetch_message(stickyMessage.current_id)
                await message.delete()
                stickyMessage.current_id = None
                stickyMessage.save()
            except Exception:
                pass


def setup(bot: commands.Bot):
    bot.add_cog(Sticky(bot))
