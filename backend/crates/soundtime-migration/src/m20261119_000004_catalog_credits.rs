use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// Restore the exact definitions shipped with the older migration. In particular
// a downgrade must preserve the event journal and signed remote revisions.
fn previous_definitions() -> Result<String, DbErr> {
    let search = include_str!("federation/search.sql");
    let mut sql = "DROP TRIGGER federation_search_credit ON artists;\n".to_owned();
    for (source, name, end) in [
        (
            include_str!("federation/publications.sql"),
            "FUNCTION federation_append_event(",
            "\n$$;",
        ),
        (
            include_str!("federation/inventories.sql"),
            "FUNCTION federation_import_signed(",
            "\n$$;",
        ),
        (search, "FUNCTION federation_search_project(", "\n$$;"),
        (search, "VIEW federation_search_input AS", ";"),
    ] {
        let (body, _) = source
            .split_once(&format!("CREATE {name}"))
            .and_then(|(_, tail)| tail.split_once(end))
            .ok_or_else(|| DbErr::Migration(format!("missing historical {name}")))?;
        sql.push_str(&format!("CREATE OR REPLACE {name}{body}{end}\n"));
    }
    sql.push_str(include_str!("federation/search_backfill.sql"));
    // A schema downgrade changes the search vocabulary back, also for rows
    // committed after the upgrade. The existing worker handles these in batches.
    sql.push_str(
        "\nINSERT INTO federation_search_jobs(kind,subject_id)
        SELECT 'artist',id FROM artists WHERE is_placeholder ON CONFLICT(kind,subject_id)
        DO UPDATE SET last_track_id=NULL,generation=federation_search_jobs.generation+1;",
    );
    Ok(sql)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("federation/catalog_credits.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&previous_definitions()?)
            .await?;
        Ok(())
    }
}
