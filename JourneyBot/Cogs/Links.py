import json
import re
import rapidfuzz
import os

from disnake import ApplicationCommandInteraction
from disnake.ext import commands

from Cogs.BaseCog import BaseCog
from Views.Paginator import LinkPaginator

LINKS_FILE = "config/links.json"


async def split_strings(inter: ApplicationCommandInteraction, arg: str) -> list[str]:
    shorthands = []
    for shorthand in re.split("[, ]+", arg):
        shorthands.append(shorthand)
    return shorthands


class Links(BaseCog):
    def __init__(self, bot: commands.Bot):
        super().__init__(bot)
        self.links: dict[str, list[str]] = {}
        self.reverse_map: dict[str, str] = {}
        self.load_links()

    @commands.slash_command(description="Find links.")
    @commands.guild_only()
    @commands.bot_has_permissions(send_messages=True)
    async def link(self, inter: ApplicationCommandInteraction):
        pass

    @link.sub_command(description="Find a link to a topic.")
    async def find(
        self,
        inter: ApplicationCommandInteraction,
        topic: str = commands.Param(description="The topic to find a link to."),
    ):
        topic = topic.lower()
        link = self.reverse_map.get(topic)
        if link:
            await inter.response.send_message(link)
            return
        matches = rapidfuzz.process.extract(topic, self.reverse_map.keys(), limit=3, score_cutoff=70)
        if matches:
            best_match, score, _ = matches[0]
            if score >= 90:
                await inter.response.send_message(
                    f"Assuming you meant `{best_match}`: {self.reverse_map.get(best_match)}"
                )
                return
            suggestions = ", ".join(f"`{m[0]}`" for m in matches)
            await inter.response.send_message(
                f"No exact match found for {topic}. Did you mean {suggestions}?", ephemeral=True
            )
            return
        await inter.response.send_message("I don't know that link.", ephemeral=True)

    @link.sub_command(description="Browse all available links.")
    async def browse(self, inter: ApplicationCommandInteraction):
        if not self.links:
            await inter.response.send_message("Currently no links registered.", ephemeral=True)
            return
        paginator = LinkPaginator(self.links, inter)
        await inter.response.send_message(embed=paginator.get_embed(), view=paginator, ephemeral=True)

    @commands.slash_command(name="link-config", description="Manage links.")
    @commands.is_owner()
    @commands.default_member_permissions(ban_members=True)
    async def link_config(self, inter: ApplicationCommandInteraction):
        pass

    @link_config.sub_command(description="Add a link.")
    async def add(
        self,
        inter: ApplicationCommandInteraction,
        link: str = commands.Param(description="The link to add"),
        shorthands: str = commands.Param(
            description="The shorthands that point to the link. Comma separated.", converter=split_strings
        ),
    ):
        if link in self.links:
            self.links[link].extend([x for x in shorthands if x not in self.links[link]])
        else:
            self.links[link] = shorthands
        self.save_links()
        self.build_rev_map()
        await inter.response.send_message("Link added.")

    @link_config.sub_command(description="Remove a link.")
    async def remove(
        self, inter: ApplicationCommandInteraction, link: str = commands.Param(description="The link to remove.")
    ):
        if link not in self.links:
            await inter.response.send_message("No link found", ephemeral=True)
            return
        del self.links[link]
        self.save_links()
        self.build_rev_map()
        await inter.response.send_message("Link removed.")

    def load_links(self):
        if not os.path.exists(LINKS_FILE):
            with open(LINKS_FILE, "w") as file:
                json.dump({}, file)
        with open(LINKS_FILE, "r") as file:
            self.links = json.load(file)
        self.build_rev_map()

    def save_links(self):
        with open(LINKS_FILE, "w") as file:
            json.dump(self.links, file, indent=4, skipkeys=True, sort_keys=True)

    def build_rev_map(self):
        self.reverse_map = {}
        for link, aliases in self.links.items():
            for alias in aliases:
                self.reverse_map[alias.lower()] = f"<{link}>"


def setup(bot: commands.Bot):
    bot.add_cog(Links(bot))
