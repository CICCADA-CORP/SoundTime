use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "publication_offers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub track_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub provider_node_id: String,
    pub expires_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
