use std::{sync::Arc, time::Duration};

use poise::serenity_prelude::GuildId;
use sea_migration::{Migrator, MigratorTrait};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectOptions, DatabaseConnection, EntityTrait,
    IntoActiveModel, PrimaryKeyTrait,
};

use crate::{Context, Error, store::Store, utils::BotError};

pub trait WithGuildId {
    fn set_guild_id(&mut self, guild: u64);
}

impl WithGuildId for sea_entity::guild_config::ActiveModel {
    fn set_guild_id(&mut self, guild: u64) {
        self.id = Set(guild as i64);
    }
}

impl WithGuildId for sea_entity::censor_config::ActiveModel {
    fn set_guild_id(&mut self, guild: u64) {
        self.id = Set(guild as i64);
    }
}

pub struct Database {
    pub sea: DatabaseConnection,
}

impl Database {
    pub async fn new(postgres_url: &str) -> Self {
        let mut opt = ConnectOptions::new(postgres_url);
        opt.sqlx_slow_statements_logging_settings(
            tracing::log::LevelFilter::Warn,
            Duration::from_millis(100),
        )
        .acquire_timeout(Duration::from_secs(2))
        .max_connections(80);

        #[cfg(not(debug_assertions))]
        opt.sqlx_logging(false);

        let sea = sea_orm::Database::connect(opt)
            .await
            .expect("Failed to create SeaORM connection");

        Migrator::up(&sea, None)
            .await
            .expect("Failed to migrate database");

        Self { sea }
    }

    pub async fn get_or_create_guild_config(
        &self,
        guild: u64,
    ) -> Result<sea_entity::guild_config::Model, BotError> {
        let guild_config = sea_entity::guild_config::Entity::find_by_id(guild as i64)
            .one(&self.sea)
            .await?;

        match guild_config {
            Some(guild_config) => Ok(guild_config),
            None => {
                let guild_config = sea_entity::guild_config::ActiveModel {
                    id: Set(guild as i64),
                    ..Default::default()
                }
                .insert(&self.sea)
                .await?;

                Ok(guild_config)
            }
        }
    }

    pub async fn get_or_create_config<E>(&self, guild: u64) -> Result<E::Model, BotError>
    where
        E: EntityTrait,
        <<E as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: From<i64>,
        E::Model: Send + Sync,
        E::ActiveModel: ActiveModelTrait<Entity = E> + Default + WithGuildId + Send + Sync,
        <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
    {
        if let Some(model) = E::find_by_id(guild as i64).one(&self.sea).await? {
            return Ok(model);
        }

        let mut active = <<E as EntityTrait>::ActiveModel as Default>::default();
        active.set_guild_id(guild);

        let model = active.insert(&self.sea).await?;
        Ok(model)
    }

    pub async fn get_or_create_censor_config(
        &self,
        guild: u64,
    ) -> Result<sea_entity::censor_config::Model, BotError> {
        let censor_config = sea_entity::censor_config::Entity::find_by_id(guild as i64)
            .one(&self.sea)
            .await?;

        match censor_config {
            Some(censor_config) => Ok(censor_config),
            None => {
                let censor_config = sea_entity::censor_config::ActiveModel {
                    id: Set(guild as i64),
                    ..Default::default()
                }
                .insert(&self.sea)
                .await?;

                Ok(censor_config)
            }
        }
    }
}

pub async fn get_config<E>(ctx: Context<'_>) -> Result<E::Model, Error>
where
    E: EntityTrait,
    <<E as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: From<i64>,
    E::Model: Send + Sync,
    E::ActiveModel: ActiveModelTrait<Entity = E> + Default + WithGuildId + Send + Sync,
    <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
{
    let guild = ctx
        .partial_guild()
        .await
        .ok_or(BotError::new("Expected a guild"))?;

    get_config_from_id::<E>(ctx.data().clone(), guild.id).await
}

pub async fn get_config_from_id<E>(store: Arc<Store>, guild_id: GuildId) -> Result<E::Model, Error>
where
    E: EntityTrait,
    <<E as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: From<i64>,
    E::Model: Send + Sync,
    E::ActiveModel: ActiveModelTrait<Entity = E> + Default + WithGuildId + Send + Sync,
    <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
{
    Ok(store.db.get_or_create_config::<E>(guild_id.into()).await?)
}
