use sea_orm_migration::{prelude::*, sea_orm::Statement};

use crate::{
    m20250826_012513_init::{EnsuredRole, GuildConfig},
    m20260121_221856_auto_roles::AutoRole,
};

fn iden_to_str(id: impl Iden) -> String {
    let mut buf = String::new();
    id.unquoted(&mut buf);
    buf
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        manager
            .alter_table(
                Table::alter()
                    .table(GuildConfig::Table)
                    .drop_column(GuildConfig::OnboardingActiveSince)
                    .to_owned(),
            )
            .await?;

        let select = Query::select()
            .columns([EnsuredRole::GuildId, EnsuredRole::RoleId])
            .from(EnsuredRole::Table)
            .build(PostgresQueryBuilder);

        for row in db
            .query_all(Statement::from_sql_and_values(backend, select.0, select.1))
            .await?
        {
            let guild_id: i64 = row.try_get("", &iden_to_str(EnsuredRole::GuildId))?;
            let role_id: i64 = row.try_get("", &iden_to_str(EnsuredRole::RoleId))?;

            let insert = Query::insert()
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
                ])
                .build(PostgresQueryBuilder);

            db.execute(Statement::from_sql_and_values(backend, insert.0, insert.1))
                .await?;
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
