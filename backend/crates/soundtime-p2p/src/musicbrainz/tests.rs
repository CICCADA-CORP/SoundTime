use super::*;
use serde_json::json;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

const FIRST: &str = "01111111-1111-4111-8111-111111111111";
const SECOND: &str = "02222222-2222-4222-8222-222222222222";
const ARTIST: &str = "03333333-3333-4333-8333-333333333333";
const RELEASE: &str = "04444444-4444-4444-8444-444444444444";

fn candidate(id: &str, title: &str, artist: &str, album: &str, length: u64) -> Recording {
    serde_json::from_value(json!({
        "id":id, "title":title, "length":length, "score":92,
        "artist-credit":[{"artist":{"id":ARTIST,"name":artist}}],
        "releases":[{"id":RELEASE,"title":album,"date":"1999"}],
        "tags":[{"name":"chanson","count":2}]
    }))
    .unwrap()
}
fn hints<'a>(
    title: &'a str,
    artist: Option<&'a str>,
    album: Option<&'a str>,
    duration_ms: Option<u64>,
) -> Hints<'a> {
    Hints {
        title,
        artist,
        album,
        duration_ms,
    }
}
fn winner(hints: &Hints<'_>, rows: &[Recording]) -> String {
    match ranking::choose(hints, rows) {
        RecordingMatch::Reliable(r) => r.id,
        other => panic!("expected a reliable choice, got {other:?}"),
    }
}

#[test]
fn accents_and_punctuation_do_not_promote_a_high_scoring_namesake() {
    let mut wrong = candidate(FIRST, "L'âme sœur", "Someone else", "Rêves", 212_000);
    wrong.score = 100;
    let right = candidate(SECOND, "L’âme sœur", "Hélène", "Rêves", 212_000);
    assert_eq!(
        winner(
            &hints("L’a\u{302}me soeur", Some("HELENE"), None, None),
            &[wrong, right]
        ),
        SECOND
    );
}
#[test]
fn homonyms_need_independent_evidence_and_the_remote_score_does_not_break_a_tie() {
    let mut first = candidate(FIRST, "Stay", "The Band", "Red", 201_000);
    first.score = 100;
    let second = candidate(SECOND, "Stay", "The Band", "Blue", 241_000);
    let rows = [first, second];
    assert!(
        matches!(ranking::choose(&hints("Stay",Some("The Band"),None,None),&rows),
        RecordingMatch::Ambiguous(r) if r.len()==2)
    );
    assert_eq!(
        winner(&hints("Stay", Some("The Band"), Some("Blue"), None), &rows),
        SECOND
    );
    assert_eq!(
        winner(&hints("Stay", None, None, Some(241_100)), &rows),
        SECOND
    );
    assert!(matches!(
        ranking::choose(&hints("Stay", None, None, None), &rows[..1]),
        RecordingMatch::Ambiguous(_)
    ));
    assert!(matches!(
        ranking::choose(&hints("Unrelated", Some("The Band"), None, None), &rows),
        RecordingMatch::Absent
    ));
}
#[test]
fn collaboration_uses_credited_names_and_join_phrases() {
    let mut duet = candidate(FIRST, "Empire", "JAY-Z", "The Blueprint", 210_000);
    duet.artist_credit[0].name = Some("Jay Z".into());
    duet.artist_credit[0].joinphrase = " feat. ".into();
    duet.artist_credit.push(ArtistCredit {
        name: None,
        joinphrase: String::new(),
        artist: Artist {
            id: SECOND.into(),
            name: "Alicia Keys".into(),
            sort_name: None,
        },
    });
    assert_eq!(duet.credited_artist(), "Jay Z feat. Alicia Keys");
    assert_eq!(
        winner(
            &hints("Empire", Some("JAY-Z & Alicia Keys"), None, None),
            std::slice::from_ref(&duet)
        ),
        FIRST
    );
    assert_eq!(
        winner(
            &hints("Empire (feat. Alicia Keys)", Some("JAY-Z"), None, None),
            std::slice::from_ref(&duet)
        ),
        FIRST
    );
    assert!(matches!(
        ranking::choose(
            &hints("Empire (feat. Other)", Some("JAY-Z"), None, None),
            &[duet]
        ),
        RecordingMatch::Absent
    ));
}
#[test]
fn named_versions_and_live_disambiguations_cannot_replace_a_studio_recording() {
    assert_eq!(
        text_queries(&hints("Souvenir (live in Paris)", Some("Elle"), None, None)),
        [
            r#"recording:"Souvenir (live in Paris)" AND artist:"Elle""#,
            r#"recording:"Souvenir" AND artist:"Elle""#,
            r#"recording:"Souvenir""#
        ]
    );
    let studio = candidate(FIRST, "Souvenir", "Elle", "Disque", 200_000);
    let mut live = candidate(SECOND, "Souvenir", "Elle", "Disque", 200_000);
    live.disambiguation = Some("live, Paris, 2015".into());
    let rows = [live, studio];
    assert_eq!(
        winner(&hints("Souvenir", Some("Elle"), None, None), &rows),
        FIRST
    );
    assert_eq!(
        winner(
            &hints("Souvenir (live in Paris)", Some("Elle"), None, None),
            &rows
        ),
        SECOND
    );
    assert!(matches!(
        ranking::choose(
            &hints("Souvenir (live in Rome)", Some("Elle"), None, None),
            &rows
        ),
        RecordingMatch::Absent
    ));
    let mixes = [
        candidate(FIRST, "Souvenir (Club Remix)", "Elle", "Disque", 210_000),
        candidate(SECOND, "Souvenir (Léa Remix)", "Elle", "Disque", 210_000),
    ];
    assert_eq!(
        winner(
            &hints("Souvenir – Lea Remix", Some("Elle"), None, None),
            &mixes
        ),
        SECOND
    );
}
#[test]
fn close_titles_need_more_than_an_artist_or_provider_score() {
    let r = candidate(FIRST, "Different Worlds", "One", "Album", 180_000);
    let rows = [r];
    assert!(matches!(
        ranking::choose(&hints("Diferent Worlds", Some("One"), None, None), &rows),
        RecordingMatch::Ambiguous(_)
    ));
    assert_eq!(
        winner(
            &hints("Diferent Worlds", Some("One"), Some("Album"), None),
            &rows
        ),
        FIRST
    );
    assert_eq!(
        isrc_code("fr-z03-16-00123").as_deref(),
        Some("FRZ031600123")
    );
    assert!(isrc_code("ISRC?").is_none());
    assert_eq!(
        quoted("artist", r#"Elle "et" Lui\"#),
        r#"artist:"Elle \"et\" Lui\\""#
    );
}

#[test]
fn search_accepts_both_musicbrainz_score_encodings_and_nullable_lists() {
    for score in [json!(95), json!("95")] {
        let rec: Recording =
            serde_json::from_value(json!({"id":FIRST,"title":"Title","score":score,
            "artist-credit":null,"releases":null,"tags":null}))
            .unwrap();
        assert_eq!(rec.score, 95);
        assert!(rec.tags.is_empty());
    }
}

#[tokio::test]
async fn artist_lookup_surfaces_a_persistent_503_after_three_attempts() {
    let server = MockServer::start().await;
    Mock::given(path(format!("/artist/{ARTIST}")))
        .and(query_param("inc", "url-rels"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "1"))
        .expect(3)
        .mount(&server)
        .await;
    assert!(get_artist(&Client::new(), &server.uri(), ARTIST)
        .await
        .is_err());
}
#[tokio::test]
async fn missing_mbid_falls_back_to_exact_isrc_then_returns_full_recording() {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(path(format!("/recording/{FIRST}")))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let mut candidate = candidate(SECOND, "Nom", "Voix", "Album", 190_000);
    candidate.isrcs.push("FRZ031600123".into());
    Mock::given(path("/recording"))
        .and(query_param("query", r#"isrc:"FRZ031600123""#))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"recordings":[candidate],"count":1})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/recording/{SECOND}")))
        .and(query_param("inc", "artist-credits+releases+tags+isrcs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&candidate))
        .expect(1)
        .mount(&server)
        .await;
    let found = resolve_tagged_recording(
        &Client::new(),
        &base,
        Some(FIRST.parse().unwrap()),
        Some("fr-z03-16-00123"),
        "",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let RecordingMatch::Reliable(r) = found else {
        panic!("exact ISRC lost")
    };
    assert_eq!(
        (
            r.id.as_str(),
            r.artist_credit[0].artist.id.as_str(),
            r.releases[0].id.as_str()
        ),
        (SECOND, ARTIST, RELEASE)
    );
    assert_eq!(r.tags[0].name, "chanson");
}
#[tokio::test]
async fn shared_isrc_returns_manual_candidates_without_selecting_the_first_result() {
    let server = MockServer::start().await;
    let rows = [
        candidate(FIRST, "Same", "Artist", "Release", 180_000),
        candidate(SECOND, "Same", "Artist", "Release", 180_000),
    ];
    Mock::given(method("GET"))
        .and(path("/recording"))
        .and(query_param("query", r#"isrc:"FRZ031600123""#))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"recordings":rows,"count":2})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = resolve_tagged_recording(
        &Client::new(),
        &server.uri(),
        None,
        Some("FRZ031600123"),
        "Same",
        Some("Artist"),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(result,RecordingMatch::Ambiguous(list) if list.len()==2));
}
#[tokio::test]
async fn retry_and_progressive_lookup_use_one_clock_and_keep_legacy_shape() {
    let server = MockServer::start().await;
    let base = server.uri();
    let client = MusicBrainzClient {
        http: Client::new(),
        base_url: base.clone(),
    };
    let row = candidate(FIRST, "Chanson", "Léa", "Printemps", 182_000);
    Mock::given(path("/recording"))
        .and(query_param(
            "query",
            r#"recording:"Chanson" AND artist:"Léa""#,
        ))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .with_priority(1)
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/recording"))
        .and(query_param(
            "query",
            r#"recording:"Chanson" AND artist:"Léa""#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"recordings":[]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/recording"))
        .and(query_param("query", r#"recording:"Chanson""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"recordings":[row]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("/recording/{FIRST}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&row))
        .expect(1)
        .mount(&server)
        .await;
    let start = Instant::now();
    let r = client.lookup_recording("Chanson", "Léa").await.unwrap();
    assert!(start.elapsed() >= Duration::from_secs(3));
    assert_eq!(r.artist_name.as_deref(), Some("Léa"));
    assert_eq!(r.release_title.as_deref(), Some("Printemps"));
    assert_eq!(r.year, Some(1999));
}

#[tokio::test]
async fn concurrent_artist_and_recording_helpers_share_the_provider_request_rate() {
    use std::sync::{Arc, Mutex};
    let server = MockServer::start().await;
    let starts = Arc::new(Mutex::new(Vec::new()));
    for (resource, template) in [
        (
            format!("/artist/{ARTIST}"),
            ResponseTemplate::new(200).set_body_json(json!({"id":ARTIST})),
        ),
        (
            format!("/recording/{FIRST}"),
            ResponseTemplate::new(200)
                .set_body_json(candidate(FIRST, "Chanson", "Léa", "Album", 190_000)),
        ),
    ] {
        let starts = Arc::clone(&starts);
        Mock::given(path(resource))
            .respond_with(move |_: &wiremock::Request| {
                starts.lock().unwrap().push(Instant::now());
                template.clone()
            })
            .expect(1)
            .mount(&server)
            .await;
    }
    let base = server.uri();
    let client = Client::new();
    let (artist, recording) = tokio::join!(
        get_artist(&client, &base, ARTIST),
        get_recording(&client, &base, FIRST.parse().unwrap())
    );
    assert!(artist.unwrap().status().is_success());
    assert_eq!(recording.unwrap().unwrap().id, FIRST);
    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 2);
    assert!(starts[1].duration_since(starts[0]) >= Duration::from_millis(950));
}

#[tokio::test]
async fn title_only_truncated_repetitions_prompt_once_and_never_select() {
    let server = MockServer::start().await;
    let repeated = candidate(FIRST, "Hello! Hello! Hello!", "One", "A", 200_000);
    let exact = candidate(SECOND, "Hello", "Two", "B", 210_000);
    for (q, rows) in [
        (r#"recording:"Hello""#, vec![repeated.clone()]),
        (
            r#"recording:"Hello" AND NOT recording:"hello hello""#,
            vec![repeated, exact],
        ),
    ] {
        Mock::given(path("/recording"))
            .and(query_param("query", q))
            .and(query_param("limit", "25"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"count":10000,"recordings":rows})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let answer = resolve_recording(&Client::new(), &server.uri(), "Hello", None, None, None)
        .await
        .unwrap();
    assert!(
        matches!(answer,RecordingMatch::Ambiguous(rows) if rows.len()==1 && rows[0].id==SECOND)
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn incomplete_title_only_search_without_plausible_hits_needs_more_information() {
    let server = MockServer::start().await;
    let row = candidate(FIRST, "Night Waves at Dawn", "One", "A", 200_000);
    let rows = vec![row; PAGE_SIZE];
    Mock::given(path("/recording"))
        .and(query_param("query", r#"recording:"Night Wave""#))
        .and(query_param("offset", "25"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"count":500,"recordings":[]})),
        )
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/recording"))
        .and(query_param("query", r#"recording:"Night Wave""#))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"count":500,"recordings":rows})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let answer = resolve_recording(
        &Client::new(),
        &server.uri(),
        "Night Wave",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(answer,RecordingMatch::Ambiguous(rows) if rows.is_empty()));

    Mock::given(path("/recording"))
        .and(query_param("query", r#"recording:"No such title""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"count":0,"recordings":[]})))
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        resolve_recording(
            &Client::new(),
            &server.uri(),
            "No such title",
            None,
            None,
            None
        )
        .await
        .unwrap(),
        RecordingMatch::Absent
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}
