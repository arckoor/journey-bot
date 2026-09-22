use sea_orm_migration::{prelude::*, schema::*};

use crate::m20250826_012513_init::RedditFeed;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RedditFeed::Table)
                    .add_column_if_not_exists(integer_null(AlteredRedditFeed::ActivityTimeout))
                    .to_owned(),
            )
            .await?;

        manager
            .exec_stmt(
                Query::update()
                    .table(RedditFeed::Table)
                    .value(
                        AlteredRedditFeed::ActivityTimeout,
                        Expr::value(60 * 60 * 24 * 7),
                    )
                    .and_where(Expr::col(AlteredRedditFeed::ActivityTimeout).is_null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(RedditFeed::Table)
                    .modify_column(integer(AlteredRedditFeed::ActivityTimeout).not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RedditFeed::Table)
                    .drop_column(AlteredRedditFeed::ActivityTimeout)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum AlteredRedditFeed {
    ActivityTimeout,
}
