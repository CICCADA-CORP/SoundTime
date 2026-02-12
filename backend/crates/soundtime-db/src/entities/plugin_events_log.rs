use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plugin_events_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub plugin_id: Uuid,
    pub event_name: String,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub payload: Option<serde_json::Value>,
    pub result: String,
    pub execution_time_ms: i32,
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::plugin::Entity",
        from = "Column::PluginId",
        to = "super::plugin::Column::Id"
    )]
    Plugin,
}

impl Related<super::plugin::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Plugin.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
