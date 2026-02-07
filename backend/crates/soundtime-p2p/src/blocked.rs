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

    #[test]
    fn test_module_compiles() {
        // Basic compile check — integration tests require a real DB
        assert!(true);
    }
}
