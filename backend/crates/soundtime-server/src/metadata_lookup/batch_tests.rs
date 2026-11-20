use super::{new_metadata_tracker, MetadataResult, MetadataTaskTrackerHandle};
use crate::api::admin::{
    enrich_all_metadata, enrich_track_metadata, metadata_task_dismiss, metadata_task_status,
    RecordingChoice,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension,
};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DbBackend, EntityTrait, Schema, Statement,
};
use serde_json::{json, Value};
use soundtime_db::{
    entities::{album, artist, instance_setting, playlist, playlist_track, track},
    AppState,
};
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, timeout};
use uuid::Uuid;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

const TEST_ARTIST: &str = "7849fc8a-9b73-42e1-b45e-7230a5100c60";
const FOUND: &str = "d634597d-84b0-4cea-8cf8-f61ae39c4bc1";
const ALTERNATE: &str = "9a42bdc6-6648-40c3-9351-bf6721712370";
const CHOICE: &str = "e89194f9-0149-40cf-a730-b5590e21f4f1";
const RELEASE: &str = "1f8afb19-70ea-46d9-92e6-f7bbf075e44b";

struct Fixture {
    state: Arc<AppState>,
    provider: MockServer,
    _music: tempfile::TempDir,
    credited: Uuid,
    placeholder: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let mut options = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
        // The batch and the HTTP handlers share exactly the same temporary tables.
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.unwrap();
        db.execute_unprepared("CREATE TEMP TABLE users(id uuid PRIMARY KEY)")
            .await
            .unwrap();
        let schema = Schema::new(DbBackend::Postgres);
        for mut table in [
            schema.create_table_from_entity(artist::Entity),
            schema.create_table_from_entity(album::Entity),
            schema.create_table_from_entity(track::Entity),
            schema.create_table_from_entity(instance_setting::Entity),
            schema.create_table_from_entity(playlist::Entity),
            schema.create_table_from_entity(playlist_track::Entity),
        ] {
            db.execute(DbBackend::Postgres.build(table.temporary()))
                .await
                .unwrap();
        }
        db.execute_unprepared(
            "CREATE TEMP TABLE track_recording_hints(
            track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
            recording_id uuid, isrc text CHECK(isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
            CHECK(recording_id IS NOT NULL OR isrc IS NOT NULL))",
        )
        .await
        .unwrap();
        // The runner clones the fully migrated PostgreSQL database. Copy its
        // projection shapes into this connection's isolated test namespace.
        db.execute_unprepared(
            "CREATE TEMP TABLE federation_catalogs (LIKE public.federation_catalogs INCLUDING ALL);
             CREATE TEMP TABLE track_publications (LIKE public.track_publications INCLUDING ALL);",
        )
        .await
        .unwrap();
        let provider = MockServer::start().await;
        db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO instance_settings(id,key,value,updated_at) VALUES
              (gen_random_uuid(),'musicbrainz_base_url',$1,now()), (gen_random_uuid(),'coverart_base_url',$1,now())",
            [provider.uri().into()])).await.unwrap();
        let credited = Uuid::new_v4();
        let placeholder = Uuid::new_v4();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO artists(id,name,created_at,is_placeholder) VALUES
                ($1,'Test Artist',now(),false), ($2,'Unknown Artist',now(),true)",
            [credited.into(), placeholder.into()],
        ))
        .await
        .unwrap();
        let music = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState {
            db,
            jwt_secret: String::new(),
            domain: "localhost".into(),
            storage: Arc::new(soundtime_audio::AudioStorage::new(music.path())),
            p2p: None,
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
        });
        Self {
            state,
            provider,
            _music: music,
            credited,
            placeholder,
        }
    }

    async fn track(
        &self,
        title: &str,
        artist: Uuid,
        album: Option<Uuid>,
        recording: Option<&str>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        self.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO tracks(id,title,artist_id,album_id,duration_secs,file_path,file_size,format,
                     created_at,musicbrainz_id,genre,year,file_hash,play_count)
             VALUES($1,$2,$3,$4,30,$2,1024,'wav',now(),$5,'Imported genre',2001,'original-bytes',7)",
            [id.into(),title.into(),artist.into(),album.into(),recording.into()])).await.unwrap();
        id
    }

    async fn stored(&self, id: Uuid) -> track::Model {
        track::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn enrich(&self, id: Uuid, choice: Option<&str>) -> MetadataResult {
        enrich_track_metadata(
            State(self.state.clone()),
            Path(id),
            Query(RecordingChoice {
                recording_id: choice.map(|s| Uuid::parse_str(s).unwrap()),
            }),
        )
        .await
        .unwrap()
        .0
    }

    async fn detail(&self, value: Value) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/recording/{}",
                value["id"].as_str().unwrap()
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(value))
            .mount(&self.provider)
            .await;
    }
}

fn recording(id: &str, title: &str, credit: &str, artist_id: &str) -> Value {
    json!({"id":id,"title":title,"length":30000,"score":100,
        "artist-credit":[{"name":credit,"artist":{"name":credit,"id":artist_id}}],
        "releases":[{"id":RELEASE,"title":"Gallery Sessions"}],"tags":[{"name":"provider genre","count":12}],
        "first-release-date":"2025-01-19"})
}

fn query(req: &Request) -> String {
    req.url
        .query_pairs()
        .find(|(key, _)| key == "query")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

fn page(rows: Vec<Value>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"count":rows.len(),"recordings":rows}))
}

async fn completed(tracker: &MetadataTaskTrackerHandle) -> Value {
    timeout(Duration::from_secs(45), async {
        loop {
            let state = metadata_task_status(Extension(tracker.clone())).await.0;
            if state["status"] != "running" {
                assert_eq!(state["status"], "completed", "{state}");
                return state["result"].clone();
            }
            let progress = &state["progress"];
            assert!(
                progress["processed"].as_u64().unwrap() <= progress["total"].as_u64().unwrap(),
                "{state}"
            );
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("batch did not reach a terminal state")
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL for a PostgreSQL connection"]
async fn metadata_batch_finishes_with_unmatched_tracks_and_a_provider_error() {
    let f = Fixture::new().await;
    let album = Uuid::new_v4();
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO albums(id,artist_id,title,cover_url,created_at) VALUES($1,$2,'Gallery Sessions','/existing.jpg',now())",
        [album.into(),f.credited.into()])).await.unwrap();
    let unmatched = f.track("Missing One", f.credited, Some(album), None).await;
    let failed = f.track("Offline Beacon", f.credited, None, None).await;
    let identified = f
        .track("Drift Signal", f.placeholder, Some(album), None)
        .await;
    let ambiguous = f.track("Northern Lights", f.placeholder, None, None).await;
    let existing = f
        .track("Already enriched", f.credited, None, Some(FOUND))
        .await;
    // Plain file paths ensure this exercises catalogue ownership, even for a
    // mirror that has retained its own copy of an original.
    let remote = f.track("Foreign original", f.credited, None, None).await;
    let withdrawn = f.track("Withdrawn original", f.credited, None, None).await;
    let catalog = Uuid::new_v4();
    let foreign_catalog = Uuid::new_v4();
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_catalogs(id,is_local,node_id) VALUES($1,true,NULL),($2,false,'remote-fixture')",
        [catalog.into(), foreign_catalog.into()])).await.unwrap();
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO track_publications(track_id,catalog_id,origin_publication_id,revision,state)
         VALUES($1,$2,$1,1,'active'),($3,$4,$3,2,'withdrawn')",
        [remote.into(),foreign_catalog.into(),withdrawn.into(),catalog.into()])).await.unwrap();
    let before_remote = f.stored(remote).await;
    let before_withdrawn = f.stored(withdrawn).await;
    let before = f.stored(ambiguous).await;
    let before_unmatched = f.stored(unmatched).await;
    let before_failure = f.stored(failed).await;
    let placeholder = artist::Entity::find_by_id(f.placeholder)
        .one(&f.state.db)
        .await
        .unwrap()
        .unwrap();
    let listener = Uuid::new_v4();
    let collection = Uuid::new_v4();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO users VALUES($1)",
            [listener.into()],
        ))
        .await
        .unwrap();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO playlists(id,user_id,name,is_public,is_editorial,created_at,updated_at)
         VALUES ($1,$2,'Listener playlist',false,false,now(),now())",
            [collection.into(), listener.into()],
        ))
        .await
        .unwrap();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO playlist_tracks(playlist_id,track_id,position) VALUES($1,$2,4),($1,$3,5)",
            [collection.into(), existing.into(), ambiguous.into()],
        ))
        .await
        .unwrap();
    let entries = || async {
        let pinned = playlist_track::Entity::find_by_id((collection, ambiguous))
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        let prior = playlist_track::Entity::find_by_id((collection, existing))
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        (prior, pinned)
    };
    let references = entries().await;
    let known = recording(
        FOUND,
        "Drift Signal",
        "New Credit",
        "ed8a9eee-bb9a-41bf-85b9-ff719d8dd48e",
    );
    let alternate = recording(
        ALTERNATE,
        "Northern Lights",
        "Another Performer",
        "2777f4ab-a337-46b9-9080-55eba1bbc251",
    );
    let chosen = recording(
        CHOICE,
        "Northern Lights",
        "Selected Performer",
        "2c3e270d-7b09-4b82-b271-6529e9342192",
    );
    f.detail(known.clone()).await;
    f.detail(chosen.clone()).await;
    // Artist relations return the fixture's default 404, so no Wikipedia fallback is contacted.
    Mock::given(method("GET"))
        .and(path("/recording"))
        .respond_with(move |r: &Request| {
            let q = query(r);
            if q.contains("Offline Beacon") {
                return ResponseTemplate::new(503).insert_header("Retry-After", "1");
            }
            if q.contains("Northern Lights") {
                return page(vec![alternate.clone(), chosen.clone()]);
            }
            if q.contains("Drift Signal") && !q.contains("Gallery Sessions") {
                return page(vec![known.clone()]);
            }
            page(Vec::new())
        })
        .mount(&f.provider)
        .await;
    let tracker = new_metadata_tracker();
    let launch = || enrich_all_metadata(State(f.state.clone()), Extension(tracker.clone()));
    assert_eq!(launch().await.unwrap().0["status"], "started");
    assert_eq!(launch().await.unwrap_err().0, StatusCode::CONFLICT);
    assert_eq!(
        metadata_task_dismiss(Extension(tracker.clone()))
            .await
            .into_response()
            .status(),
        StatusCode::CONFLICT
    );
    timeout(Duration::from_secs(15), async {
        while f.provider.received_requests().await.unwrap().is_empty() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("first provider request");
    let late = f.track("Added after cutoff", f.credited, None, None).await;
    let result = completed(&tracker).await;
    assert_eq!(
        result,
        json!({"total_processed":4,"enriched":1,"not_found":1,"ambiguous":1,"errors":1,"already_enriched":0})
    );
    assert_eq!(
        metadata_task_status(Extension(tracker.clone())).await.0["result"],
        result
    );
    assert_eq!(f.stored(late).await.musicbrainz_id, None);
    assert_eq!(f.stored(unmatched).await, before_unmatched);
    assert_eq!(f.stored(failed).await, before_failure);
    assert_eq!(f.stored(ambiguous).await, before);
    assert_eq!(entries().await, references);
    let saved = f.stored(identified).await;
    let real = artist::Entity::find_by_id(saved.artist_id)
        .one(&f.state.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.musicbrainz_id.as_deref(), Some(FOUND));
    assert_eq!(real.name, "New Credit");
    assert!(!real.is_placeholder && real.id != placeholder.id);
    assert_eq!(
        artist::Entity::find_by_id(placeholder.id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap(),
        placeholder
    );

    let before_manual = f.provider.received_requests().await.unwrap().len();
    for id in [remote, withdrawn] {
        assert!(matches!(
            f.enrich(id, Some(CHOICE)).await.status,
            super::MetadataStatus::NotFound
        ));
    }
    assert_eq!(f.stored(remote).await, before_remote);
    assert_eq!(f.stored(withdrawn).await, before_withdrawn);
    let requests = f.provider.received_requests().await.unwrap();
    assert_eq!(requests.len(), before_manual);
    let queries: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/recording")
        .map(query)
        .collect();
    assert!(queries.iter().all(|q| !q.contains("Unknown Artist")
        && !q.contains("Already enriched")
        && !q.contains("Foreign original")
        && !q.contains("Withdrawn original")
        && !q.contains("Added after cutoff")));
    let steps: Vec<_> = queries
        .iter()
        .filter(|q| q.contains("Missing One"))
        .collect();
    let artist = steps
        .iter()
        .position(|q| q.contains("Test Artist"))
        .expect("artist and title query");
    let album = steps
        .iter()
        .position(|q| q.contains("Gallery Sessions") && !q.contains("Test Artist"))
        .expect("release and title query");
    let title = steps
        .iter()
        .position(|q| !q.contains("Test Artist") && !q.contains("Gallery Sessions"))
        .expect("title query");
    assert!(artist < album && album < title, "{steps:?}");
    assert!(queries.iter().any(|q| q.contains("Offline Beacon")));

    let response = f.enrich(ambiguous, None).await;
    assert!(matches!(response.status, super::MetadataStatus::Ambiguous));
    let options: std::collections::BTreeSet<_> = response
        .candidates
        .iter()
        .map(|r| r.recording_mbid.as_str())
        .collect();
    assert_eq!(
        options,
        std::collections::BTreeSet::from([ALTERNATE, CHOICE])
    );
    assert_eq!(f.stored(ambiguous).await, before);
    let confirmed = f.enrich(ambiguous, Some(CHOICE)).await;
    assert!(matches!(confirmed.status, super::MetadataStatus::Enriched));
    let actual = f.stored(ambiguous).await;
    assert_eq!(actual.id, before.id);
    assert_eq!(actual.musicbrainz_id.as_deref(), Some(CHOICE));
    assert_eq!(actual.genre, before.genre);
    assert_eq!(actual.year, before.year);
    assert_eq!(actual.file_hash, before.file_hash);
    assert_eq!(actual.play_count, before.play_count);
    assert_eq!(entries().await, references);
    assert_eq!(
        artist::Entity::find_by_id(actual.artist_id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Selected Performer"
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL for a PostgreSQL connection"]
async fn recording_tags_take_priority_and_failed_confirmation_preserves_existing_metadata() {
    let f = Fixture::new().await;
    let tagged = f.track("first.wav", f.credited, None, None).await;
    let coded = f.track("second.wav", f.credited, None, None).await;
    let isrc = "USABC2600123";
    super::hints::save(
        &f.state.db,
        tagged,
        &soundtime_audio::recording_tags::RecordingTags {
            recording_id: Some(FOUND.into()),
            isrc: Some(isrc.into()),
        },
    )
    .await
    .unwrap();
    super::hints::save(
        &f.state.db,
        coded,
        &soundtime_audio::recording_tags::RecordingTags {
            recording_id: None,
            isrc: Some(isrc.into()),
        },
    )
    .await
    .unwrap();
    let by_id = recording(FOUND, "Tagged recording", "Test Artist", TEST_ARTIST);
    let mut by_isrc = recording(CHOICE, "Coded recording", "Test Artist", TEST_ARTIST);
    by_isrc["isrcs"] = json!([isrc]);
    f.detail(by_id).await;
    f.detail(by_isrc.clone()).await;
    Mock::given(method("GET"))
        .and(path("/recording"))
        .respond_with(move |r: &Request| {
            if query(r).contains(isrc) {
                page(vec![by_isrc.clone()])
            } else {
                page(vec![])
            }
        })
        .mount(&f.provider)
        .await;
    assert!(matches!(
        f.enrich(tagged, None).await.status,
        super::MetadataStatus::Enriched
    ));
    let requests = f.provider.received_requests().await.unwrap();
    assert!(
        !requests.iter().any(|r| r.url.path() == "/recording"),
        "A tagged MBID must not need a text or ISRC search"
    );
    assert_eq!(f.stored(tagged).await.title, "Tagged recording");
    assert!(matches!(
        f.enrich(coded, None).await.status,
        super::MetadataStatus::Enriched
    ));
    let saved = f.stored(coded).await;
    assert_eq!(saved.musicbrainz_id.as_deref(), Some(CHOICE));
    assert_eq!(saved.title, "Coded recording");
    let requests = f.provider.received_requests().await.unwrap();
    let searched: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/recording")
        .map(query)
        .collect();
    assert!(searched.iter().any(|q| q.contains(isrc)));
    assert!(searched
        .iter()
        .all(|q| !q.contains("first.wav") && !q.contains("second.wav")));
    Mock::given(method("GET"))
        .and(path(format!("/recording/{ALTERNATE}")))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&f.provider)
        .await;
    assert!(matches!(
        f.enrich(coded, Some(ALTERNATE)).await.status,
        super::MetadataStatus::Error
    ));
    assert_eq!(f.stored(coded).await, saved);
}
