-- CreateTable
CREATE TABLE "StickyMessage" (
    "id" TEXT NOT NULL,
    "channel" BIGINT NOT NULL,
    "guild" BIGINT NOT NULL,
    "author" BIGINT NOT NULL,
    "content" TEXT NOT NULL,
    "last_sent" DOUBLE PRECISION NOT NULL,
    "messages_since" INTEGER NOT NULL,
    "active" BOOLEAN NOT NULL DEFAULT true,
    "current_id" BIGINT,
    "message_limit" INTEGER DEFAULT 0,
    "time_limit" INTEGER DEFAULT 0,
    "delete_old_sticky" BOOLEAN DEFAULT true,

    CONSTRAINT "StickyMessage_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "RedditFeed" (
    "id" TEXT NOT NULL,
    "guild" BIGINT NOT NULL,
    "channel" BIGINT NOT NULL,
    "subreddit" TEXT NOT NULL,
    "template" TEXT NOT NULL,
    "latest_post" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "RedditFeed_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "StreamObserver" (
    "id" TEXT NOT NULL,
    "guild" BIGINT NOT NULL,
    "channel" BIGINT NOT NULL,
    "game_id" TEXT NOT NULL,
    "game_name" TEXT NOT NULL,
    "template" TEXT NOT NULL,
    "end_template" TEXT NOT NULL,
    "blacklist" TEXT[] DEFAULT ARRAY[]::TEXT[],

    CONSTRAINT "StreamObserver_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "KnownStream" (
    "id" SERIAL NOT NULL,
    "stream_id" TEXT NOT NULL,
    "user_id" TEXT NOT NULL,
    "user_login" TEXT NOT NULL,
    "last_seen" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "message_id" BIGINT,
    "streamObserverId" TEXT NOT NULL,

    CONSTRAINT "KnownStream_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "EnsuredRole" (
    "id" SERIAL NOT NULL,
    "guild" BIGINT NOT NULL,
    "role" BIGINT NOT NULL,

    CONSTRAINT "EnsuredRole_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "GuildConfig" (
    "id" SERIAL NOT NULL,
    "guild" BIGINT NOT NULL,
    "guild_log" BIGINT,
    "time_zone" TEXT DEFAULT 'UTC',
    "onboarding_active_since" TIMESTAMP(3) NOT NULL DEFAULT '1970-01-01 00:00:00 +00:00',
    "react_remove_excluded_channels" BIGINT[] DEFAULT ARRAY[]::BIGINT[],
    "react_remove_greedy_limit" INTEGER DEFAULT 25,
    "new_user_threshold" INTEGER DEFAULT 14,

    CONSTRAINT "GuildConfig_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "AntiSpamConfig" (
    "id" SERIAL NOT NULL,
    "guild" BIGINT NOT NULL,
    "enabled" BOOLEAN NOT NULL DEFAULT false,
    "punishment" TEXT DEFAULT 'mute',
    "mute_role" BIGINT,
    "max_messages" INTEGER[] DEFAULT ARRAY[5]::INTEGER[],
    "similar_message_threshold" DOUBLE PRECISION[] DEFAULT ARRAY[0.95]::DOUBLE PRECISION[],
    "similar_message_re_ban_threshold" DOUBLE PRECISION DEFAULT 0.95,
    "time_frame" INTEGER DEFAULT 300,
    "trusted_users" BIGINT[] DEFAULT ARRAY[]::BIGINT[],
    "trusted_roles" BIGINT[] DEFAULT ARRAY[]::BIGINT[],
    "ignored_channels" BIGINT[] DEFAULT ARRAY[]::BIGINT[],

    CONSTRAINT "AntiSpamConfig_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "PunishedMessage" (
    "id" SERIAL NOT NULL,
    "guild" BIGINT NOT NULL,
    "content" TEXT NOT NULL,
    "timestamp" DOUBLE PRECISION NOT NULL,
    "antiSpamConfigId" INTEGER NOT NULL,

    CONSTRAINT "PunishedMessage_pkey" PRIMARY KEY ("id")
);

-- CreateIndex
CREATE UNIQUE INDEX "StickyMessage_channel_key" ON "StickyMessage"("channel");

-- CreateIndex
CREATE INDEX "StickyMessage_channel_idx" ON "StickyMessage"("channel");

-- CreateIndex
CREATE INDEX "KnownStream_stream_id_idx" ON "KnownStream"("stream_id");

-- CreateIndex
CREATE UNIQUE INDEX "KnownStream_stream_id_streamObserverId_key" ON "KnownStream"("stream_id", "streamObserverId");

-- CreateIndex
CREATE UNIQUE INDEX "EnsuredRole_guild_role_key" ON "EnsuredRole"("guild", "role");

-- CreateIndex
CREATE UNIQUE INDEX "GuildConfig_guild_key" ON "GuildConfig"("guild");

-- CreateIndex
CREATE INDEX "GuildConfig_guild_idx" ON "GuildConfig"("guild");

-- CreateIndex
CREATE UNIQUE INDEX "AntiSpamConfig_guild_key" ON "AntiSpamConfig"("guild");

-- CreateIndex
CREATE INDEX "AntiSpamConfig_guild_idx" ON "AntiSpamConfig"("guild");

-- CreateIndex
CREATE UNIQUE INDEX "PunishedMessage_guild_content_key" ON "PunishedMessage"("guild", "content");

-- AddForeignKey
ALTER TABLE "KnownStream" ADD CONSTRAINT "KnownStream_streamObserverId_fkey" FOREIGN KEY ("streamObserverId") REFERENCES "StreamObserver"("id") ON DELETE CASCADE ON UPDATE CASCADE;

-- AddForeignKey
ALTER TABLE "PunishedMessage" ADD CONSTRAINT "PunishedMessage_antiSpamConfigId_fkey" FOREIGN KEY ("antiSpamConfigId") REFERENCES "AntiSpamConfig"("id") ON DELETE CASCADE ON UPDATE CASCADE;
