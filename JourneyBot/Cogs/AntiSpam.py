
import time
import textwrap
import unicodedata
from dataclasses import dataclass
import Levenshtein

import disnake  # noqa
from disnake import ApplicationCommandInteraction, Message, Forbidden
from disnake.ext import commands
from disnake.ext import tasks

from Cogs.BaseCog import BaseCog
from Views import Embed
from Util import Configuration, Logging, Utils
from Util.Emoji import msg_with_emoji


@dataclass
class PoolMessage:
    content: str
    id: int
    timestamp: float


class Bucket:
    def __init__(self, max_size: int, user_id: int, time_limit: int, remove_callback, ):
        self.bucket: list[PoolMessage] = []
        self.max_size = max_size
        self.time_limit = time_limit
        self.user_id = user_id
        self.remove_callback = remove_callback
        self.remove_old_messages.start()

    def set_max_size(self, max_size: int):
        self.max_size = max_size
        if len(self.bucket) > self.max_size:
            self.bucket = self.bucket[-self.max_size:]

    def __iter__(self):
        return iter(self.bucket)

    def __len__(self):
        return len(self.bucket)

    def __str__(self):
        return textwrap.indent("\n".join([f"{msg.content}" for msg in self.bucket]), prefix=" "*4)

    def add_message(self, message: PoolMessage):
        self.bucket.append(message)
        if len(self.bucket) > self.max_size:
            self.bucket.pop(0)

    @tasks.loop(minutes=5)
    async def remove_old_messages(self):
        for message in self.bucket:
            if time.time() - message.timestamp > self.time_limit:
                self.bucket.remove(message)
        if not self.bucket:
            self.remove_old_messages.stop()
            self.remove_callback(self.user_id, self)


class Pool:
    def __init__(self, guild_id: int):
        self.pool: dict[int, list[Bucket]] = {}
        self.config = {}
        self.guild_id = guild_id
        self.update_config()

    def buckets(self, user_id: int):
        return iter(self.pool[user_id])

    def update_config(self):
        guild_config = Utils.get_guild_config(self.guild_id)
        self.config["max_spam_messages"] = guild_config.anti_spam_max_messages
        self.config["similar_message_threshold"] = guild_config.anti_spam_similar_message_threshold
        for bucket in self.pool.values():
            bucket.max_size = self.config["max_spam_messages"]

    def add_message(self, message: Message) -> tuple[bool, Bucket, float]:
        if message.author.id not in self.pool:
            self.pool[message.author.id] = []
        content = self.preprocess_message(message)
        closest_bucket, confidence = self.find_closest_bucket(content, message.author.id)
        closest_bucket.add_message(PoolMessage(content, message.id, time.time()))
        if len(closest_bucket) >= self.config["max_spam_messages"]:
            return True, closest_bucket, confidence
        return False, None, None

    def find_closest_bucket(self, message, user_id) -> tuple[Bucket, float]:
        max_avg = 0
        closest_bucket = None
        for bucket in self.pool[user_id]:
            avg = 0
            for msg in bucket:
                avg += Levenshtein.jaro_winkler(msg.content, message)
            avg /= len(bucket)
            if avg > max_avg:
                max_avg = avg
                if avg > self.config["similar_message_threshold"]:
                    closest_bucket = bucket
        if closest_bucket:
            return closest_bucket, max_avg
        else:
            new_bucket = self.get_new_bucket(user_id)
            return new_bucket, 0

    def get_new_bucket(self, user_id):
        new_bucket = Bucket(self.config["max_spam_messages"], user_id, 300, self.remove_empty_bucket)  # TODO should this be the same as line 98?
        self.pool[user_id].append(new_bucket)
        return new_bucket

    def remove_empty_bucket(self, user_id: int, bucket: Bucket):
        if user_id not in self.pool:
            return
        self.pool[user_id].remove(bucket)
        if not self.pool[user_id]:
            del self.pool[user_id]

    def remove_user(self, user_id: int):
        if user_id in self.pool:
            for bucket in self.pool[user_id]:
                self.remove_empty_bucket(user_id, bucket)
                bucket.remove_old_messages.stop()
                del bucket
            del self.pool[user_id]

    def preprocess_message(self, message: Message):
        if not message.content:
            msg = ", ".join(attachment.url for attachment in message.attachments)
        else:
            replacements = {
                "𝘢": "a",
                "𝘤": "c",
                "𝘦": "e",
                "𝘧": "f",
                "𝘩": "h",
                "𝘯": "n",
                "𝘪": "i",
                "𝘪": "i",
                "𝘬": "k",
                "𝘲": "q",
                "𝘳": "r",
                "𝘴": "s",
                "𝘵": "t",
                "𝘶": "u",
                "𝘸": "w",
                "𝘺": "y"
            }
            msg = message.content.lower()
            msg = "".join(replacements.get(char, char) for char in msg)
            msg = "".join(unicodedata.normalize("NFKD", char).encode("ASCII", "ignore").decode() for char in msg)
        return msg

    def print_pool(self):
        if not self.pool:
            return "Pool is currently empty."
        msg = "----- Pool Dump -----\n```\n"
        for user_id, buckets in self.pool.items():
            msg += f"User {user_id}:\n"
            for bucket in buckets:
                msg += f"  Bucket: \n{str(bucket)}\n"
        msg += "```"
        return msg


class AntiSpam(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.pools: dict[int, Pool] = {}

    @commands.slash_command(name="anti-spam-config", description="Anti-Spam management", dm_permission=False)
    @commands.guild_only()
    @commands.bot_has_permissions(read_message_history=True, embed_links=True, send_messages=True, view_channel=True)
    @commands.default_member_permissions(ban_members=True)
    async def as_config(self, inter: ApplicationCommandInteraction):
        pass

    @as_config.sub_command(name="show", description="Show the config of the anti-spam module.")
    async def as_show(self, inter: ApplicationCommandInteraction):
        embed = Embed.default_embed(
            title="Anti-Spam Config",
            description="Current config of the anti-spam module.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        guild_config = Utils.get_guild_config(inter.guild_id)
        embed.add_field(name="Anti-Spam enabled", value=guild_config.anti_spam_enabled, inline=True)
        if guild_config.anti_spam_enabled:
            embed.add_field(name="Punishment", value=guild_config.anti_spam_punishment, inline=True)
            embed.add_field(name="Max spam messages", value=guild_config.anti_spam_max_messages, inline=True)
            embed.add_field(name="Similarity threshold", value=guild_config.anti_spam_similar_message_threshold, inline=True)
            embed.add_field(name="Trusted users", value="\n".join([f"<@{user}>" for user in guild_config.trusted_users]) or "None", inline=True)
            embed.add_field(name="Trusted roles", value="\n".join([f"<@&{role}>" for role in guild_config.trusted_roles]) or "None", inline=True)
        await inter.response.send_message(embed=embed)

    @as_config.sub_command(name="module-enable", description="Enable the anti-spam module.")
    async def as_enable(self, inter: ApplicationCommandInteraction):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if guild_config.anti_spam_enabled:
            await inter.response.send_message("Anti-spam is already enabled for this guild.", ephemeral=True)
            return
        guild_config.anti_spam_enabled = True
        guild_config.save()
        await inter.response.send_message("Anti-spam enabled for this guild.")

    @as_config.sub_command(name="module-disable", description="Disable the anti-spam module.")
    async def as_disable(self, inter: ApplicationCommandInteraction):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if guild_config.anti_spam_enabled:
            await inter.response.send_message("Anti-spam is already disabled for this guild.", ephemeral=True)
            return
        guild_config.anti_spam_enabled = False
        guild_config.save()
        await inter.response.send_message("Anti-spam disabled for this guild.")

    @as_config.sub_command(name="punishment", description="Configure the punishment for the anti-spam module.")
    async def as_configure_punishment(
        self,
        inter: ApplicationCommandInteraction,
        punishment: str = commands.Param(description="punishment", choices=["mute", "ban"]),
        mute_role: disnake.Role = commands.Param(description="mute-role", default=None),
        max_spam_messages: int = commands.Param(description="max-spam-messages", ge=2),
        similarity_threshold: float = commands.Param(description="similarity-threshold", gt=0.0, le=1.0),
    ):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if punishment == "mute":
            if not mute_role:
                await inter.response.send_message("You must specify a mute role.", ephemeral=True)
                return
            if not mute_role.is_assignable():
                await inter.response.send_message("I cannot assign that role.", ephemeral=True)
                return
            guild_config.mute_role = mute_role.id
        else:
            if not inter.guild.me.guild_permissions.ban_members:
                await inter.response.send_message("I don't have permission to ban members.", ephemeral=True)
                return
        guild_config.anti_spam_punishment = punishment
        guild_config.anti_spam_max_messages = max_spam_messages
        guild_config.anti_spam_similar_message_threshold = similarity_threshold
        guild_config.save()
        await inter.response.send_message(f"Punishment set to {punishment}.")
        if inter.guild_id in self.pools:
            self.pools[inter.guild_id].update_config()

    @as_config.sub_command_group(name="trusted-roles", description="Configure the trusted roles for the anti-spam module.")
    async def as_configure_trusted_roles(self, inter: ApplicationCommandInteraction):
        pass

    @as_configure_trusted_roles.sub_command(name="add", description="Add a trusted role.")
    async def as_configure_trusted_roles_add(self, inter: ApplicationCommandInteraction, role: disnake.Role):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if role.id in guild_config.trusted_roles:
            await inter.response.send_message("That role is already trusted.", ephemeral=True)
            return
        guild_config.trusted_roles.append(role.id)
        guild_config.save()
        await inter.response.send_message(f"Role {role.name} (`{role.id}`) added to trusted roles.")

    @as_configure_trusted_roles.sub_command(name="remove", description="Remove a trusted role.")
    async def as_configure_trusted_roles_remove(self, inter: ApplicationCommandInteraction, role: disnake.Role):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if role.id not in guild_config.trusted_roles:
            await inter.response.send_message("That role is not trusted.", ephemeral=True)
            return
        guild_config.trusted_roles.remove(role.id)
        guild_config.save()
        await inter.response.send_message(f"Role {role.name} (`{role.id}`) removed from trusted roles.")

    @as_config.sub_command_group(name="trusted-users", description="Configure the trusted users for the anti-spam module.")
    async def as_configure_trusted_users(self, inter: ApplicationCommandInteraction):
        pass

    @as_configure_trusted_users.sub_command(name="add", description="Add a trusted user.")
    async def as_configure_trusted_users_add(self, inter: ApplicationCommandInteraction, user: disnake.User):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if user.id in guild_config.trusted_users:
            await inter.response.send_message("That user is already trusted.", ephemeral=True)
            return
        guild_config.trusted_users.append(user.id)
        guild_config.save()
        await inter.response.send_message(f"User {user.name} (`{user.id}`) added to trusted users.")

    @as_configure_trusted_users.sub_command(name="remove", description="Remove a trusted user.")
    async def as_configure_trusted_users_remove(self, inter: ApplicationCommandInteraction, user: disnake.User):
        guild_config = Utils.get_guild_config(inter.guild_id)
        if user.id not in guild_config.trusted_users:
            await inter.response.send_message("That user is not trusted.", ephemeral=True)
            return
        guild_config.trusted_users.remove(user.id)
        guild_config.save()
        await inter.response.send_message(f"User {user.name} (`{user.id}`) removed from trusted users.")

    @commands.slash_command(description="Print the current pool.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.guild_only()
    @commands.default_member_permissions(administrator=True)
    async def pool(self, inter: disnake.ApplicationCommandInteraction):
        if inter.guild_id not in self.pools:
            await inter.response.send_message("Pool is currently empty", ephemeral=True)
            return
        await inter.response.send_message(self.pools[inter.guild_id].print_pool())

    @commands.Cog.listener()
    @commands.guild_only()
    async def on_message(self, message: Message):
        guild_config = Utils.get_guild_config(message.guild.id)
        if not message.guild or not guild_config.anti_spam_enabled:
            return
        if message.guild.id not in self.pools:
            self.pools[message.guild.id] = Pool(message.guild.id)
        pool = self.pools[message.guild.id]
        if message.author.id in guild_config.trusted_users:
            return
        elif any([role in guild_config.trusted_roles for role in (role.id for role in message.author.roles)]):
            return
        elif message.author.bot:
            return
        spam, bucket, confidence = pool.add_message(message)
        if spam:
            Logging.info(f"Detected spam by user {message.author.name} (`{message.author.id}`) with confidence {confidence}.")
            file = Utils.make_file(self.bot, message.channel.name, (msg for msg in bucket))
            if guild_config.anti_spam_punishment == "mute":
                mute_role = message.guild.get_role(guild_config.mute_role)
                if not mute_role:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"Could not find mute role {guild_config.mute_role}."))
                    return
                elif not mute_role.is_assignable():
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"I cannot assign mute role {mute_role.name}."))
                    return
                try:
                    await message.author.add_roles(mute_role, reason=f"Spam detected in {message.channel.name}")
                    await self.clean_user(message.guild.id, message.author.id)
                except Forbidden:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"I cannot assign mute role {mute_role.name} to {message.author.name}."))
                    return
            else:
                if not message.guild.me.guild_permissions.ban_members:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", "I cannot ban members."))
                    return
                try:
                    await message.guild.ban(message.author, reason=f"Spam detected in {message.channel.name}", clean_history_duration=0)
                except Forbidden:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"I cannot ban {message.author.name}."))
            msg = msg_with_emoji("BAN", f"{message.author.name} (`{message.author.id}`) has been {'muted' if guild_config.anti_spam_punishment == 'mute' else 'banned'} for spamming.")
            await Logging.guild_log(message.guild.id, message=msg, file=file)

    @commands.Cog.listener()
    async def on_member_ban(self, guild: disnake.Guild, user: disnake.User):
        if guild.id not in self.pools:
            return
        await self.clean_user(guild.id, user.id)
        self.pools[guild.id].remove_user(user.id)

    async def clean_user(self, guild_id: int, user_id: int):
        pool = self.pools[guild_id]
        for bucket in pool.buckets(user_id):
            bucket: Bucket
            for message in bucket:
                try:
                    msg = self.bot.get_message(message.id)
                    if msg:
                        await msg.delete()
                except Exception as exce:
                    print(exce)
        pool.remove_user(user_id)


def setup(bot: commands.Bot):
    bot.add_cog(AntiSpam(bot))
