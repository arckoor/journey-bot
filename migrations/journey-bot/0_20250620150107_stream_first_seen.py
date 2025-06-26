from tortoise import BaseDBAsyncClient


async def upgrade(db: BaseDBAsyncClient) -> str:
    return """
ALTER TABLE "knownstream"
ADD first_seen timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL;
"""


async def downgrade(db: BaseDBAsyncClient) -> str:
    return """
ALTER TABLE "knownstream"
DROP COLUMN first_seen;
"""
