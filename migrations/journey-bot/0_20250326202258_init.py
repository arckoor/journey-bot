from tortoise import BaseDBAsyncClient


async def upgrade(db: BaseDBAsyncClient) -> str:
    return """
CREATE TABLE IF NOT EXISTS "aerich" (
    "id" SERIAL NOT NULL PRIMARY KEY,
    "version" VARCHAR(255) NOT NULL,
    "app" VARCHAR(100) NOT NULL,
    "content" JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS "guildconfig" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL UNIQUE,
    guild_log bigint,
    time_zone text NOT NULL,
    onboarding_active_since timestamp with time zone DEFAULT '1970-01-01 01:00:00+00'::timestamp with time zone NOT NULL,
    react_remove_excluded_channels bigint[] DEFAULT '{}'::bigint[] NOT NULL,
    react_remove_greedy_limit integer DEFAULT 25 NOT NULL,
    new_user_threshold integer DEFAULT 14 NOT NULL
);

CREATE TABLE IF NOT EXISTS "antispamconfig" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL UNIQUE,
    enabled boolean DEFAULT false NOT NULL,
    punishment text NOT NULL,
    timeout_duration integer,
    clean_user boolean DEFAULT false NOT NULL,
    max_messages integer[] DEFAULT '{5}'::integer[] NOT NULL,
    similar_message_threshold double precision[] DEFAULT '{0.95}'::double precision[] NOT NULL,
    similar_message_re_ban_threshold double precision DEFAULT 0.95 NOT NULL,
    time_frame integer DEFAULT 300 NOT NULL,
    trusted_users bigint[] DEFAULT '{}'::bigint[] NOT NULL,
    trusted_roles bigint[] DEFAULT '{}'::bigint[] NOT NULL,
    ignored_channels bigint[] DEFAULT '{}'::bigint[] NOT NULL
);

CREATE TABLE IF NOT EXISTS "punishedmessage" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    content text NOT NULL,
    "timestamp" double precision NOT NULL,
    anti_spam_config_id integer NOT NULL references "antispamconfig"(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "ensuredrole" (
    id SERIAL NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    role bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS "redditfeed" (
    id uuid NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    channel bigint NOT NULL,
    subreddit text NOT NULL,
    template text NOT NULL,
    latest_post timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS "stickymessage" (
    id uuid NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    channel bigint NOT NULL UNIQUE,
    author bigint NOT NULL,
    content text NOT NULL,
    last_sent double precision NOT NULL,
    messages_since integer NOT NULL,
    active boolean DEFAULT true NOT NULL,
    current_id bigint,
    message_limit integer DEFAULT 0 NOT NULL,
    time_limit integer DEFAULT 0 NOT NULL,
    delete_old_sticky boolean DEFAULT true NOT NULL
);

CREATE TABLE IF NOT EXISTS "streamobserver" (
    id uuid NOT NULL PRIMARY KEY,
    guild bigint NOT NULL,
    channel bigint NOT NULL,
    game_id text NOT NULL,
    game_name text NOT NULL,
    template text NOT NULL,
    end_template text NOT NULL,
    blacklist text[] DEFAULT '{}'::text[] NOT NULL
);

CREATE TABLE IF NOT EXISTS "knownstream" (
    id SERIAL NOT NULL PRIMARY KEY,
    stream_id text NOT NULL,
    user_id text NOT NULL,
    user_login text NOT NULL,
    last_seen timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    message_id bigint,
    stream_observer_id uuid NOT NULL references "streamobserver"(id) ON UPDATE CASCADE ON DELETE CASCADE
);

ALTER TABLE ONLY "ensuredrole"
    ADD CONSTRAINT uid_ensuredrole_guild_f98edd UNIQUE (guild, role);

ALTER TABLE ONLY "knownstream"
    ADD CONSTRAINT uid_knownstream_stream__73a392 UNIQUE (stream_id, stream_observer_id);

ALTER TABLE ONLY "punishedmessage"
    ADD CONSTRAINT uid_punishedmes_guild_053887 UNIQUE (guild, content);

CREATE INDEX idx_antispamcon_guild_b0c3e7 ON "antispamconfig" USING btree (guild);

CREATE INDEX idx_ensuredrole_guild_f5f10a ON "ensuredrole" USING btree (guild);

CREATE INDEX idx_ensuredrole_role_fc7f98 ON "ensuredrole" USING btree (role);

CREATE INDEX idx_guildconfig_guild_c357ed ON "guildconfig" USING btree (guild);

CREATE INDEX idx_punishedmes_guild_c5db00 ON "punishedmessage" USING btree (guild);

CREATE INDEX idx_redditfeed_guild_f96080 ON "redditfeed" USING btree (guild);

CREATE INDEX idx_stickymessa_channel_1808a1 ON "stickymessage" USING btree (channel);

CREATE INDEX idx_stickymessa_guild_50426a ON "stickymessage" USING btree (guild);

CREATE INDEX idx_streamobser_guild_e4b0ed ON "streamobserver" USING btree (guild);
"""


async def downgrade(db: BaseDBAsyncClient) -> str:
    return """
DROP TABLE IF EXISTS "knownstream" CASCADE;
DROP TABLE IF EXISTS "streamobserver" CASCADE;
DROP TABLE IF EXISTS "stickymessage" CASCADE;
DROP TABLE IF EXISTS "redditfeed" CASCADE;
DROP TABLE IF EXISTS "ensuredrole" CASCADE;
DROP TABLE IF EXISTS "punishedmessage" CASCADE;
DROP TABLE IF EXISTS "antispamconfig" CASCADE;
DROP TABLE IF EXISTS "guildconfig" CASCADE;
DROP TABLE IF EXISTS "aerich" CASCADE;
"""
