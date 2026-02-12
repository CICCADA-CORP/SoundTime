use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plugins")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub version: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub git_url: String,
    pub wasm_path: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub permissions: serde_json::Value,
    pub status: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
    pub installed_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub installed_by: Option<Uuid>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::plugin_config::Entity")]
    PluginConfigs,
    #[sea_orm(has_many = "super::plugin_events_log::Entity")]
    PluginEventsLog,
}

impl Related<super::plugin_config::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PluginConfigs.def()
    }
}

impl Related<super::plugin_events_log::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PluginEventsLog.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
