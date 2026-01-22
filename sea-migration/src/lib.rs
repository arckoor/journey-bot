pub use sea_orm_migration::prelude::*;

mod m20250826_012513_init;
mod m20251005_135525_cl_staging;
mod m20260121_221856_auto_roles;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250826_012513_init::Migration),
            Box::new(m20251005_135525_cl_staging::Migration),
            Box::new(m20260121_221856_auto_roles::Migration),
        ]
    }
}
