use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "CREATE TABLE federation_cache_pins (
                scope text NOT NULL CHECK (scope IN ('tracks','albums')),
                scope_id uuid NOT NULL, track_id uuid NOT NULL, revision bigint NOT NULL, content_hash text NOT NULL,
                PRIMARY KEY(scope,scope_id,track_id),
                FOREIGN KEY(track_id, revision) REFERENCES publication_versions(track_id,revision));
             CREATE INDEX federation_cache_pin_assets ON federation_cache_pins(content_hash);
             CREATE FUNCTION federation_drop_inactive_pins() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
               DELETE FROM federation_cache_pins WHERE track_id=NEW.track_id;
               RETURN NULL;
             END; $$;
             CREATE TRIGGER inactive_publication_pins AFTER UPDATE ON track_publications FOR EACH ROW
               WHEN (NEW.state <> 'active' OR NEW.revision <> OLD.revision) EXECUTE FUNCTION federation_drop_inactive_pins();
             CREATE TRIGGER hidden_publication_pins AFTER INSERT ON federation_hidden_publications FOR EACH ROW
               EXECUTE FUNCTION federation_drop_inactive_pins();"
        ).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration("cache pin references must survive rollback".into()))
    }
}
