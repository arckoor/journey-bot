from tortoise import BaseDBAsyncClient


async def upgrade(db: BaseDBAsyncClient) -> str:
    return """
DROP TABLE IF EXISTS "redditflair" CASCADE;
DROP TABLE IF EXISTS "redditautoreply" CASCADE;
"""


async def downgrade(db: BaseDBAsyncClient) -> str:
    return """
CREATE TABLE IF NOT EXISTS "redditautoreply" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    subreddit text NOT NULL,
    latest_post timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    management_role bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS "redditflair" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    flair_name text NOT NULL,
    flair_reply text NOT NULL,
    auto_reply_id integer NOT NULL references "redditautoreply"(id) ON UPDATE CASCADE ON DELETE CASCADE
);
"""
