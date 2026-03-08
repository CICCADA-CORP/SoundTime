use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A single listen event in the user's playback history.
///
/// Each row records that a user played a specific track at a given time,
/// along with how long they listened and optional behavioral signals
/// (added in Phase 2) that power the recommendation engine.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "listen_history")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    pub track_id: Uuid,
    pub listened_at: DateTimeWithTimeZone,
    pub duration_listened: f32,
    /// Where the track was played from (e.g. "album", "playlist", "radio").
    /// `None` for listens recorded before Phase 2 or by older clients.
    pub source_context: Option<String>,
    /// Whether the track finished naturally (`true`) vs. the user moved on
    /// before it ended (`false`). `None` for legacy rows.
    pub completed: Option<bool>,
    /// Whether the user actively skipped the track. A positive skip is a
    /// negative recommendation signal. `None` for legacy rows.
    pub skipped: Option<bool>,
    /// Playback position (seconds) when the skip occurred. Only meaningful
    /// when `skipped` is `Some(true)`.
    pub skip_position: Option<f32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
    #[sea_orm(
        belongs_to = "super::track::Entity",
        from = "Column::TrackId",
        to = "super::track::Column::Id"
    )]
    Track,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl Related<super::track::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Track.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
