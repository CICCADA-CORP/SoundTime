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

pub struct Migrator;

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
        ]
    }
}
