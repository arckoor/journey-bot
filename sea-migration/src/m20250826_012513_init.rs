use sea_orm_migration::{
    prelude::{extension::postgres::Type, *},
    schema::*,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let current_ts = Expr::cust(r"EXTRACT(epoch FROM now())");

        manager
            .create_table(
                Table::create()
                    .table(GuildConfig::Table)
                    .if_not_exists()
                    .col(big_integer(GuildConfig::Id).primary_key())
                    .col(big_integer_null(GuildConfig::GuildLog))
                    .col(string(GuildConfig::TimeZone).default("UTC"))
                    .col(integer(GuildConfig::NewUserThreshold).default(14))
                    .col(double(GuildConfig::OnboardingActiveSince).default(0.0))
                    .col(
                        array(GuildConfig::TrustedRoles, ColumnType::BigInteger)
                            .default(Vec::<String>::new()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(EnsuredRole::Table)
                    .if_not_exists()
                    .primary_key(
                        Index::create()
                            .col(EnsuredRole::GuildId)
                            .col(EnsuredRole::RoleId),
                    )
                    .col(big_integer(EnsuredRole::GuildId))
                    .col(big_integer(EnsuredRole::RoleId))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RedditFeed::Table)
                    .if_not_exists()
                    .col(string(RedditFeed::Id).primary_key())
                    .col(big_integer(RedditFeed::GuildId))
                    .col(big_integer(RedditFeed::ChannelId))
                    .col(string(RedditFeed::Subreddit))
                    .col(text(RedditFeed::Template))
                    .col(double(RedditFeed::LatestPost).default(current_ts.clone()))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(StreamObserver::Table)
                    .if_not_exists()
                    .col(string(StreamObserver::Id).primary_key())
                    .col(big_integer(StreamObserver::GuildId))
                    .col(big_integer(StreamObserver::ChannelId))
                    .col(string(StreamObserver::GameId))
                    .col(string(StreamObserver::GameName))
                    .col(text(StreamObserver::Template))
                    .col(text(StreamObserver::EndTemplate))
                    .col(array(StreamObserver::Blacklist, ColumnType::Text))
                    .col(string_null(StreamObserver::AutoBlacklistSheetId))
                    .col(string_null(StreamObserver::AutoBlacklistColumnName))
                    .col(array(StreamObserver::AutoBlacklist, ColumnType::Text))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(KnownStream::Table)
                    .if_not_exists()
                    .col(integer(KnownStream::Id).primary_key().auto_increment())
                    .col(string(KnownStream::StreamId))
                    .col(string(KnownStream::StreamObserverId))
                    .col(string(KnownStream::UserId))
                    .col(string(KnownStream::UserLogin))
                    .col(double(KnownStream::FirstSeen).default(current_ts.clone()))
                    .col(double(KnownStream::LastSeen).default(current_ts.clone()))
                    .col(big_integer_null(KnownStream::MessageId))
                    .foreign_key(
                        ForeignKey::create()
                            .from(KnownStream::Table, KnownStream::StreamObserverId)
                            .to(StreamObserver::Table, StreamObserver::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PostedStream::Table)
                    .if_not_exists()
                    .col(big_integer(PostedStream::MessageId).primary_key())
                    .col(string(PostedStream::UserLogin))
                    .col(string(PostedStream::StreamObserverId))
                    .col(double(PostedStream::CreatedAt).default(current_ts.clone()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(PostedStream::Table, PostedStream::StreamObserverId)
                            .to(StreamObserver::Table, StreamObserver::Id)
                            .on_update(ForeignKeyAction::Cascade)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(StickyMessage::Table)
                    .if_not_exists()
                    .col(string(StickyMessage::Id).primary_key())
                    .col(big_integer(StickyMessage::GuildId))
                    .col(big_integer(StickyMessage::ChannelId))
                    .col(text(StickyMessage::Content))
                    .col(integer(StickyMessage::MessagesSince))
                    .col(big_integer_null(StickyMessage::CurrentId))
                    .col(integer(StickyMessage::MessageLimit))
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(Punishment::Enum)
                    .values([Punishment::Mute, Punishment::Ban])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AntiSpamConfig::Table)
                    .if_not_exists()
                    .col(big_integer(AntiSpamConfig::Id).primary_key())
                    .col(
                        ColumnDef::new(AntiSpamConfig::Punishment)
                            .custom(Punishment::Enum)
                            .not_null(),
                    )
                    .col(integer(AntiSpamConfig::TimeoutDuration))
                    .col(array(AntiSpamConfig::MaxMessages, ColumnType::Integer))
                    .col(array(
                        AntiSpamConfig::SimilarMessageThreshold,
                        ColumnType::Double,
                    ))
                    .col(double(AntiSpamConfig::SimilarMessageRePunishThreshold))
                    .col(integer(AntiSpamConfig::TimeFrame))
                    .col(boolean(AntiSpamConfig::CleanUser))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PunishedMessage::Table)
                    .if_not_exists()
                    .col(integer(PunishedMessage::Id).primary_key().auto_increment())
                    .col(text(PunishedMessage::Content))
                    .col(double(PunishedMessage::Timestamp))
                    .col(big_integer(PunishedMessage::AntiSpamConfigId))
                    .foreign_key(
                        ForeignKey::create()
                            .from(PunishedMessage::Table, PunishedMessage::AntiSpamConfigId)
                            .to(AntiSpamConfig::Table, AntiSpamConfig::Id)
                            .on_update(ForeignKeyAction::Cascade)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(CensorConfig::Table)
                    .if_not_exists()
                    .col(big_integer(CensorConfig::Id).primary_key())
                    .col(big_integer_null(CensorConfig::LogChannel))
                    .col(
                        array(CensorConfig::CensorList, ColumnType::Text)
                            .default(Vec::<String>::new()),
                    )
                    .col(
                        array(CensorConfig::AutoCensorList, ColumnType::Text)
                            .default(Vec::<String>::new()),
                    )
                    .col(string_null(CensorConfig::AutoCensorListSheetId))
                    .col(
                        array(CensorConfig::AutoCensorListColumnNames, ColumnType::Text)
                            .default(Vec::<String>::new()),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table(GuildConfig::Table)
                    .table(EnsuredRole::Table)
                    .table(RedditFeed::Table)
                    .table(KnownStream::Table)
                    .table(PostedStream::Table)
                    .table(StreamObserver::Table)
                    .table(StickyMessage::Table)
                    .table(PunishedMessage::Table)
                    .table(AntiSpamConfig::Table)
                    .table(CensorConfig::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_type(
                Type::drop()
                    .if_exists()
                    .names([SeaRc::new(Punishment::Enum) as DynIden])
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum GuildConfig {
    Table,
    Id,
    GuildLog,
    TimeZone,
    NewUserThreshold,
    OnboardingActiveSince,
    TrustedRoles,
}

#[derive(DeriveIden)]
enum EnsuredRole {
    Table,
    GuildId,
    RoleId,
}

#[derive(DeriveIden)]
enum RedditFeed {
    Table,
    Id,
    GuildId,
    ChannelId,
    Subreddit,
    Template,
    LatestPost,
}

#[derive(DeriveIden)]
enum StreamObserver {
    Table,
    Id,
    GuildId,
    ChannelId,
    GameId,
    GameName,
    Template,
    EndTemplate,
    Blacklist,
    AutoBlacklistSheetId,
    AutoBlacklistColumnName,
    AutoBlacklist,
}

#[derive(DeriveIden)]
enum KnownStream {
    Table,
    Id,
    StreamId,
    UserId,
    UserLogin,
    FirstSeen,
    LastSeen,
    MessageId,
    StreamObserverId,
}

#[derive(DeriveIden)]
enum PostedStream {
    Table,
    MessageId,
    UserLogin,
    StreamObserverId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum StickyMessage {
    Table,
    Id,
    GuildId,
    ChannelId,
    Content,
    MessagesSince,
    CurrentId,
    MessageLimit,
}

#[derive(DeriveIden)]
enum Punishment {
    #[sea_orm(iden = "punishment")]
    Enum,
    Mute,
    Ban,
}

#[derive(DeriveIden)]
enum AntiSpamConfig {
    Table,
    Id,
    Punishment,
    TimeoutDuration,
    MaxMessages,
    SimilarMessageThreshold,
    SimilarMessageRePunishThreshold,
    TimeFrame,
    CleanUser,
}

#[derive(DeriveIden)]
enum PunishedMessage {
    Table,
    Id,
    Content,
    Timestamp,
    AntiSpamConfigId,
}

#[derive(DeriveIden)]
enum CensorConfig {
    Table,
    Id,
    LogChannel,
    CensorList,
    AutoCensorList,
    AutoCensorListSheetId,
    AutoCensorListColumnNames,
}
