# journey-bot

A bot to take care of various little things in the Journey Discord.

## Features
- Fully configurable sticky messages
- Support for subscribing to Subreddits and sending their posts to a channel
- React-Spam removal tool
- Anti-Spam measures regarding text messages, but using string distance algorithms instead of the usual direct comparison
- Able to automatically assign roles everyone should have

## Installation
- Pull the docker image using `docker compose pull`
- create a `.env` file with `PG_PASSWORD=...` and `ENV=prod`
- modify the `config/master.json` to your liking
- run `docker compose up`
