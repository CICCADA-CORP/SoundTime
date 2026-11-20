use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, DbErr,
    EntityTrait, QueryFilter, Set, Statement,
};
use soundtime_audio::{metadata::normalize_genre, AudioMetadata, StorageBackend, StorageError};
use soundtime_db::entities::{album, artist};
use uuid::Uuid;

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

pub(crate) fn tag_value<'a>(form: Option<&'a str>, embedded: Option<&'a str>) -> Option<&'a str> {
    nonblank(form).or_else(|| nonblank(embedded))
}

pub(crate) async fn artist_id(tx: &DatabaseTransaction, name: Option<&str>) -> Result<Uuid, DbErr> {
    let is_placeholder = name.is_none();
    let name = name.unwrap_or(artist::UNATTRIBUTED_NAME);
    if let Some(model) = artist::Entity::find()
        .filter(artist::Column::Name.eq(name))
        .filter(artist::Column::IsPlaceholder.eq(is_placeholder))
        .one(tx)
        .await?
    {
        return Ok(model.id);
    }
    let model = artist::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(name.to_owned()),
        is_placeholder: Set(is_placeholder),
        bio: Set(None),
        image_url: Set(None),
        musicbrainz_id: Set(None),
        created_at: Set(chrono::Utc::now().into()),
    };
    Ok(model.insert(tx).await?.id)
}

pub(crate) struct ImportedRelease {
    pub artist_id: Uuid,
    pub album_id: Option<Uuid>,
    pub track_cover_url: Option<String>,
}

pub(crate) async fn store_track_cover(
    storage: &dyn StorageBackend,
    id: Uuid,
    art: &[u8],
) -> Result<String, StorageError> {
    let path = storage.store_cover(id, Some("track"), art).await?;
    Ok(format!("/api/media/{path}"))
}

/// Catalog rows explicitly confirmed by the caller must survive a later legacy cleanup.
pub(crate) async fn confirm_named_singles(
    tx: &DatabaseTransaction,
    album_id: Uuid,
) -> Result<(), DbErr> {
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM legacy_single_albums WHERE album_id=$1",
        [album_id.into()],
    ))
    .await?;
    Ok(())
}

// The catalog write transaction serializes the lookup and creation across uploads and scans.
pub(crate) async fn resolve(
    tx: &DatabaseTransaction,
    storage: &dyn StorageBackend,
    track_id: Uuid,
    track_artist: Option<&str>,
    title: Option<&str>,
    metadata: &AudioMetadata,
) -> Result<ImportedRelease, DbErr> {
    let track_artist_id = artist_id(tx, track_artist).await?;
    let Some(title) = title else {
        let track_cover_url = if let Some(art) = &metadata.cover_art {
            match store_track_cover(storage, track_id, art).await {
                Ok(url) => Some(url),
                Err(error) => {
                    tracing::warn!(%track_id, %error, "could not store track artwork");
                    None
                }
            }
        } else {
            None
        };
        return Ok(ImportedRelease {
            artist_id: track_artist_id,
            album_id: None,
            track_cover_url,
        });
    };

    let album_artist_id = match nonblank(metadata.album_artist.as_deref()) {
        Some(name) if Some(name) != track_artist => artist_id(tx, Some(name)).await?,
        _ => track_artist_id,
    };
    let existing = album::Entity::find()
        .filter(album::Column::ArtistId.eq(album_artist_id))
        .filter(album::Column::Title.eq(title))
        .one(tx)
        .await?;
    let album = match existing {
        Some(album) => {
            if title == "Singles" {
                confirm_named_singles(tx, album.id).await?;
            }
            album
        }
        None => {
            album::ActiveModel {
                id: Set(Uuid::new_v4()),
                title: Set(title.to_owned()),
                artist_id: Set(album_artist_id),
                release_date: Set(None),
                cover_url: Set(None),
                musicbrainz_id: Set(None),
                genre: Set(metadata
                    .genre
                    .as_deref()
                    .map(normalize_genre)
                    .filter(|v| !v.is_empty())),
                year: Set(metadata.year.map(|v| v as i16)),
                created_at: Set(chrono::Utc::now().into()),
            }
            .insert(tx)
            .await?
        }
    };
    let album_id = album.id;
    // A later track can supply the first artwork for a real album.
    if let (None, Some(art)) = (&album.cover_url, &metadata.cover_art) {
        match storage.store_cover(album_id, Some("album"), art).await {
            Ok(path) => {
                let mut update: album::ActiveModel = album.into();
                update.cover_url = Set(Some(format!("/api/media/{path}")));
                update.update(tx).await?;
            }
            Err(error) => tracing::warn!(%album_id, %error, "could not store album artwork"),
        }
    }
    Ok(ImportedRelease {
        artist_id: track_artist_id,
        album_id: Some(album_id),
        track_cover_url: None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) async fn check_imports(state: &soundtime_db::AppState, mut metadata: AudioMetadata) {
        use sea_orm::{PaginatorTrait, TransactionTrait};

        let tx = state.db.begin().await.unwrap();
        let storage = state.storage.as_ref();
        let uncredited = artist_id(&tx, None).await.unwrap();
        assert_eq!(artist_id(&tx, None).await.unwrap(), uncredited);
        let homonym = artist_id(&tx, Some(artist::UNATTRIBUTED_NAME))
            .await
            .unwrap();
        assert_ne!(uncredited, homonym);
        let visible = artist::Entity::credited()
            .filter(artist::Column::Id.is_in([uncredited, homonym]))
            .all(&tx)
            .await
            .unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, homonym);
        // An album-artist tag or embedded art alone does not declare a release.
        metadata.album_artist = Some("Various Artists".into());
        metadata.cover_art = Some(b"first art".to_vec());
        let first_id = Uuid::new_v4();
        let first = resolve(&tx, storage, first_id, Some("Performer"), None, &metadata)
            .await
            .unwrap();
        metadata.cover_art = Some(b"second art".to_vec());
        let second = resolve(
            &tx,
            storage,
            Uuid::new_v4(),
            Some("Performer"),
            None,
            &metadata,
        )
        .await
        .unwrap();
        assert!(first.album_id.is_none() && second.album_id.is_none());
        assert_eq!(first.artist_id, second.artist_id);
        assert_eq!(
            artist::Entity::find()
                .filter(artist::Column::Name.eq("Various Artists"))
                .count(&tx)
                .await
                .unwrap(),
            0
        );
        let first_cover = first.track_cover_url.unwrap();
        assert_ne!(Some(&first_cover), second.track_cover_url.as_ref());
        assert!(first_cover.contains(&first_id.to_string()));
        assert_eq!(
            storage
                .read_file(first_cover.strip_prefix("/api/media/").unwrap())
                .await
                .unwrap(),
            b"first art"
        );

        let real = resolve(
            &tx,
            storage,
            Uuid::new_v4(),
            Some("Performer"),
            Some("Singles"),
            &metadata,
        )
        .await
        .unwrap();
        let record = album::Entity::find_by_id(real.album_id.unwrap())
            .one(&tx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.title, "Singles");
        assert_ne!(record.artist_id, real.artist_id);
        assert!(real.track_cover_url.is_none() && record.cover_url.is_some());
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO legacy_single_albums (album_id) VALUES ($1)",
            [record.id.into()],
        ))
        .await
        .unwrap();
        let compilation = resolve(
            &tx,
            storage,
            Uuid::new_v4(),
            Some("Guest"),
            Some("Singles"),
            &metadata,
        )
        .await
        .unwrap();
        assert_eq!(real.album_id, compilation.album_id);
        assert!(tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT album_id FROM legacy_single_albums WHERE album_id=$1",
                [record.id.into()]
            ))
            .await
            .unwrap()
            .is_none());
        tx.rollback().await.unwrap();
    }

    #[test]
    fn resolves_only_nonblank_form_or_embedded_tags() {
        assert_eq!(tag_value(None, None), None);
        assert_eq!(tag_value(Some("  "), Some("\t")), None);
        assert_eq!(tag_value(Some(""), Some(" Singles ")), Some("Singles"));
        assert_eq!(tag_value(Some(" Singles "), Some("Other")), Some("Singles"));
        assert_eq!(tag_value(None, Some("One song")), Some("One song"));
    }
}
