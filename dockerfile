FROM python:3.10-slim

WORKDIR /JourneyBot

COPY requirements.txt .

RUN pip install --no-cache-dir -r requirements.txt

COPY loader.sh /usr/local/bin/

RUN chmod +x /usr/local/bin/loader.sh

COPY . .

RUN prisma generate

ENTRYPOINT [ "loader.sh" ]
CMD [ "python", "JourneyBot/JourneyBot.py" ]
