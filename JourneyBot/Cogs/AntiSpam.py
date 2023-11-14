
import time
import string
import re
import textwrap
import unicodedata
from dataclasses import dataclass, asdict
import Levenshtein

import disnake  # noqa
from disnake import ApplicationCommandInteraction, Message, Forbidden
from disnake.ext import commands, tasks

from Cogs.BaseCog import BaseCog
from Views import Embed
from Util import Configuration, Logging, Utils
from Util.Emoji import msg_with_emoji


@dataclass
class PoolMessage:
    content: str
    id: int
    timestamp: float


@dataclass
class PunishedMessage:
    content: str
    timestamp: float


class Bucket:
    def __init__(self, max_size: int, user_id: int, time_limit: int, remove_callback):
        self.bucket: list[PoolMessage] = []
        self.size_overflow_buffer = 3
        self.last_score = 0
        self.max_size = 0
        self.time_limit = 0
        self.user_id = user_id
        self.remove_callback = remove_callback
        self.set_max_size(max_size)
        self.set_time_frame(time_limit)
        self.remove_old_messages.start()

    def set_max_size(self, max_size: int):
        self.max_size = max_size + self.size_overflow_buffer
        if len(self.bucket) > self.max_size:
            self.bucket = self.bucket[-self.max_size:]

    def set_time_frame(self, time_limit: int):
        self.time_limit = time_limit

    def __iter__(self):
        return iter(self.bucket)

    def __len__(self):
        return len(self.bucket)

    def __str__(self):
        return textwrap.indent("\n".join([f"{msg.content}" for msg in self.bucket]), prefix=" "*4)

    def add_message(self, message: PoolMessage, score: float):
        self.bucket.append(message)
        self.last_score = score
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
        self.recently_punished: list[PunishedMessage] = []
        self.config = {}
        self.guild_id = guild_id
        self.initialize()

    def buckets(self, user_id: int):
        return iter(self.pool[user_id])

    def initialize(self):
        self.update_config()
        as_config = Utils.get_anti_spam_config(self.guild_id)
        for punished_message in as_config.recently_punished:
            punished_message["timestamp"] = float(punished_message["timestamp"])
            self.recently_punished.append(PunishedMessage(**punished_message))
        self.remove_recently_punished.start()

    def update_config(self):
        as_config = Utils.get_anti_spam_config(self.guild_id)
        self.config["violation_trigger"] = [(as_config.similar_message_threshold[i], as_config.max_messages[i]) for i in range(len(as_config.max_messages))]
        self.config["max_spam_messages"] = max(as_config.max_messages)
        self.config["min_sim_message_threshold"] = min(as_config.similar_message_threshold)
        self.config["sim_message_re_ban_threshold"] = as_config.similar_message_re_ban_threshold
        self.config["time_frame"] = as_config.time_frame
        for user_id in self.pool:
            for bucket in self.pool[user_id]:
                bucket.set_max_size(self.config["max_spam_messages"])
                bucket.set_time_frame(self.config["time_frame"])

    def add_message(self, message: Message) -> tuple[bool, Bucket, float]:
        content = self.preprocess_message(message)
        if not content:
            return False, None, None
        if message.author.id not in self.pool:
            self.pool[message.author.id] = []
        closest_bucket, confidence = self.find_closest_bucket(content, message.author.id)
        closest_bucket.add_message(PoolMessage(content, message.id, time.time()), confidence)
        is_recently_punished, is_recently_punished_confidence = self.is_recently_punished(content)
        if is_recently_punished:
            return True, closest_bucket, is_recently_punished_confidence
        for c_level, max_size in self.config["violation_trigger"]:
            if confidence >= c_level and len(closest_bucket) >= max_size:
                self.add_recent_punishment(PunishedMessage(content, time.time()))
                return True, closest_bucket, confidence
        return False, None, None

    def is_recently_punished(self, message: str):
        max_avg = 0
        for punished_message in self.recently_punished:
            avg = Levenshtein.jaro_winkler(message, punished_message.content)
            if avg > max_avg:
                max_avg = avg
        return max_avg > self.config["sim_message_re_ban_threshold"], max_avg

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
                if avg > self.config["min_sim_message_threshold"]:
                    closest_bucket = bucket
        if closest_bucket:
            return closest_bucket, max_avg
        else:
            new_bucket = self.get_new_bucket(user_id)
            return new_bucket, 0

    def get_new_bucket(self, user_id):
        new_bucket = Bucket(self.config["max_spam_messages"], user_id, self.config["time_frame"], self.remove_empty_bucket)
        self.pool[user_id].append(new_bucket)
        return new_bucket

    def remove_empty_bucket(self, user_id: int, bucket: Bucket):
        if user_id not in self.pool:
            return
        bucket.remove_old_messages.cancel()
        self.pool[user_id].remove(bucket)
        if not self.pool[user_id]:
            del self.pool[user_id]

    def remove_user(self, user_id: int):
        if user_id in self.pool:
            for bucket in self.pool[user_id]:
                self.remove_empty_bucket(user_id, bucket)
                del bucket

    def preprocess_message(self, message: Message):
        if not message.content:
            if message.attachments:
                msg = ", ".join(attachment.url for attachment in message.attachments)
            else:
                return None
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
            msg = msg.translate(str.maketrans("", "", string.punctuation))
            msg = "".join(reversed(re.sub(r"\A\d+.*", "", "".join(reversed(msg)))))
        return msg

    def add_recent_punishment(self, message: PunishedMessage):
        self.recently_punished.append(message)
        as_config = Utils.get_anti_spam_config(self.guild_id)
        new_message = {k: str(v) for k, v in asdict(message).items()}
        if new_message not in as_config.recently_punished:
            as_config.recently_punished.append(new_message)
        as_config.save()

    @tasks.loop(hours=4)
    async def remove_recently_punished(self):
        as_config = Utils.get_anti_spam_config(self.guild_id)
        for message in self.recently_punished:
            if time.time() - message.timestamp > 60*60*24*7:  # one week
                self.recently_punished.remove(message)
                as_config.recently_punished.remove({k: str(v) for k, v in asdict(message).items()})
        as_config.save()

    def print_pool(self, format_for_discord=True):
        if not self.pool and not self.recently_punished:
            return "Pool is currently empty."
        msg = "```\n" if format_for_discord else "\n"
        for user_id, buckets in self.pool.items():
            msg += f"User {user_id}:\n"
            for bucket in buckets:
                msg += f"  Bucket: ({bucket.last_score}) \n{str(bucket)}\n"
        msg += "Recent punishments:\n"
        for message in self.recently_punished:
            msg += f"  Message: ({message.timestamp}) {message.content}\n"
        msg += "```" if format_for_discord else ""
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
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        embed.add_field(name="Anti-Spam enabled", value=as_config.enabled, inline=True)
        if as_config.enabled:
            embed.add_field(name="Punishment", value=as_config.punishment, inline=True)
            embed.add_field(name="Max spam messages", value=", ".join(str(x) for x in as_config.max_messages), inline=True)
            embed.add_field(name="Similarity threshold(s)", value=", ".join(str(x) for x in as_config.similar_message_threshold), inline=True)
            embed.add_field(name="Similarity re-ban threshold", value=as_config.similar_message_re_ban_threshold, inline=True)
            embed.add_field(name="Trusted users", value="\n".join([f"<@{user}>" for user in as_config.trusted_users]) or "None", inline=True)
            embed.add_field(name="Trusted roles", value="\n".join([f"<@&{role}>" for role in as_config.trusted_roles]) or "None", inline=True)
        await inter.response.send_message(embed=embed)

    @as_config.sub_command(name="help", description="Show the help for the anti-spam module.")
    async def as_help(self, inter: ApplicationCommandInteraction):
        embed = Embed.default_embed(
            title="Anti-Spam Help",
            description="Help for the anti-spam module.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="max-spam-messages", value="The number of similar messages to trigger a violation. Multiple comma-separated values are allowed. For example `6, 4, 2`.", inline=False)
        embed.add_field(name="similarity-threshold", value="The threshold for when a message is considered similar. " +
                        "Multiple comma-separated values are allowed. For example. `.9, .95, .99`.", inline=False)
        embed.add_field(name="Correlation between max-spam-messages and similarity-threshold",
                        value="The first value of max-spam-messages corresponds to the first value of similarity-threshold, the second to the second, etc. " +
                        "- In the example above, a user would get punished for 6 messages that are .9 similar, but already for 4 if they are .95 similar and so on.", inline=False)
        embed.add_field(name="similarity-re-ban-threshold",
                        value="All recently punished messages are stored. If a message is similar to a recently punished message with a similarity higher than this threshold, " +
                        "the user is immediately punished. Useful for users that join with new accounts to spam again. Set > 1 to disable this behaviour.", inline=False)
        await inter.response.send_message(embed=embed)

    @as_config.sub_command(name="module-enable", description="Enable the anti-spam module.")
    async def as_enable(self, inter: ApplicationCommandInteraction):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if as_config.enabled:
            await inter.response.send_message("Anti-spam is already enabled for this guild.", ephemeral=True)
            return
        as_config.enabled = True
        as_config.save()
        await inter.response.send_message("Anti-spam enabled for this guild.")

    @as_config.sub_command(name="module-disable", description="Disable the anti-spam module.")
    async def as_disable(self, inter: ApplicationCommandInteraction):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if as_config.enabled:
            await inter.response.send_message("Anti-spam is already disabled for this guild.", ephemeral=True)
            return
        as_config.enabled = False
        as_config.save()
        await inter.response.send_message("Anti-spam disabled for this guild.")

    @as_config.sub_command(name="punishment", description="Configure the punishment for the anti-spam module.")
    async def as_configure_punishment(
        self,
        inter: ApplicationCommandInteraction,
        punishment:             str = commands.Param(name="punishment", description="The punishment for a violation.", choices=["mute", "ban"]),
        mute_role:     disnake.Role = commands.Param(name="mute-role", description="The role to assign to muted users.", default=None),
        max_spam_messages:      str = commands.Param(name="max-spam-messages", description="The number of similar messages to trigger a violation. Multiple comma-separated values are allowed."),
        similarity_threshold:   str = commands.Param(name="similarity-threshold", description="The threshold for when a message is considered similar. Multiple comma-separated values are allowed."),
        sim_re_ban_threshold: float = commands.Param(name="similarity-re-ban-threshold", description="The threshold for a message to lead to an immediate re-ban. Set > 1 to disable.", ge=0, le=2),
        time_frame:             int = commands.Param(name="time-frame", description="For how long a message is taken into account (in seconds).", ge=10)
    ):
        try:
            max_spam_messages = self.parse_string_to_list(max_spam_messages, int)
            similarity_threshold = self.parse_string_to_list(similarity_threshold, float)
        except ValueError:
            await inter.response.send_message("Could not parse max-spam-messages or similarity-threshold. Enter them in the format 'x, y, z'.", ephemeral=True)
            return
        if len(max_spam_messages) != len(similarity_threshold):
            await inter.response.send_message("The number of max-spam-messages and similarity-thresholds must be the same.", ephemeral=True)
            return
        elif any(x < 2 for x in max_spam_messages):
            await inter.response.send_message("Each value of max-spam-messages must be greater than 1.", ephemeral=True)
            return
        elif any(x < 0 or x > 1 for x in similarity_threshold):
            await inter.response.send_message("Each value of similarity-threshold must be between 0 and 1.", ephemeral=True)
            return
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if punishment == "mute":
            if not mute_role and as_config.mute_role:
                mute_role = inter.guild.get_role(as_config.mute_role)
            if not mute_role:
                await inter.response.send_message("You must specify a mute role.", ephemeral=True)
                return
            if not inter.me.guild_permissions.manage_roles:
                await inter.response.send_message("I don't have permission to manage and assign roles.", ephemeral=True)
                return
            as_config.mute_role = mute_role.id
        else:
            if not inter.guild.me.guild_permissions.ban_members:
                await inter.response.send_message("I don't have permission to ban members.", ephemeral=True)
                return
        as_config.punishment = punishment
        as_config.max_messages = max_spam_messages
        as_config.similar_message_threshold = similarity_threshold
        as_config.similar_message_re_ban_threshold = sim_re_ban_threshold
        as_config.time_frame = time_frame
        as_config.enabled = True
        as_config.save()
        await inter.response.send_message(f"Punishment set to {punishment}.")
        if inter.guild_id in self.pools:
            self.pools[inter.guild_id].update_config()

    @as_config.sub_command_group(name="trusted-roles", description="Configure the trusted roles for the anti-spam module.")
    async def as_configure_trusted_roles(self, inter: ApplicationCommandInteraction):
        pass

    @as_configure_trusted_roles.sub_command(name="add", description="Add a trusted role.")
    async def as_configure_trusted_roles_add(self, inter: ApplicationCommandInteraction, role: disnake.Role):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if role.id in as_config.trusted_roles:
            await inter.response.send_message("That role is already trusted.", ephemeral=True)
            return
        as_config.trusted_roles.append(role.id)
        as_config.save()
        await inter.response.send_message(f"Role {role.name} (`{role.id}`) added to trusted roles.")

    @as_configure_trusted_roles.sub_command(name="remove", description="Remove a trusted role.")
    async def as_configure_trusted_roles_remove(self, inter: ApplicationCommandInteraction, role: disnake.Role):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if role.id not in as_config.trusted_roles:
            await inter.response.send_message("That role is not trusted.", ephemeral=True)
            return
        as_config.trusted_roles.remove(role.id)
        as_config.save()
        await inter.response.send_message(f"Role {role.name} (`{role.id}`) removed from trusted roles.")

    @as_config.sub_command_group(name="trusted-users", description="Configure the trusted users for the anti-spam module.")
    async def as_configure_trusted_users(self, inter: ApplicationCommandInteraction):
        pass

    @as_configure_trusted_users.sub_command(name="add", description="Add a trusted user.")
    async def as_configure_trusted_users_add(self, inter: ApplicationCommandInteraction, user: disnake.User):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if user.id in as_config.trusted_users:
            await inter.response.send_message("That user is already trusted.", ephemeral=True)
            return
        as_config.trusted_users.append(user.id)
        as_config.save()
        await inter.response.send_message(f"User {user.name} (`{user.id}`) added to trusted users.")

    @as_configure_trusted_users.sub_command(name="remove", description="Remove a trusted user.")
    async def as_configure_trusted_users_remove(self, inter: ApplicationCommandInteraction, user: disnake.User):
        as_config = Utils.get_anti_spam_config(inter.guild_id)
        if user.id not in as_config.trusted_users:
            await inter.response.send_message("That user is not trusted.", ephemeral=True)
            return
        as_config.trusted_users.remove(user.id)
        as_config.save()
        await inter.response.send_message(f"User {user.name} (`{user.id}`) removed from trusted users.")

    @commands.slash_command(description="Print the current pool.", guild_ids=[Configuration.get_master_var("ADMIN_GUILD", 0)])
    @commands.is_owner()
    @commands.guild_only()
    @commands.default_member_permissions(administrator=True)
    async def pool(self, inter: disnake.ApplicationCommandInteraction, id: int = commands.Param(description="The guild id to print the pool of.", large=True, default=None)):
        if not id:
            id = inter.guild_id
        if id not in self.pools:
            await inter.response.send_message("Pool is currently empty.", ephemeral=True)
            return
        await inter.response.send_message(self.pools[id].print_pool())

    @commands.Cog.listener()
    @commands.guild_only()
    async def on_message(self, message: Message):
        as_config = Utils.get_anti_spam_config(message.guild.id)
        if not message.guild or not as_config.enabled:
            return
        if message.guild.id not in self.pools:
            self.pools[message.guild.id] = Pool(message.guild.id)
        pool = self.pools[message.guild.id]
        if message.author.id in as_config.trusted_users:
            return
        elif any([role in as_config.trusted_roles for role in (role.id for role in message.author.roles)]):
            return
        elif message.author.bot:
            return
        spam, bucket, confidence = pool.add_message(message)
        if spam:
            Logging.info(f"Detected spam by user {message.author.name} (`{message.author.id}`) with confidence {confidence}.")
            file = None
            if as_config.punishment == "mute":
                mute_role = message.guild.get_role(as_config.mute_role)
                try:
                    await message.author.add_roles(mute_role, reason=f"Spam detected in #{message.channel.name}")
                    file = Utils.make_file(self.bot, message.channel.name, (msg for msg in bucket))
                    await self.clean_user(message.guild.id, message.author.id)
                except Forbidden:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"I cannot assign the mute role {mute_role.name} (`{mute_role.id}`) to {message.author.name}."))
                    return
            else:
                try:
                    await message.guild.ban(message.author, reason=f"Spam detected in #{message.channel.name}", clean_history_duration=0)
                    file = Utils.make_file(self.bot, message.channel.name, (msg for msg in bucket))
                    await self.clean_user(message.guild.id, message.author.id)
                except Forbidden:
                    await Logging.guild_log(message.guild.id, msg_with_emoji("WARN", f"I cannot ban {message.author.name}."))
                    return
            msg = msg_with_emoji(
                "BAN",
                f"{message.author.name} (`{message.author.id}`) has been {'muted' if as_config.punishment == 'mute' else 'banned'} for spam. Confidence: " +
                f"{confidence:.3f}".rstrip("0").rstrip("."),
            )
            await Logging.guild_log(message.guild.id, message=msg, file=file)

    @commands.Cog.listener()
    async def on_member_ban(self, guild: disnake.Guild, user: disnake.User):
        if guild.id not in self.pools:
            return
        if user.id in self.pools[guild.id].pool:
            Logging.info(f"{user.name} ({user.id}) has been banned. They had the following pools: {self.pools[user.id].print_pool()}")

    async def clean_user(self, guild_id: int, user_id: int):
        pool = self.pools[guild_id]
        Logging.pool_log(pool.print_pool(format_for_discord=False))
        for bucket in pool.buckets(user_id):
            bucket: Bucket
            for message in bucket:
                try:
                    msg = self.bot.get_message(message.id)
                    if msg:
                        await msg.delete()
                except Exception:
                    pass
        pool.remove_user(user_id)

    def parse_string_to_list(self, input: str, type: type):
        return [type(x.strip()) for x in input.split(",") if x]


def setup(bot: commands.Bot):
    bot.add_cog(AntiSpam(bot))
