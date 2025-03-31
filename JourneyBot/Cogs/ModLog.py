import datetime
import io
import time
import zoneinfo

import disnake
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import get_guild_config
from Util import Emoji, Logging


class ModLog(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

    @commands.slash_command(name="mod-log-config", description="Mod-Log management")
    @commands.guild_only()
    @commands.bot_has_permissions(
        read_message_history=True,
        embed_links=True,
        send_messages=True,
        view_channel=True,
    )
    @commands.default_member_permissions(ban_members=True)
    async def ml_config(self, inter: ApplicationCommandInteraction):
        pass

    @ml_config.sub_command(name="channel", description="Set the mod-log channel.")
    async def ml_configure_channel(
        self,
        inter: ApplicationCommandInteraction,
        channel: disnake.TextChannel = commands.Param(description="The channel to set as the Mod-Log channel."),
    ):
        perms = channel.permissions_for(inter.guild.me)
        if not perms.view_channel:
            await inter.response.send_message("I don't have permission to view that channel.", ephemeral=True)
            return
        elif not perms.send_messages:
            await inter.response.send_message(
                "I don't have permission to send messages in that channel.",
                ephemeral=True,
            )
            return
        elif not perms.embed_links:
            await inter.response.send_message(
                "I don't have permission to embed links in that channel.",
                ephemeral=True,
            )
            return

        _ = await get_guild_config(inter.guild_id)

        guild_config = await get_guild_config(inter.guild_id)
        guild_config.guild_log = channel.id
        await guild_config.save()
        await inter.response.send_message(f"Mod-Log channel set to {channel.mention}.")

    @ml_config.sub_command(name="new-threshold", description="Set the threshold for new users.")
    async def ml_configure_new_threshold(
        self,
        inter: ApplicationCommandInteraction,
        threshold: int = commands.param(description="The new user threshold (in days)", ge=1),
    ):
        guild_config = await get_guild_config(inter.guild_id)
        guild_config.new_user_threshold = threshold
        await guild_config.save()
        await inter.response.send_message(f"New user threshold set to {threshold} days.")

    @ml_config.sub_command(name="time-zone", description="Set the time zone for logs")
    async def ml_configure_time_zone(
        self,
        inter: ApplicationCommandInteraction,
        time_zone: str = commands.param(description="The time zone to use for logs."),
    ):
        available_zones = zoneinfo.available_timezones()
        if time_zone not in available_zones:
            out = "\n".join(x for x in (sorted(x for x in available_zones)))
            buffer = io.BytesIO()
            buffer.write(out.encode("utf-8"))
            buffer.seek(0)
            await inter.response.send_message(
                "I don't know this time zone. See the attached file for all valid values.",
                file=disnake.File(buffer, filename="time-zones.txt"),
                ephemeral=True,
            )
            return

        guild_config = await get_guild_config(inter.guild_id)
        guild_config.time_zone = time_zone
        await guild_config.save()
        await inter.response.send_message(f"Time zone set to {time_zone}.")

    @commands.Cog.listener()
    async def on_member_join(self, member: disnake.Member):
        guild_config = await get_guild_config(member.guild.id)
        dif = datetime.datetime.utcfromtimestamp(time.time()).replace(tzinfo=datetime.timezone.utc) - member.created_at
        new_user_threshold = datetime.timedelta(days=guild_config.new_user_threshold)
        minutes, _ = divmod(dif.days * 86400 + dif.seconds, 60)
        hours, minutes = divmod(minutes, 60)
        if dif.days > 0:
            age = f"{dif.days} days"
        else:
            age = f"{hours} hours, {minutes} minutes"
        await Logging.guild_log(
            member.guild.id,
            Emoji.msg_with_emoji(
                "JOIN",
                f"{member.mention} (`{member.id}`) has joined the server, account created {age} ago. {':new:' if new_user_threshold > dif else ''}",
            ),
        )


def setup(bot: commands.Bot):
    bot.add_cog(ModLog(bot))
