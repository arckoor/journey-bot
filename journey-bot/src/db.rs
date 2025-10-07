use std::{collections::HashSet, sync::Arc, time::Duration};

use poise::serenity_prelude::GuildId;
use sea_migration::{Migrator, MigratorTrait};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, DatabaseConnection,
    EntityTrait, IntoActiveModel, PrimaryKeyTrait, QueryFilter,
};

use crate::{
    Context, Error,
    store::Store,
    utils::{BotError, fetch_sheet, fetch_sheet_columns},
};

pub trait SetGuildId {
    fn set_guild_id(&mut self, guild: u64);
}

impl SetGuildId for sea_entity::guild_config::ActiveModel {
    fn set_guild_id(&mut self, guild: u64) {
        self.id = Set(guild as i64);
    }
}

impl SetGuildId for sea_entity::censor_config::ActiveModel {
    fn set_guild_id(&mut self, guild: u64) {
        self.id = Set(guild as i64);
    }
}

pub trait GetCensorList {
    fn get_censor_list(&self) -> Vec<String>;
}

impl GetCensorList for sea_entity::censor_config::Model {
    fn get_censor_list(&self) -> Vec<String> {
        self.auto_censor_list.clone()
    }
}

impl GetCensorList for sea_entity::stream_observer::Model {
    fn get_censor_list(&self) -> Vec<String> {
        self.auto_blacklist.clone()
    }
}

pub trait SetCensorList {
    fn set_censor_list(&mut self, list: Vec<String>);
}

impl SetCensorList for sea_entity::censor_config::ActiveModel {
    fn set_censor_list(&mut self, list: Vec<String>) {
        self.auto_censor_list = Set(list);
    }
}

impl SetCensorList for sea_entity::stream_observer::ActiveModel {
    fn set_censor_list(&mut self, list: Vec<String>) {
        self.auto_blacklist = Set(list);
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
        E::ActiveModel: ActiveModelTrait<Entity = E> + Default + SetGuildId + Send,
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

    pub async fn stage_new_items<E>(
        &self,
        config: E::Model,
        config_id: &str,
        column_names: Vec<String>,
        sheet_id: &str,
    ) -> Result<(Vec<String>, Vec<(String, bool)>), BotError>
    where
        E: EntityTrait,
        E::Model: GetCensorList,
        E::ActiveModel: ActiveModelTrait<Entity = E> + SetCensorList + Send,
        <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
    {
        let sheet = fetch_sheet(sheet_id).await?;

        let mut columns = {
            let cols = column_names.iter().collect::<Vec<_>>();
            fetch_sheet_columns(sheet, &cols).await?
        };

        let mut censor_list = Vec::new();

        for column in &column_names {
            censor_list.append(&mut columns.remove(column).unwrap());
        }

        let old = {
            let staged = sea_entity::staged_censor_item::Entity::find()
                .filter(sea_entity::staged_censor_item::Column::ForeignId.eq(config_id))
                .all(&self.sea)
                .await?
                .into_iter()
                .map(|s| s.item);
            let mut current = config.get_censor_list();
            current.extend(staged);
            current.into_iter().collect::<HashSet<_>>()
        };

        let new = censor_list
            .into_iter()
            .map(|c| c.to_lowercase())
            .collect::<HashSet<_>>();

        let added = new.difference(&old).cloned().collect::<Vec<_>>();
        let removed = old.difference(&new).cloned().collect::<Vec<_>>();

        let mut censor_list = config.get_censor_list();
        censor_list.retain(|s| !removed.contains(s));

        let mut config = config.into_active_model();
        config.set_censor_list(censor_list);
        let _ = config.update(&self.sea).await;

        for added in added.iter() {
            let _ = sea_entity::staged_censor_item::ActiveModel {
                item: Set(added.clone()),
                foreign_id: Set(config_id.to_string()),
                ..Default::default()
            }
            .insert(&self.sea)
            .await;
        }

        let mut removed_ = Vec::new();

        for rem in removed.into_iter() {
            if let Ok(r) = sea_entity::staged_censor_item::Entity::delete_many()
                .filter(
                    sea_orm::Condition::all()
                        .add(sea_entity::staged_censor_item::Column::ForeignId.eq(config_id))
                        .add(sea_entity::staged_censor_item::Column::Item.eq(&rem)),
                )
                .exec(&self.sea)
                .await
                && r.rows_affected > 0
            {
                removed_.push((rem, true));
            } else {
                removed_.push((rem, false));
            }
        }

        Ok((added, removed_))
    }

    pub async fn commit_staged_items<E>(
        &self,
        id: &str,
        diff: f64,
        config: E::Model,
    ) -> Result<Vec<String>, BotError>
    where
        E: EntityTrait,
        E::Model: GetCensorList,
        E::ActiveModel: ActiveModelTrait<Entity = E> + SetCensorList + Send,
        <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
    {
        let staged = sea_entity::staged_censor_item::Entity::delete_many()
            .filter(
                sea_orm::Condition::all()
                    .add(sea_entity::staged_censor_item::Column::ForeignId.eq(id))
                    .add(sea_entity::staged_censor_item::Column::CreatedAt.lt(diff)),
            )
            .exec_with_returning(&self.sea)
            .await?;

        if staged.is_empty() {
            return Err(BotError::new("No staged items available"));
        }

        let staged = staged.into_iter().map(|m| m.item).collect::<Vec<_>>();

        let mut censor_list = config.get_censor_list();
        censor_list.extend(staged.iter().cloned());
        let censor_list = censor_list
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let mut config = config.into_active_model();
        config.set_censor_list(censor_list);
        config.update(&self.sea).await?;

        Ok(staged)
    }
}

pub async fn get_config<E>(ctx: Context<'_>) -> Result<E::Model, Error>
where
    E: EntityTrait,
    <<E as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType: From<i64>,
    E::ActiveModel: ActiveModelTrait<Entity = E> + Default + SetGuildId + Send,
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
    E::ActiveModel: ActiveModelTrait<Entity = E> + Default + SetGuildId + Send,
    <E as EntityTrait>::Model: IntoActiveModel<<E as EntityTrait>::ActiveModel>,
{
    Ok(store.db.get_or_create_config::<E>(guild_id.into()).await?)
}
