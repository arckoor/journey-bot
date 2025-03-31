FROM python:3.13-slim

WORKDIR /JourneyBot

COPY . .

RUN pip install --no-cache-dir -r requirements.txt

CMD [ "python", "JourneyBot/JourneyBot.py" ]
