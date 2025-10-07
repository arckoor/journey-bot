use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let current_ts = Expr::cust(r"EXTRACT(epoch FROM now())");

        manager
            .create_table(
                Table::create()
                    .table(StagedCensorItem::Table)
                    .if_not_exists()
                    .col(integer(StagedCensorItem::Id).primary_key().auto_increment())
                    .col(string(StagedCensorItem::Item))
                    .col(string(StagedCensorItem::ForeignId))
                    .col(double(StagedCensorItem::CreatedAt).default(current_ts.clone()))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(StagedCensorItem::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum StagedCensorItem {
    Table,
    Id,
    Item,
    ForeignId,
    CreatedAt,
}
