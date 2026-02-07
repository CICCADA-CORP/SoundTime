use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tracks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub title: String,
    pub artist_id: Uuid,
    pub album_id: Option<Uuid>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub duration_secs: f32,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub musicbrainz_id: Option<String>,
    pub file_path: String,
    pub file_size: i64,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub waveform_data: Option<serde_json::Value>,
    pub uploaded_by: Option<Uuid>,
    /// BLAKE3 content hash from iroh-blobs (set when P2P is enabled)
    pub content_hash: Option<String>,
    #[sea_orm(default_value = "0")]
    pub play_count: i64,
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
    #[sea_orm(
        belongs_to = "super::album::Entity",
        from = "Column::AlbumId",
        to = "super::album::Column::Id"
    )]
    Album,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UploadedBy",
        to = "super::user::Column::Id"
    )]
    Uploader,
    #[sea_orm(has_many = "super::listen_history::Entity")]
    ListenHistory,
    #[sea_orm(has_many = "super::favorite::Entity")]
    Favorite,
}

impl Related<super::artist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Artist.def()
    }
}

impl Related<super::album::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Album.def()
    }
}

impl Related<super::listen_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ListenHistory.def()
    }
}

impl Related<super::favorite::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Favorite.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Uploader.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
