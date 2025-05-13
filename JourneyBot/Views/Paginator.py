from disnake import ButtonStyle, Interaction
from disnake.ui import View, Button

from Views.Embed import default_embed


class LinkPaginator(View):
    def __init__(self, data: dict[str, list[str]], inter: Interaction, per_page: int = 10):
        super().__init__(timeout=180)
        self.data = list(data.items())
        self.inter = inter
        self.per_page = per_page
        self.page = 0
        self.max_pages = (len(self.data) - 1) // per_page

        self.prev_button = Button(label="Previous", style=ButtonStyle.secondary)
        self.next_button = Button(label="Next", style=ButtonStyle.secondary)

        self.prev_button.callback = self.prev_page
        self.next_button.callback = self.next_page

        self.add_item(self.prev_button)
        self.add_item(self.next_button)

    def get_embed(self):
        embed = default_embed(title="Available Links")

        start = self.page * self.per_page
        end = start + self.per_page

        for link, aliases in self.data[start:end]:
            embed.add_field(name=", ".join(f"`{a}`" for a in aliases), value=link, inline=False)

        embed.set_footer(
            text=f"Requested by {self.inter.user.display_name} | Page {self.page + 1} of {self.max_pages + 1}",
            icon_url=self.inter.user.display_avatar.url,
        )
        return embed

    async def prev_page(self, inter: Interaction):
        if self.page > 0:
            self.page -= 1
        await inter.response.edit_message(embed=self.get_embed(), view=self)

    async def next_page(self, inter: Interaction):
        if self.page < self.max_pages:
            self.page += 1
        await inter.response.edit_message(embed=self.get_embed(), view=self)
