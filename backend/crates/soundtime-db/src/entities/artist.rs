use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "artists")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub musicbrainz_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub bio: Option<String>,
    pub image_url: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::album::Entity")]
    Album,
    #[sea_orm(has_many = "super::track::Entity")]
    Track,
}

impl Related<super::album::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Album.def()
    }
}

impl Related<super::track::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Track.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
