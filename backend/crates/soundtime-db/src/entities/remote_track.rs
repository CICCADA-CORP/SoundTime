use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "remote_tracks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub local_track_id: Option<Uuid>,
    pub musicbrainz_id: Option<String>,
    pub title: String,
    pub artist_name: String,
    pub album_title: Option<String>,
    pub instance_domain: String,
    #[sea_orm(unique)]
    pub remote_uri: String,
    pub remote_stream_url: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub format: Option<String>,
    pub is_available: bool,
    pub last_checked_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::track::Entity",
        from = "Column::LocalTrackId",
        to = "super::track::Column::Id"
    )]
    Track,
}

impl Related<super::track::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Track.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
