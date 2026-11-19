use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "federation_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub catalog_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub cursor: i64,
    pub origin_publication_id: Uuid,
    pub revision: i64,
    pub kind: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub payload: serde_json::Value,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
