use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "track_publications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub track_id: Uuid,
    pub catalog_id: Option<Uuid>,
    pub origin_publication_id: Option<Uuid>,
    pub revision: i64,
    pub state: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
