use super::*;
use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

fn row(id: Uuid) -> Value {
    json!({"stationuuid":id,"name":" Radio One ","url":"https://broadcast.example/playlist.pls",
        "url_resolved":"http://broadcast.example/live","favicon":"https://art.example/logo.png",
        "tags":"Jazz, lounge, JAZZ,, lounge","country":"France","language":"french","codec":"AAC",
        "bitrate":128,"hls":0,"lastcheckok":1})
}

#[test]
fn missing_and_unknown_codecs_are_omitted_from_station_details() {
    for codec in [
        None,
        Some(Value::Null),
        Some(json!("  ")),
        Some(json!(" Unknown ")),
    ] {
        let mut raw = row(Uuid::new_v4());
        if let Some(value) = codec {
            raw["codec"] = value;
        } else {
            raw.as_object_mut().unwrap().remove("codec");
        }
        let listing: Listing = serde_json::from_value(raw).unwrap();
        let station = listing.station().unwrap();
        assert!(station.codec.is_empty());
        assert_eq!(station.description, "France · french · 128 kbps");
    }
}

#[tokio::test]
async fn paged_search_coalesces_requests_and_uses_the_working_mirror() {
    let down = MockServer::start().await;
    let up = MockServer::start().await;
    let dir = Directory::with_mirrors(vec![down.uri(), up.uri()]).unwrap();
    let id = Uuid::new_v4();
    let active = row(id);
    let mut broken = row(Uuid::new_v4());
    broken["lastcheckok"] = json!(0);
    let mut next = row(Uuid::new_v4());
    next["description"] = json!("On air from Lyon");
    next["favicon"] = json!("https://secret@art.example/logo");
    let search = || {
        Mock::given(method("GET"))
            .and(path("/json/stations/search"))
            .and(header(
                "User-Agent",
                concat!("SoundTime/", env!("CARGO_PKG_VERSION")),
            ))
            .and(query_param("name", "Été & nuit"))
            .and(query_param("tag", "r&b"))
            .and(query_param("limit", "3"))
            .and(query_param("hidebroken", "true"))
    };
    search()
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&down)
        .await;
    search()
        .and(query_param("offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(25))
                .set_body_json(json!([active, broken, next.clone()])),
        )
        .expect(1)
        .mount(&up)
        .await;
    search()
        .and(query_param("offset", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([next])))
        .expect(1)
        .mount(&up)
        .await;

    let first = || dir.stations("Été & nuit", "r&b", 0, 2);
    let (a, b) = tokio::join!(first(), first());
    let a = a.unwrap();
    assert_eq!(a.next_offset, Some(2));
    assert_eq!(a.stations.len(), 1);
    assert_eq!(a.stations[0].id, id);
    assert_eq!(a.stations[0].tags, ["Jazz", "lounge"]);
    assert_eq!(
        a.stations[0].description,
        "France · french · AAC · 128 kbps"
    );
    assert_eq!(
        a.stations[0].icon_url.as_deref(),
        Some("https://art.example/logo.png")
    );
    assert_eq!(b.unwrap().stations[0].name, "Radio One");
    assert_eq!(first().await.unwrap().stations[0].id, id);
    let last = dir.stations("Été & nuit", "r&b", 2, 2).await.unwrap();
    assert!(last.next_offset.is_none());
    assert_eq!(last.stations[0].description, "On air from Lyon");
    assert!(last.stations[0].icon_url.is_none());
    assert_eq!(up.received_requests().await.unwrap().len(), 2);
    assert_eq!(down.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn play_counts_once_locate_counts_nothing_and_both_return_the_resolved_url() {
    let server = MockServer::start().await;
    let dir = Directory::with_mirrors(vec![server.uri()]).unwrap();
    let id = Uuid::new_v4();
    let mut listing = row(id);
    listing["hls"] = json!(1);
    Mock::given(method("GET"))
        .and(path(format!("/json/stations/byuuid/{id}")))
        .and(query_param("hidebroken", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([listing])))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/json/url/{id}")))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let located = dir.locate(id).await.unwrap();
    assert!(located.hls);
    assert_eq!(located.url, "http://broadcast.example/live");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    let played = dir.play(id).await.unwrap();
    assert_eq!(played.url, located.url);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn unknown_broken_and_unsafe_stations_are_never_counted() {
    let server = MockServer::start().await;
    let dir = Directory::with_mirrors(vec![server.uri()]).unwrap();
    for kind in ["missing", "mismatched", "broken", "unsafe"] {
        let id = Uuid::new_v4();
        let mut station = row(id);
        let rows = match kind {
            "missing" => vec![],
            "mismatched" => vec![row(Uuid::new_v4())],
            "broken" => {
                station["lastcheckok"] = json!(0);
                vec![station]
            }
            _ => {
                station["url_resolved"] = json!("file:///var/music");
                vec![station]
            }
        };
        Mock::given(method("GET"))
            .and(path(format!("/json/stations/byuuid/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(rows))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(dir.play(id).await.unwrap_err(), StatusCode::NOT_FOUND);
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn oversized_and_invalid_directory_responses_and_capacity_are_bounded() {
    let large = MockServer::start().await;
    let invalid = MockServer::start().await;
    let dir = Directory::with_mirrors(vec![large.uri(), invalid.uri()]).unwrap();
    let search = || Mock::given(method("GET")).and(path("/json/stations/search"));
    search()
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; MAX_JSON + 1]))
        .expect(1)
        .mount(&large)
        .await;
    search()
        .respond_with(ResponseTemplate::new(200).set_body_string("invalid json"))
        .expect(1)
        .mount(&invalid)
        .await;
    assert_eq!(
        dir.stations("", "", 0, 1).await.unwrap_err(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        dir.stations("", "", u32::MAX, 1).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    let _all = dir.permits.acquire_many(MAX_PARALLEL as u32).await.unwrap();
    assert_eq!(
        dir.locate(Uuid::new_v4()).await.unwrap_err(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    for i in 0..(MAX_PAGES as u32 + 1) {
        assert_eq!(
            dir.stations("many", "", i, 1).await.unwrap_err(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    assert_eq!(dir.pages.lock().unwrap().len(), MAX_PAGES);
}

#[test]
fn browser_urls_exclude_credentials_local_addresses_and_active_schemes() {
    for url in [
        "//music.example/live",
        "https://name:pw@music.example/live",
        "data:image/png,x",
        "javascript:alert(1)",
        "http://127.1/a",
        "http://[::1]/a",
        "http://router.local/a",
        "http://100.64.12.1/",
        "http://10.0.0.2/live",
        "http://[::ffff:127.0.0.1]/",
        "http://localhost./a",
    ] {
        assert!(web_url(url).is_none(), "{url}");
    }
    let mut fallback = row(Uuid::new_v4());
    fallback["url_resolved"] = json!("");
    fallback["country"] = json!("");
    fallback["countrycode"] = json!("FR");
    let listing: Listing = serde_json::from_value(fallback).unwrap();
    assert_eq!(
        listing.stream().unwrap().url,
        "https://broadcast.example/playlist.pls"
    );
    assert_eq!(listing.station().unwrap().country, "FR");
}
