import datetime
import zoneinfo

import disnake
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from prisma.models import GuildConfig
from Cogs.BaseCog import BaseCog
from Database.DBConnector import db, get_guild_config

from Views import Embed
from Util import Logging, Utils


class EnsureRole(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.onboarding_cache: dict[int, tuple[bool, datetime.datetime]] = {}

    @commands.slash_command(name="ensure-role", dm_permission=False, description="Ensured role management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(manage_roles=True)
    async def ensure_role(self, inter: ApplicationCommandInteraction):
        pass

    @ensure_role.sub_command(name="list", description="List all ensured roles.")
    async def ensure_role_list(self, inter: ApplicationCommandInteraction):
        ensured_roles = await db.ensuredrole.find_many(
            where={
                "guild": inter.guild_id
            }
        )
        if not ensured_roles:
            await inter.response.send_message("No roles ensured.", ephemeral=True)
            return

        embed = Embed.default_embed(
            title="Ensured Roles",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        guild_roles = await inter.guild.fetch_roles()
        roles = [role for role in guild_roles if role.id in [ensured_role.role for ensured_role in ensured_roles]]
        embed.add_field(name="All ensured roles in the server:", value="\n".join([role.mention for role in roles]))
        await inter.response.send_message(embed=embed)

    @ensure_role.sub_command(name="add", description="Add a role to the ensured roles.")
    async def ensure_role_add(self, inter: ApplicationCommandInteraction, role: disnake.Role = commands.Param(description="The role to ensure.")):
        if await db.ensuredrole.find_unique(
            where={
                "guild_role": {
                    "guild": inter.guild_id,
                    "role": role.id
                }
            }
        ):
            await inter.response.send_message("Role already ensured.", ephemeral=True)
            return
        await db.ensuredrole.create(
            data={
                "guild": inter.guild_id,
                "role": role.id
            }
        )
        await inter.response.send_message(f"Role {role.name} ensured.")

    @ensure_role.sub_command(name="remove", description="Remove a role from the ensured roles.")
    async def ensure_role_remove(self, inter: ApplicationCommandInteraction, role: disnake.Role = commands.Param(description="The role to remove.")):
        if not await db.ensuredrole.find_unique(
            where={
                "guild_role": {
                    "guild": inter.guild_id,
                    "role": role.id
                }
            }
        ):
            await inter.response.send_message("Role not ensured.", ephemeral=True)
            return
        await db.ensuredrole.delete(
            where={
                "guild_role": {
                    "guild": inter.guild_id,
                    "role": role.id
                }
            }
        )
        await inter.response.send_message(f"Role {role.name} no longer ensured.")

    @ensure_role.sub_command(name="sweep", description="Sweep all members for ensured roles.")
    async def ensure_role_sweep(self, inter: ApplicationCommandInteraction):
        ensured_roles = await db.ensuredrole.find_many(
            where={
                "guild": inter.guild_id
            }
        )
        if not ensured_roles:
            await inter.response.send_message("No roles ensured.", ephemeral=True)
            return
        guild_config = await get_guild_config(inter.guild_id)
        onboarding_enabled = await self.get_onboarding_enabled(inter.guild_id, inter.guild.onboarding)

        thinking_id = await inter.response.defer(with_message=True, ephemeral=False)
        guild_roles = await inter.guild.fetch_roles()
        member_cnt, role_add_cnt = 0, 0
        for member in inter.guild.members:
            member_cnt += 1
            if not await self.member_is_valid_target(member, guild_config, onboarding_enabled):
                continue
            for ensured_role in ensured_roles:
                for role in guild_roles:
                    if role.id == ensured_role.role and role not in member.roles:
                        await member.add_roles(role)
                        Logging.info(f"Added role {role.id} to {member.id} in {inter.guild.id}.")
                        role_add_cnt += 1

        reply = f"I looked at {member_cnt} members and added {role_add_cnt} roles."
        if not inter.is_expired():
            await inter.followup.send(content=reply)
        else:
            try:
                await thinking_id.delete()
            except Exception:
                pass
            await inter.channel.send(content=reply)

    @ensure_role.sub_command(name="set-onboarding-time", description="Set the time onboarding was enabled.")
    async def ensure_role_set_onboarding_time(self, inter: ApplicationCommandInteraction, time: str = commands.Param(description="The time onboarding was enabled.")):
        if not (await inter.guild.onboarding()).enabled:
            await inter.response.send_message("Onboarding is not enabled for this guild.", ephemeral=True)
            return
        guild_config = await get_guild_config(inter.guild_id)
        try:
            timezone = zoneinfo.ZoneInfo(Utils.coalesce(guild_config.time_zone, "UTC"))
            parsed_time = datetime.datetime.strptime(time, "%d-%m-%Y %H:%M:%S").replace(tzinfo=timezone)
        except Exception:
            await inter.response.send_message(f"Invalid time format. Please use DD-MM-YYYY HH:MM:SS. {guild_config.time_zone} is assumed as a timezone.", ephemeral=True)
            return

        await db.guildconfig.update(
            where={
                "guild": inter.guild_id
            },
            data={
                "onboarding_active_since": parsed_time
            }
        )
        await inter.response.send_message(f"Onboarding time set to {time}.")

    @commands.Cog.listener()
    async def on_member_update(self, _: disnake.Member, after: disnake.Member):
        ensured_roles = await db.ensuredrole.find_many(
            where={
                "guild": after.guild.id
            }
        )
        if not ensured_roles:
            return
        if not await self.member_is_valid_target(after, await get_guild_config(after.guild.id), await self.get_onboarding_enabled(after.guild.id, after.guild.onboarding)):
            return
        for ensured_role in ensured_roles:
            guild_roles = await after.guild.fetch_roles()
            for role in guild_roles:
                if role.id == ensured_role.role:
                    if role not in after.roles:
                        await after.add_roles(role)
                        Logging.info(f"Added role {role.id} to {after.id} in {after.guild.id}.")

    async def get_onboarding_enabled(self, guild_id: int, onboarding):
        if guild_id in self.onboarding_cache:
            time_diff = datetime.datetime.now() - self.onboarding_cache[guild_id][1]
            if time_diff.seconds < 1800:
                return self.onboarding_cache[guild_id][0]
        onboarding_enabled = (await onboarding()).enabled
        self.onboarding_cache[guild_id] = (onboarding_enabled, datetime.datetime.now())
        return onboarding_enabled

    @staticmethod
    async def member_is_valid_target(member: disnake.Member, guild_config: GuildConfig, onboarding_enabled: bool):
        if member.bot:
            return False
        if onboarding_enabled:
            if not (member.flags.completed_onboarding or member.joined_at < guild_config.onboarding_active_since):
                return False
        return True


def setup(bot: commands.Bot):
    bot.add_cog(EnsureRole(bot))
