# journey-bot

A bot to take care of various little things in the Journey Discord.

## Features
- Fully configurable sticky messages
- Support for sending reddit RSS feeds to a channel
- React-Spam removal tool
- Anti-Spam measures regarding text messages, but using string distance algorithms instead of the usual direct comparison

## Installation
- Create a new python environment (python 3.10) and install all dependencies listed in the [requirements.txt](/requirements.txt) file
- You will also need a running PostgreSQL instance
- Create a new postgres user `journey_bot` and make it own a new database `journey-bot-database`
- Add `DATABASE_URL="postgresql://username:password@postgres_url/journey-bot-db?schema=jbot"` to your `.env` file
- Run `prisma db push`
- Copy / rename the [master.json.example](/config/master.json.example) file to `master.json`
- Create a new application in your discord developer portal, and copy the bot token to your newly created `master.json` file
- Modify the `master.json` file to suit your needs
- Finally run the [JourneyBot.py](/JourneyBot/JourneyBot.py) file
