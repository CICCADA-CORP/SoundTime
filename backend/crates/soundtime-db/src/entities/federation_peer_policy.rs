use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "federation_peer_policies")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub node_id: String,
    pub is_followed: bool,
    pub is_banned: bool,
    pub share_identity: bool,
    pub applied_cursor: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
