import datetime
import asyncio

import disnake # noqa
from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from twitchAPI.twitch import Stream
from twitchAPI.helper import limit

from Cogs.BaseCog import BaseCog
from prisma.models import StreamObserver
from Database.DBConnector import db
from Views import Embed
from Util import Configuration, Utils, Logging, Twitch
from Util.Emoji import msg_with_emoji


class Streams(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.twitch_api = Twitch.get_twitch()
        self.stop_requests = []
        config = Configuration.get_master_var("TWITCH_API")
        self.max_concurrent_streams = config.get("MAX_CONCURRENT_STREAMS", 10)
        self.refresh_interval = config.get("REFRESH_INTERVAL", 60)
        self.offline_threshold = config.get("OFFLINE_THRESHOLD", 60 * 10)

    async def cog_load(self):
        for observer in await db.streamobserver.find_many(
            include={
                "known_streams": True
            }
        ):
            self.bot.loop.create_task(self.observe_game(observer))

    async def close(self):
        for observer in await db.streamobserver.find_many():
            if observer.id not in self.stop_requests:
                self.stop_requests.append(observer.id)
        timer = 0
        while self.stop_requests:
            await asyncio.sleep(1)
            timer += 1
            if timer > 1.5 * self.refresh_interval:
                break

    @commands.slash_command(dm_permission=False, name="stream-observer", description="Stream observer management.")
    @commands.guild_only()
    @commands.default_member_permissions(ban_members=True)
    @commands.bot_has_permissions(send_messages=True)
    async def stream_observer(self, inter: ApplicationCommandInteraction):
        pass

    @stream_observer.sub_command(name="template-help", description="Stream observer template help.")
    async def template_help(self, inter: ApplicationCommandInteraction):
        embed = Embed.default_embed(
            title="Stream observer template help.",
            description="Explanation of the template syntax.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="Line breaks", value="Line breaks are represented by `\\n`.", inline=False)
        embed.add_field(name="Variables", value="Variables are replaced with the corresponding value from a stream.", inline=False)
        embed.add_field(name="{{title}}", value="The title of the stream.", inline=False)
        embed.add_field(name="{{user}}", value="The user streaming.", inline=False)
        embed.add_field(name="{{user_login}}", value="The user streaming.", inline=False)
        embed.add_field(name="{{game}}", value="The game being played.", inline=False)
        embed.add_field(name="{{tags}}", value="The tags of the stream.", inline=False)
        embed.add_field(name="{{viewer_count}}", value="The number of viewers.", inline=False)
        embed.add_field(name="{{link}}", value="The link to the stream.", inline=False)
        embed.add_field(name="End template", value="For the end template, only {{game}} is available.", inline=False)
        await inter.response.send_message(embed=embed, ephemeral=True)

    @stream_observer.sub_command(name="list", description="List all stream observers.")
    async def list(self, inter: ApplicationCommandInteraction):
        observers = await db.streamobserver.find_many(
            where={
                "guild": inter.guild_id
            }
        )
        if not observers:
            await inter.response.send_message("No stream observers found.", ephemeral=True)
            return
        embed = Embed.default_embed(
            title="Stream observers",
            description="List of all stream observers.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        for observer in observers:
            channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(observer.channel), Utils.get_alternate_channel(observer.channel))
            embed.add_field(name=f"#{channel.name} | ID: {observer.id}", value=f"Game id: {observer.game_id}", inline=False)
        await inter.response.send_message(embed=embed)

    @stream_observer.sub_command(name="info", description="Get info about a stream observer.")
    async def info(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(name="observer-id", description="The ID of the stream observer to get info about.", min_length=36, max_length=36)
    ):
        observer = await db.streamobserver.find_first(
            where={
                "id": id
            }
        )
        if not observer:
            await inter.response.send_message("No stream observer found with that id", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(observer.channel), Utils.get_alternate_channel(observer.channel))
        embed = Embed.default_embed(
            title="Stream observer Info",
            description="Info about a stream observer.",
            author=inter.author.name,
            icon_url=inter.author.avatar.url
        )
        embed.add_field(name="ID", value=observer.id, inline=False)
        embed.add_field(name="Game ID", value=observer.game_id, inline=False)
        embed.add_field(name="Game Name", value=observer.game_name, inline=False)
        embed.add_field(name="Channel", value=channel.mention, inline=False)
        embed.add_field(name="Template", value=observer.template, inline=False)
        embed.add_field(name="End Template", value=observer.end_template, inline=False)
        embed.add_field(name="Blacklisted users", value=", ".join(observer.blacklist) if observer.blacklist else "None", inline=False)
        await inter.response.send_message(embed=embed)

    @stream_observer.sub_command(name="add", description="Observe a game for streams.")
    async def add(
        self,
        inter:    ApplicationCommandInteraction,
        game_id:      str = commands.Param(name="game-id", description="The ID of the game to watch.", min_length=1),
        template:     str = commands.Param(default=None, description="The template to use for the stream."),
        end_template: str = commands.Param(default=None, description="The template to use for the end of the stream. Gets appended to the message when the stream ends.")
    ):
        games = self.twitch_api.get_games(game_ids=[game_id])
        cnt, game = 0, None
        async for g in games:
            game = g
            cnt += 1
        if cnt != 1:
            await inter.response.send_message("Game not found.", ephemeral=True)
            return

        if not template:
            template = "{{user}} is playing {{game}} with {{viewer_count}} viewers.\n{{title}} - {{link}}"
        else:
            template = template.replace("\\n", "\n")

        if not end_template:
            end_template = "\n\n{{game}} is no longer being streamed."
        else:
            end_template = end_template.replace("\\n", "\n")

        observer = await db.streamobserver.create(
            data={
                "guild": inter.guild_id,
                "channel": inter.channel_id,
                "game_id": game_id,
                "game_name": game.name,
                "template": template,
                "end_template": end_template
            },
            include={
                "known_streams": True
            }
        )
        self.bot.loop.create_task(self.observe_game(observer))
        await inter.response.send_message(f"Added stream observer for {game.name}.")
        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji("TWITCH", f"A stream observer `{observer.id}` (`{game.id}` - `{game.name}`) has been added to {inter.channel.mention} by {inter.author.name} (`{inter.author.id}`)")
        )
        Logging.info(f"A stream observer {observer.id} ({game.id} - {game.name}) was added to channel {inter.channel.name} ({inter.channel.guild.name}) by {inter.author.name} (`{inter.author.id}`)")

    @stream_observer.sub_command(name="remove", description="Remove a stream observer.")
    async def remove(
        self,
        inter: ApplicationCommandInteraction,
        id: str = commands.Param(name="observer-id", description="The ID of the stream observer to blacklist the user from.", min_length=36, max_length=36)
    ):
        observer = await db.streamobserver.find_first(
            where={
                "id": id
            }
        )
        if not observer:
            await inter.response.send_message("No stream observer found with that id", ephemeral=True)
            return
        channel: disnake.abc.GuildChannel = Utils.coalesce(self.bot.get_channel(observer.channel), Utils.get_alternate_channel(observer.channel))
        await db.streamobserver.delete(
            where={
                "id": observer.id
            }
        )
        self.stop_requests.append(observer.id)
        await inter.response.send_message("Stream observer removed.")
        await Logging.guild_log(
            inter.guild_id,
            msg_with_emoji(
                "TWITCH",
                f"A stream observer `{observer.id}` (`{observer.game_id}` - `{observer.game_name}`) has been removed from {channel.mention} by {inter.author.name} (`{inter.author.id}`)"
                )
        )
        Logging.info(
            f"A stream observer {observer.id} ({observer.game_id} - {observer.game_name}) was removed from channel {channel.name} ({channel.guild.name}) by {inter.author.name} (`{inter.author.id}`)"
        )

    @stream_observer.sub_command(name="blacklist-user", description="Blacklist a Twitch user.")
    async def blacklist_add(
        self,
        inter:       ApplicationCommandInteraction,
        observer_id: str = commands.Param(name="observer-id", description="The ID of the stream observer to blacklist the user from.", min_length=36, max_length=36),
        user_id:     str = commands.Param(name="user-id", description="The user to blacklist.", min_length=1),
    ):
        observer = await db.streamobserver.find_first(
            where={
                "id": observer_id
            }
        )
        if not observer:
            await inter.response.send_message("No stream observer found with that id.", ephemeral=True)
            return
        if user_id in observer.blacklist:
            await inter.response.send_message("User already blacklisted.", ephemeral=True)
            return

        users = self.twitch_api.get_users(user_ids=[user_id])
        found, user = False, None
        async for u in users:
            user = u
            found = True

        await db.streamobserver.update(
            where={
                "id": observer.id
            },
            data={
                "blacklist": observer.blacklist + [user_id]
            }
        )
        if found:
            await inter.response.send_message(f"User `{user.display_name}` (`{user.id}`) added to the blacklist.")
        else:
            await inter.response.send_message(f"I didn't find a user with the id `{user_id}`, but I added them to the blacklist anyway.")
        await Logging.guild_log(
            observer.guild,
            msg_with_emoji("TWITCH", f"User `{user_id}` has been blacklisted from observer `{observer.id}` by {inter.author.name} (`{inter.author.id}`)")
        )
        Logging.info(f"User {user_id} was blacklisted for observer {observer.id} by {inter.author.name} (`{inter.author.id}`)")

    @stream_observer.sub_command(name="un-blacklist-user", description="Remove a user from the blacklist.")
    async def blacklist_remove(
        self,
        inter:       ApplicationCommandInteraction,
        observer_id: str = commands.Param(name="observer-id", description="The ID of the stream observer to un-blacklist the user from.", min_length=36, max_length=36),
        user_id:     str = commands.Param(name="user-id", description="The user to remove from the blacklist.", min_length=1),
    ):
        observer = await db.streamobserver.find_first(
            where={
                "id": observer_id
            }
        )
        if not observer:
            await inter.response.send_message("No stream observer found with that id.", ephemeral=True)
            return
        if user_id not in observer.blacklist:
            await inter.response.send_message("User not blacklisted.", ephemeral=True)
            return
        await db.streamobserver.update(
            where={
                "id": observer.id
            },
            data={
                "blacklist": [x for x in observer.blacklist if x != user_id]
            }
        )
        await inter.response.send_message(f"User `{user_id}` removed from the blacklist.")
        await Logging.guild_log(
            observer.guild,
            msg_with_emoji("TWITCH", f"User `{user_id}` has been removed from the blacklist from observer `{observer.id}` by {inter.author.name} (`{inter.author.id}`)")
        )
        Logging.info(f"User {user_id} was removed from the blacklist for observer {observer.id} by {inter.author.name} (`{inter.author.id}`)")

    async def observe_game(self, observer: StreamObserver):
        Logging.info(f"Starting observer for {observer.id} ({observer.game_id})")
        while observer.id not in self.stop_requests:
            try:
                streams = limit(self.twitch_api.get_streams(game_id=observer.game_id, stream_type="live"), self.max_concurrent_streams)
                async for stream in streams:
                    if stream.user_id in observer.blacklist \
                            or stream.game_id != observer.game_id \
                            or stream.is_mature:
                        continue
                    existing_stream, ks_id = await self.check_stream_known(stream, observer.id)
                    message_id = None
                    if not existing_stream:
                        Logging.info(f"Found new stream for {observer.id}: {stream.id}, {stream.user_name} is playing {stream.game_name} since {stream.started_at}")
                        message_id = await self.post_stream(observer, stream)
                    await self.update_known_stream(observer, stream, ks_id, message_id)
                observer = await db.streamobserver.find_first(
                    where={
                        "id": observer.id
                    },
                    include={
                        "known_streams": True
                    }
                )
                await self.remove_known_streams(observer)
                if len(observer.known_streams) == self.max_concurrent_streams:
                    await Logging.guild_log(
                        observer.guild,
                        msg_with_emoji("WARN", f"Stream observer `{observer.id}` (`{observer.game_id}`) has reached the maximum number of concurrent streams and is at risk of dropping streams.")
                    )
            except Exception as e:
                Logging.exception(f"Error in stream observer for {observer.id} ({observer.game_id})", e)
            await asyncio.sleep(self.refresh_interval)

    async def check_stream_known(self, stream: Stream, observerId: str):
        existing_stream = await db.knownstream.find_first(
            where={
                "OR": [
                    {
                        "streamObserverId": {"equals": observerId},
                        "stream_id": {"equals": stream.id}
                    },
                    {
                        "user_id": {"equals": stream.user_id},
                        "user_login": {"equals": stream.user_login}
                    }
                ]
            }
        )
        if existing_stream and existing_stream.stream_id != stream.id:
            Logging.info(f"Known stream {existing_stream.id} has changed stream id from {existing_stream.stream_id} to {stream.id}")
        ks_id = existing_stream.id if existing_stream else -1
        return bool(existing_stream), ks_id

    async def update_known_stream(self, observer: StreamObserver, stream: Stream, ks_id, message_id: int = None):
        current_time = datetime.datetime.now().replace(tzinfo=datetime.timezone.utc)
        await db.knownstream.upsert(
            where={
                "id": ks_id
            },
            data={
                "create": {
                    "stream_id": stream.id,
                    "user_id": stream.user_id,
                    "user_login": stream.user_login,
                    "last_seen": current_time,
                    "message_id": message_id,
                    "StreamObserver": {
                        "connect": {
                            "id": observer.id
                        }
                    }
                },
                "update": {
                    "last_seen": current_time,
                    "stream_id": stream.id
                }
            }
        )

    async def remove_known_streams(self, observer: StreamObserver):
        for ks in observer.known_streams:
            if (datetime.datetime.now().replace(tzinfo=datetime.timezone.utc) - ks.last_seen.replace(tzinfo=datetime.timezone.utc)).seconds > self.offline_threshold:
                message = self.bot.get_message(ks.message_id)
                if message:
                    await message.edit(content=message.content + observer.end_template.replace("{{game}}", observer.game_name))
                await db.knownstream.delete(
                    where={
                        "id": ks.id
                    }
                )
                Logging.info(f"Removed known stream {ks.stream_id} from observer {observer.id} ({observer.game_id})")

    async def post_stream(self, observer: StreamObserver, stream: Stream):
        channel = self.bot.get_channel(observer.channel)
        if not channel or not channel.permissions_for(channel.guild.me).send_messages:
            await Logging.guild_log(
                observer.guild,
                msg_with_emoji("WARN", f"Unable to post to channel {observer.channel} for stream observer `{observer.id}` (`{observer.game_id}` - `{observer.game_name}`)")
            )
            Logging.warning(f"Unable to post to channel {observer.channel} for feed {observer.id} ({observer.game_id} - {observer.game_name})")
            return
        await channel.trigger_typing()
        await asyncio.sleep(3)
        message = observer.template.replace("{{title}}", self.escape(stream.title))
        message = message.replace("{{user}}", self.escape(stream.user_name))
        message = message.replace("{{user_login}}", self.escape(stream.user_login))
        message = message.replace("{{game}}", self.escape(stream.game_name))
        message = message.replace("{{tags}}", ", ".join(f"`{x}`" for x in Utils.coalesce(stream.tags, [])))
        message = message.replace("{{viewer_count}}", str(stream.viewer_count))
        message = message.replace("{{link}}", f"https://www.twitch.tv/{stream.user_login}")
        msg = await channel.send(message)
        return msg.id

    def escape(self, text: str):
        for char in ["_", "*", "~", "`", "|"]:
            text = text.replace(char, f"\\{char}")
        return text


def setup(bot: commands.Bot):
    bot.add_cog(Streams(bot))
