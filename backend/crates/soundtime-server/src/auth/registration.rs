//! Shared registration policy for account creation, nodeinfo and directory announcements.
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use soundtime_db::entities::instance_setting;

pub async fn registrations_open(db: &DatabaseConnection, user_count: u64) -> Result<bool, DbErr> {
    let settings = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.is_in([
            "setup_complete",
            "instance_private",
            "open_registrations",
        ]))
        .all(db)
        .await?;
    Ok(from_settings(&settings, user_count))
}

pub fn from_settings(settings: &[instance_setting::Model], user_count: u64) -> bool {
    let value = |key: &str| {
        settings
            .iter()
            .find(|row| row.key == key)
            .map(|row| row.value.as_str())
    };
    // Preserve the existing first-account bootstrap and defaults of upgraded instances.
    (user_count == 0 || value("setup_complete") == Some("true"))
        && value("instance_private") != Some("true")
        && value("open_registrations").is_none_or(|value| value == "true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> Vec<instance_setting::Model> {
        pairs
            .iter()
            .map(|&(key, value)| instance_setting::Model {
                id: uuid::Uuid::nil(),
                key: key.into(),
                value: value.into(),
                updated_at: chrono::Utc::now().fixed_offset(),
            })
            .collect()
    }

    #[test]
    fn effective_policy_includes_setup_visibility_and_opt_in_with_legacy_defaults() {
        for (private, open, expected) in [
            ("false", "true", true),
            ("false", "false", false),
            ("true", "true", false),
            ("true", "false", false),
        ] {
            assert_eq!(
                from_settings(
                    &settings(&[
                        ("setup_complete", "true"),
                        ("instance_private", private),
                        ("open_registrations", open)
                    ]),
                    3
                ),
                expected
            );
        }
        assert!(from_settings(&settings(&[("setup_complete", "true")]), 3));
        assert!(from_settings(&[], 0));
        assert!(!from_settings(&settings(&[("setup_complete", "false")]), 1));
    }
}
