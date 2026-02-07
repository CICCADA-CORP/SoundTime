use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "user_role")]
pub enum UserRole {
    #[sea_orm(string_value = "admin")]
    Admin,
    #[sea_orm(string_value = "user")]
    User,
}

impl UserRole {
    pub fn as_str(&self) -> &str {
        match self {
            UserRole::Admin => "admin",
            UserRole::User => "user",
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub username: String,
    #[sea_orm(unique)]
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: UserRole,
    pub is_banned: bool,
    pub ban_reason: Option<String>,
    pub banned_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::library::Entity")]
    Library,
    #[sea_orm(has_many = "super::playlist::Entity")]
    Playlist,
    #[sea_orm(has_many = "super::listen_history::Entity")]
    ListenHistory,
    #[sea_orm(has_many = "super::favorite::Entity")]
    Favorite,
}

impl Related<super::library::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Library.def()
    }
}

impl Related<super::playlist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Playlist.def()
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

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_role_as_str_admin() {
        assert_eq!(UserRole::Admin.as_str(), "admin");
    }

    #[test]
    fn test_user_role_as_str_user() {
        assert_eq!(UserRole::User.as_str(), "user");
    }

    #[test]
    fn test_user_role_display() {
        assert_eq!(format!("{}", UserRole::Admin), "admin");
        assert_eq!(format!("{}", UserRole::User), "user");
    }

    #[test]
    fn test_user_role_serialization() {
        let json = serde_json::to_string(&UserRole::Admin).unwrap();
        assert_eq!(json, r#""Admin""#);

        let json = serde_json::to_string(&UserRole::User).unwrap();
        assert_eq!(json, r#""User""#);
    }

    #[test]
    fn test_user_role_deserialization() {
        let role: UserRole = serde_json::from_str(r#""Admin""#).unwrap();
        assert_eq!(role, UserRole::Admin);

        let role: UserRole = serde_json::from_str(r#""User""#).unwrap();
        assert_eq!(role, UserRole::User);
    }
}
