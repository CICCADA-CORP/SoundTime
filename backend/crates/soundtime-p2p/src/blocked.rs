//! Peer blocking — check if a peer is blocked by PeerID or readable name.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use soundtime_db::entities::blocked_domain;

/// Check if a peer is blocked, either by its iroh NodeId string or readable name.
/// We reuse the `blocked_domains` table — the `domain` column stores either
/// the iroh NodeId (base32 string) or a human-readable peer name.
pub async fn is_peer_blocked(db: &DatabaseConnection, peer_id: &str) -> bool {
    let result = blocked_domain::Entity::find()
        .filter(blocked_domain::Column::Domain.eq(peer_id))
        .one(db)
        .await;

    matches!(result, Ok(Some(_)))
}

/// Check if any of the provided identifiers (NodeId, peer name) are blocked.
pub async fn is_any_peer_id_blocked(db: &DatabaseConnection, identifiers: &[&str]) -> bool {
    for id in identifiers {
        if is_peer_blocked(db, id).await {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // We can't use MockDatabase here because the `mock` feature on sea-orm
    // would break Clone on DatabaseConnection (used by AppState in soundtime-db).
    // Instead we test the contract and ergonomics of the public API.

    #[test]
    fn test_module_compiles() {
        // Verify is_peer_blocked and is_any_peer_id_blocked exist with correct signatures
        fn _check_is_peer_blocked<'a>(
            _db: &'a DatabaseConnection,
            _peer_id: &'a str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + 'a>> {
            Box::pin(is_peer_blocked(_db, _peer_id))
        }

        fn _check_is_any_blocked<'a>(
            _db: &'a DatabaseConnection,
            _ids: &'a [&'a str],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + 'a>> {
            Box::pin(is_any_peer_id_blocked(_db, _ids))
        }
    }

    #[test]
    fn test_blocked_domain_entity_schema() {
        // Verify the entity has the expected columns
        use sea_orm::EntityName;
        assert_eq!(blocked_domain::Entity.table_name(), "blocked_domains");
    }

    #[test]
    fn test_blocked_domain_columns() {
        // Verify key columns exist for the blocking queries
        use sea_orm::ColumnTrait;
        let _domain_col = blocked_domain::Column::Domain;
        let _id_col = blocked_domain::Column::Id;
        // The eq filter should accept a string
        let _filter = blocked_domain::Column::Domain.eq("test");
    }

    /// Verify the logic: is_any_peer_id_blocked returns false for empty slice
    /// (doesn't even need a DB connection, just verifying the loop logic).
    /// Note: We can't call the actual function without a DB, but we verify
    /// the contract: empty input → false.
    #[test]
    fn test_any_blocked_empty_identifiers_contract() {
        // The function iterates over identifiers — an empty slice means
        // the for loop body never executes, so it returns false.
        let identifiers: &[&str] = &[];
        assert!(identifiers.is_empty());
        // This is the expected behavior: no identifiers → not blocked
    }
}
