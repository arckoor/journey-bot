use sea_orm_migration::prelude::*;

use crate::{
    m20250826_012513_init::{EnsuredRole, GuildConfig},
    m20260121_221856_auto_roles::AutoRole,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        manager
            .alter_table(
                Table::alter()
                    .table(GuildConfig::Table)
                    .drop_column(GuildConfig::OnboardingActiveSince)
                    .to_owned(),
            )
            .await?;

        let mut q = Query::select();
        let select = q
            .columns([EnsuredRole::GuildId, EnsuredRole::RoleId])
            .from(EnsuredRole::Table);

        for row in db.query_all(select).await? {
            let guild_id: i64 = row.try_get("", EnsuredRole::GuildId.unquoted())?;
            let role_id: i64 = row.try_get("", EnsuredRole::RoleId.unquoted())?;

            let mut q = Query::insert();
            let insert = q
                .into_table(AutoRole::Table)
                .columns([
                    AutoRole::Id,
                    AutoRole::GuildId,
                    AutoRole::Required,
                    AutoRole::Granted,
                ])
                .values_panic([
                    cuid2::slug().into(),
                    guild_id.into(),
                    Expr::cust("ARRAY[]::BIGINT[]"),
                    role_id.into(),
                ]);

            db.execute(insert).await?;
        }

        manager
            .drop_table(Table::drop().table(EnsuredRole::Table).to_owned())
            .await?;

        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        panic!("This migration is not reversible")
    }
}
