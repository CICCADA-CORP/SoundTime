use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "p2p_peers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub node_id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub track_count: i64,
    pub is_online: bool,
    pub last_seen_at: DateTimeWithTimeZone,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
