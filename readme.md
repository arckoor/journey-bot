# journey-bot
A bot to take care of the various little things in the Journey Discord.

## Features
This bot is very overspecialised to our needs, and makes a lot of assumptions about its environment, so probably don't try to run it.
Here's a list of features anyway:

- sticky messages
- notify on new posts to a subreddit
- notify on new streams for a game
- robust repeat message spam detection
- keep a certain role assigned to all members
- ... and more

## Installation
- Pull the docker image using `docker compose pull`
- create a `.env` file with `PG_PASSWORD=...`
- rename [config.example.toml](/config.example.toml) to `config.toml` and modify it to your liking
- make sure you have a `links.json` with at least `{}` in it in the current directory
- run `docker compose up -d`
