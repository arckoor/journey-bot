use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BypassRole::Table)
                    .if_not_exists()
                    .col(string(BypassRole::Id).primary_key())
                    .col(big_integer(BypassRole::GuildId))
                    .col(big_integer(BypassRole::RoleId))
                    .col(big_integer(BypassRole::ChannelId))
                    .col(integer(BypassRole::Timeout))
                    .col(string(BypassRole::Name))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AssignedBypassRole::Table)
                    .if_not_exists()
                    .col(
                        integer(AssignedBypassRole::Id)
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(big_integer(AssignedBypassRole::UserId))
                    .col(double(AssignedBypassRole::Timestamp))
                    .col(string(AssignedBypassRole::BypassRoleId))
                    .foreign_key(
                        ForeignKey::create()
                            .from(AssignedBypassRole::Table, AssignedBypassRole::BypassRoleId)
                            .to(BypassRole::Table, BypassRole::Id)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Cascade),
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
                    .table(BypassRole::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum BypassRole {
    Table,
    Id,
    GuildId,
    RoleId,
    ChannelId,
    Timeout,
    Name,
}

#[derive(DeriveIden)]
enum AssignedBypassRole {
    Table,
    Id,
    UserId,
    Timestamp,
    BypassRoleId,
}
