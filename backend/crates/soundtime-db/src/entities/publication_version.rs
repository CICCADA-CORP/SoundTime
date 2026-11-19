use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "publication_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub track_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,
    pub content_hash: Option<String>,
    pub file_size: Option<i64>,
    pub format: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
