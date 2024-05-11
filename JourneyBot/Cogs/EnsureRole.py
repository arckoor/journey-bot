import disnake # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Database.DBConnector import db

from Views import Embed
from Util import Logging


class EnsureRole(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)

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

    @commands.Cog.listener()
    async def on_member_update(self, _: disnake.Member, after: disnake.Member):
        if after.flags.completed_onboarding:
            ensured_roles = await db.ensuredrole.find_many(
                where={
                    "guild": after.guild.id
                }
            )
            for ensured_role in ensured_roles:
                guild_roles = await after.guild.fetch_roles()
                for role in guild_roles:
                    if role.id == ensured_role.role:
                        if role not in after.roles:
                            await after.add_roles(role)
                            Logging.info(f"Added role {role.id} to {after.id} in {after.guild.id}.")


def setup(bot: commands.Bot):
    bot.add_cog(EnsureRole(bot))
