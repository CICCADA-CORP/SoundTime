pub use sea_orm_migration::prelude::*;

mod m20240101_000001_create_users;
mod m20240101_000002_create_actors;
mod m20240101_000003_create_artists;
mod m20240101_000004_create_albums;
mod m20240101_000005_create_tracks;
mod m20240101_000006_create_libraries;
mod m20240101_000007_create_library_tracks;
mod m20240101_000008_create_playlists;
mod m20240101_000009_create_playlist_tracks;
mod m20240101_000010_create_follows;
mod m20240101_000011_create_activities;
mod m20240101_000012_create_deliveries;
mod m20240101_000013_create_listen_history;
mod m20240101_000014_create_favorites;
mod m20240101_000015_create_instance_settings;
mod m20240101_000016_create_remote_tracks;
mod m20240101_000017_add_uploaded_by_to_tracks;
mod m20240101_000018_add_setup_complete_setting;
mod m20240101_000019_add_ban_fields_to_users;
mod m20240101_000020_add_editorial_to_playlists;
mod m20240101_000021_create_track_reports;
mod m20240101_000022_remove_ap_add_p2p;
mod m20240101_000023_create_p2p_peers;
mod m20240101_000024_create_plugins;
mod m20240101_000025_create_themes;
mod m20240101_000026_fix_album_dedup;
mod m20240101_000027_create_user_settings;
mod m20240101_000028_normalize_genres;
mod m20240101_000029_add_radio_indexes;
mod m20240101_000030_add_behavioral_signals;
mod m20240101_000031_add_track_embeddings;
mod m20240101_000032_add_performance_indexes;
mod m20240101_000033_refresh_collation_version;
mod m20240101_000034_track_file_hash;
mod m20240101_000035_federated_publications;
mod m20240101_000036_publication_withdrawals;
mod m20240101_000037_catalog_search;
mod m20240101_000038_received_inventories;
mod m20240101_000039_cache_pins;
mod m20240101_000040_mirror_leases;
mod m20240101_000041_peer_observations;

mod m20240101_000042_federation_workers;
mod m20240101_000043_pending_backfill;
mod m20240101_000044_track_cover;
mod m20240101_000045_artist_placeholder;
mod m20261119_000001_federation_source_updates;
mod m20261119_000002_original_proofs;
mod m20261119_000003_search_rank_generation;
mod m20261119_000004_catalog_credits;
mod m20261119_000006_recording_tags;
mod m20261119_000007_direct_search_page;

pub struct Migrator;

impl Migrator {
    /// Serialize startup upgrades across processes sharing the same database.
    pub async fn up_locked(
        db: &sea_orm_migration::sea_orm::DatabaseConnection,
    ) -> Result<(), DbErr> {
        use sea_orm_migration::sea_orm::TransactionTrait;

        let tx = db.begin().await?;
        tx.execute_unprepared("SELECT pg_advisory_xact_lock(1937011565, 2)")
            .await?;
        Self::up(&tx, None).await?;
        tx.commit().await
    }
}

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_create_users::Migration),
            Box::new(m20240101_000002_create_actors::Migration),
            Box::new(m20240101_000003_create_artists::Migration),
            Box::new(m20240101_000004_create_albums::Migration),
            Box::new(m20240101_000005_create_tracks::Migration),
            Box::new(m20240101_000006_create_libraries::Migration),
            Box::new(m20240101_000007_create_library_tracks::Migration),
            Box::new(m20240101_000008_create_playlists::Migration),
            Box::new(m20240101_000009_create_playlist_tracks::Migration),
            Box::new(m20240101_000010_create_follows::Migration),
            Box::new(m20240101_000011_create_activities::Migration),
            Box::new(m20240101_000012_create_deliveries::Migration),
            Box::new(m20240101_000013_create_listen_history::Migration),
            Box::new(m20240101_000014_create_favorites::Migration),
            Box::new(m20240101_000015_create_instance_settings::Migration),
            Box::new(m20240101_000016_create_remote_tracks::Migration),
            Box::new(m20240101_000017_add_uploaded_by_to_tracks::Migration),
            Box::new(m20240101_000018_add_setup_complete_setting::Migration),
            Box::new(m20240101_000019_add_ban_fields_to_users::Migration),
            Box::new(m20240101_000020_add_editorial_to_playlists::Migration),
            Box::new(m20240101_000021_create_track_reports::Migration),
            Box::new(m20240101_000022_remove_ap_add_p2p::Migration),
            Box::new(m20240101_000023_create_p2p_peers::Migration),
            Box::new(m20240101_000024_create_plugins::Migration),
            Box::new(m20240101_000025_create_themes::Migration),
            Box::new(m20240101_000026_fix_album_dedup::Migration),
            Box::new(m20240101_000027_create_user_settings::Migration),
            Box::new(m20240101_000028_normalize_genres::Migration),
            Box::new(m20240101_000029_add_radio_indexes::Migration),
            Box::new(m20240101_000030_add_behavioral_signals::Migration),
            Box::new(m20240101_000031_add_track_embeddings::Migration),
            Box::new(m20240101_000032_add_performance_indexes::Migration),
            Box::new(m20240101_000033_refresh_collation_version::Migration),
            Box::new(m20240101_000034_track_file_hash::Migration),
            Box::new(m20240101_000035_federated_publications::Migration),
            Box::new(m20240101_000036_publication_withdrawals::Migration),
            Box::new(m20240101_000037_catalog_search::Migration),
            Box::new(m20240101_000038_received_inventories::Migration),
            Box::new(m20240101_000039_cache_pins::Migration),
            Box::new(m20240101_000040_mirror_leases::Migration),
            Box::new(m20240101_000041_peer_observations::Migration),
            Box::new(m20240101_000042_federation_workers::Migration),
            Box::new(m20240101_000043_pending_backfill::Migration),
            Box::new(m20240101_000044_track_cover::Migration),
            Box::new(m20240101_000045_artist_placeholder::Migration),
            Box::new(m20261119_000001_federation_source_updates::Migration),
            Box::new(m20261119_000002_original_proofs::Migration),
            Box::new(m20261119_000003_search_rank_generation::Migration),
            Box::new(m20261119_000004_catalog_credits::Migration),
            Box::new(m20261119_000006_recording_tags::Migration),
            Box::new(m20261119_000007_direct_search_page::Migration),
        ]
    }
}
