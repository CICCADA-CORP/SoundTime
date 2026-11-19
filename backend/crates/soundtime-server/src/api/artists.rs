use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use super::tracks::PaginationParams;
use soundtime_db::entities::{album, artist, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct ArtistResponse {
    pub id: Uuid,
    pub name: String,
    pub musicbrainz_id: Option<String>,
    pub bio: Option<String>,
    pub image_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<artist::Model> for ArtistResponse {
    fn from(a: artist::Model) -> Self {
        Self {
            id: a.id,
            name: a.name,
            musicbrainz_id: a.musicbrainz_id,
            bio: a.bio,
            image_url: a.image_url,
            created_at: a.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ArtistDetailResponse {
    #[serde(flatten)]
    pub artist: ArtistResponse,
    pub albums: Vec<super::albums::AlbumResponse>,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

/// GET /api/artists
pub async fn list_artists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<ArtistResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = artist::Entity::credited()
        .order_by_asc(artist::Column::Name)
        .order_by_asc(artist::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let artists = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    Ok(Json(super::tracks::PaginatedResponse {
        data: artists.into_iter().map(ArtistResponse::from).collect(),
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/artists/:id
pub async fn get_artist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistDetailResponse>, (StatusCode, String)> {
    let artist_model = artist::Entity::credited()
        .filter(artist::Column::Id.eq(id))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Artist not found".to_string()))?;

    let albums = album::Entity::find()
        .filter(album::Column::ArtistId.eq(id))
        .order_by_desc(album::Column::Year)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = track::Entity::find()
        .filter(super::tracks::playable_filter(&state))
        .filter(track::Column::ArtistId.eq(id))
        .order_by_desc(track::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(ArtistDetailResponse {
        artist: ArtistResponse::from(artist_model.clone()),
        albums: albums
            .into_iter()
            .map(|a| super::albums::AlbumResponse::from_model(a, Some(artist_model.name.clone())))
            .collect(),
        tracks: tracks
            .into_iter()
            .map(|t| {
                let mut resp = super::tracks::TrackResponse::playable(t);
                resp.artist_name = Some(artist_model.name.clone());
                resp
            })
            .collect(),
    }))
}

/// GET /api/artists/top — top artists by total play_count across their tracks
pub async fn list_top_artists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<ArtistResponse>>, (StatusCode, String)> {
    use sea_orm::{FromQueryResult, JoinType, QuerySelect, RelationTrait};

    let per_page = params.per_page.unwrap_or(10).min(50);

    // Get artist IDs ordered by total play_count
    #[derive(Debug, FromQueryResult)]
    struct ArtistPlayCount {
        artist_id: Uuid,
    }

    let top_ids = track::Entity::find()
        .join(JoinType::InnerJoin, track::Relation::Artist.def())
        .filter(artist::Column::IsPlaceholder.eq(false))
        .filter(super::tracks::playable_filter(&state))
        .select_only()
        .column(track::Column::ArtistId)
        .group_by(track::Column::ArtistId)
        .order_by_desc(sea_orm::sea_query::Expr::col(track::Column::PlayCount).sum())
        .order_by_asc(track::Column::ArtistId)
        .limit(per_page)
        .into_model::<ArtistPlayCount>()
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let ids: Vec<Uuid> = top_ids.into_iter().map(|r| r.artist_id).collect();

    let artists = if !ids.is_empty() {
        artist::Entity::credited()
            .filter(artist::Column::Id.is_in(ids.clone()))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    } else {
        vec![]
    };

    // Preserve the order from the play_count query
    let artist_map: std::collections::HashMap<Uuid, artist::Model> =
        artists.into_iter().map(|a| (a.id, a)).collect();

    let ordered: Vec<ArtistResponse> = ids
        .into_iter()
        .filter_map(|id| artist_map.get(&id).cloned())
        .map(ArtistResponse::from)
        .collect();

    let total = ordered.len() as u64;

    Ok(Json(super::tracks::PaginatedResponse {
        data: ordered,
        total,
        page: 1,
        per_page,
        total_pages: 1,
    }))
}

/// GET /api/artists/random — random artists
pub async fn list_random_artists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<ArtistResponse>>, (StatusCode, String)> {
    use sea_orm::{sea_query::Expr, Order, QuerySelect};

    let per_page = params.per_page.unwrap_or(10).min(50);

    let artists = artist::Entity::credited()
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(per_page)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total = artists.len() as u64;

    Ok(Json(super::tracks::PaginatedResponse {
        data: artists.into_iter().map(ArtistResponse::from).collect(),
        total,
        page: 1,
        per_page,
        total_pages: 1,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_artist_model() -> artist::Model {
        artist::Model {
            id: Uuid::new_v4(),
            name: "Test Artist".into(),
            is_placeholder: false,
            musicbrainz_id: Some("mb-12345".into()),
            bio: Some("A great artist".into()),
            image_url: Some("https://img.example.com/artist.jpg".into()),
            created_at: Utc::now().fixed_offset(),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL for PostgreSQL"]
    async fn uncredited_artist_is_excluded_before_pagination() {
        use sea_orm::{
            ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DbBackend, Schema, Set,
        };

        let mut opts = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
        opts.max_connections(1).min_connections(1);
        let db = Database::connect(opts).await.unwrap();
        let mut table = Schema::new(DbBackend::Postgres).create_table_from_entity(artist::Entity);
        db.execute(DbBackend::Postgres.build(table.temporary()))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState {
            db,
            jwt_secret: String::new(),
            domain: "localhost".into(),
            storage: Arc::new(soundtime_audio::AudioStorage::new(dir.path())),
            p2p: None,
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
        });
        let mut ids = Vec::new();
        for (name, is_placeholder) in [
            ("Alpha", false),
            (artist::UNATTRIBUTED_NAME, true),
            (artist::UNATTRIBUTED_NAME, false),
            ("Zebra", false),
        ] {
            let id = Uuid::new_v4();
            artist::ActiveModel {
                id: Set(id),
                name: Set(name.into()),
                is_placeholder: Set(is_placeholder),
                created_at: Set(Utc::now().into()),
                ..Default::default()
            }
            .insert(&state.db)
            .await
            .unwrap();
            ids.push(id);
        }

        for (page, expected) in [(1, vec![ids[0], ids[2]]), (2, vec![ids[3]]), (3, vec![])] {
            let result = list_artists(
                State(state.clone()),
                Query(PaginationParams {
                    page: Some(page),
                    per_page: Some(2),
                }),
            )
            .await
            .unwrap()
            .0;
            assert_eq!((result.total, result.total_pages), (3, 2));
            assert_eq!(
                result
                    .data
                    .iter()
                    .map(|artist| artist.id)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let random = list_random_artists(
            State(state.clone()),
            Query(PaginationParams {
                page: None,
                per_page: Some(50),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(random.total, 3);
        assert!(random.data.iter().all(|artist| artist.id != ids[1]));
        assert_eq!(
            get_artist(State(state), Path(ids[1])).await.unwrap_err().0,
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn test_artist_response_from_model() {
        let model = make_artist_model();
        let id = model.id;
        let resp = ArtistResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.name, "Test Artist");
        assert_eq!(resp.musicbrainz_id.as_deref(), Some("mb-12345"));
        assert_eq!(resp.bio.as_deref(), Some("A great artist"));
    }

    #[test]
    fn test_artist_response_serialization() {
        let model = make_artist_model();
        let resp = ArtistResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "Test Artist");
        assert!(json["image_url"].is_string());
    }

    #[test]
    fn test_artist_response_no_optional_fields() {
        let model = artist::Model {
            id: Uuid::new_v4(),
            name: "Minimal Artist".into(),
            is_placeholder: false,
            musicbrainz_id: None,
            bio: None,
            image_url: None,
            created_at: Utc::now().fixed_offset(),
        };
        let resp = ArtistResponse::from(model);
        assert!(resp.musicbrainz_id.is_none());
        assert!(resp.bio.is_none());
        assert!(resp.image_url.is_none());
    }
}
