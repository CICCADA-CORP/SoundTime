use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "albums")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub title: String,
    pub artist_id: Uuid,
    pub release_date: Option<Date>,
    pub cover_url: Option<String>,
    pub musicbrainz_id: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::artist::Entity",
        from = "Column::ArtistId",
        to = "super::artist::Column::Id"
    )]
    Artist,
    #[sea_orm(has_many = "super::track::Entity")]
    Track,
}

impl Related<super::artist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Artist.def()
    }
}

impl Related<super::track::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Track.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
