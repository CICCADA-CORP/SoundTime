use super::*;
use serde_json::json;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

fn mock_directory(mock: &MockServer) -> Podcasts {
    let mut p = Podcasts::new(Client::new());
    p.apple = mock.uri();
    p.charts = mock.uri();
    p
}

#[tokio::test]
async fn apple_directory_is_market_scoped_and_coalesces_identical_searches() {
    let mock = MockServer::start().await;
    let p = mock_directory(&mock);
    let search = json!({"kind":"podcast","collectionId":22,"collectionName":"Journal","artistName":"Radio Test","primaryGenreName":"News","artworkUrl100":"http://127.0.0.1/image"});
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("term", "Été & voix"))
        .and(query_param("entity", "podcast"))
        .and(query_param("media", "podcast"))
        .and(query_param("country", "FR"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"results":[search,search,{"kind":"song"}]}))
                .set_delay(Duration::from_millis(10)),
        )
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("GET")).and(path("/api/v2/ca/podcasts/top/25/podcasts.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"feed":{"results":[{"kind":"podcasts","id":"33","name":"Le Québec","artistName":"CA","genres":[{"name":"Podcasts"},{"name":"Histoire"}]}]}})))
        .expect(1).mount(&mock).await;
    let (a, b) = tokio::join!(
        p.browse("FR", "Été & voix", 0),
        p.browse("FR", "Été & voix", 0)
    );
    for page in [a.unwrap(), b.unwrap()] {
        assert_eq!(page.shows.len(), 1);
        assert_eq!(page.shows[0].id, "22");
        assert!(page.shows[0].artwork_url.is_none());
    }
    let chart = p.browse("CA", "", 0).await.unwrap();
    assert_eq!(chart.shows[0].genre, "Histoire");
    assert_eq!(chart.shows[0].id, "33");
}

#[tokio::test]
async fn lookup_checks_podcast_identity_and_rejects_private_feed_before_connecting() {
    let mock = MockServer::start().await;
    let p = mock_directory(&mock);
    Mock::given(method("GET")).and(path("/lookup")).and(query_param("id","77")).and(query_param("country","FR"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[
            {"collectionId":76,"kind":"podcast","feedUrl":"https://publisher.example/feed"},
            {"collectionId":77,"collectionName":"Radio Test","kind":"podcast","feedUrl":"http://127.0.0.1/feed"}
        ]}))).expect(1).mount(&mock).await;
    assert_eq!(
        p.feed("FR", "77").await.err(),
        Some(StatusCode::UNPROCESSABLE_ENTITY)
    );
    assert_eq!(
        p.feed("FR", "77").await.err(),
        Some(StatusCode::UNPROCESSABLE_ENTITY)
    );
    assert_eq!(
        p.feed("FR", "00077").await.err(),
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        p.feed("FR", "https://example.org/s").await.err(),
        Some(StatusCode::BAD_REQUEST)
    );
}

#[test]
fn feed_pages_keep_parent_and_explicit_bounds() {
    let mut body = "<rss><channel>".to_owned();
    for n in 0..502 {
        body.push_str(&format!("<item><title>Episode {n}</title><enclosure url='https://publisher.example/{n}.mp3' type='audio/mpeg'/></item>"));
    }
    body.push_str("</channel></rss>");
    let (episodes, truncated) = rss::episodes(body.as_bytes(), false).unwrap();
    let archive = Archive {
        show: show(
            &json!({"kind":"podcast","collectionId":22,"collectionName":"Journal"}),
            false,
        )
        .unwrap(),
        episodes,
        truncated,
    };
    let first = archive.page(0);
    assert_eq!(first.show.id, "22");
    assert_eq!(first.episodes.len(), 50);
    assert_eq!(first.next_offset, Some(50));
    assert!(first.truncated);
    assert_eq!(archive.page(450).next_offset, None);
    assert!(archive.page(u32::MAX).episodes.is_empty());
}

#[tokio::test]
async fn chart_falls_back_to_apples_legacy_storefront_without_unbounded_calls() {
    let mock = MockServer::start().await;
    let p = mock_directory(&mock);
    Mock::given(path("/api/v2/fr/podcasts/top/25/podcasts.json"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(path("/fr/rss/toppodcasts/limit=25/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"feed":{"entry":[
            {"id":{"attributes":{"im:id":"23"}},"im:name":{"label":"Old format"},"im:artist":{"label":"RTL"},
            "im:image":[{"label":"https://example.org/show.jpg"}],"category":{"attributes":{"label":"Culture"}}}
        ]}}))).expect(1).mount(&mock).await;
    let rows = p.browse("FR", "", 0).await.unwrap().shows;
    assert_eq!(rows[0].title, "Old format");
    assert_eq!(rows[0].genre, "Culture");
    p.apple_calls.lock().unwrap().resize(20, Instant::now());
    assert_eq!(
        p.browse("FR", "new query", 0).await.err(),
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
    assert_eq!(p.browse("FR", "", 0).await.unwrap().shows[0].id, "23");
}

#[tokio::test]
async fn reads_only_feed_bytes_and_never_requests_an_enclosure() {
    let mock = MockServer::start().await;
    let client = Client::new();
    Mock::given(path("/feed")).respond_with(ResponseTemplate::new(200).set_body_raw(
        r#"<rss><channel><item><title>Public discourse</title><enclosure type="audio/mp4" url="https://cdn.example.net/test.m4a"/></item></channel></rss>"#, "application/rss+xml")).expect(1).mount(&mock).await;
    Mock::given(path("/audio"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(vec![0; 100], "audio/mpeg"))
        .expect(1)
        .mount(&mock)
        .await;
    let parent = || {
        show(
            &json!({"kind":"podcast","collectionId":2,"collectionName":"Public Series"}),
            false,
        )
        .unwrap()
    };
    let r = client
        .get(format!("{}/feed", mock.uri()))
        .send()
        .await
        .unwrap();
    let page = read_archive(r, parent()).await.unwrap().page(0);
    assert_eq!(page.episodes.len(), 1);
    assert_eq!(page.show.title, "Public Series");
    let r = client
        .get(format!("{}/audio", mock.uri()))
        .send()
        .await
        .unwrap();
    assert_eq!(read_archive(r, parent()).await.err(), Some(BAD));
}

#[tokio::test]
async fn text_search_pages_share_one_bounded_result_across_offsets() {
    let mock = MockServer::start().await;
    let podcasts = mock_directory(&mock);
    let mut results = vec![json!({"collectionId":14,"kind":"song"})];
    for i in 1..=205 {
        let show = json!({"collectionId":i,"kind":"podcast","collectionName":format!("Show {i}")});
        results.push(show.clone());
        if i == 17 {
            results.push(show);
        }
    }
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("limit", "200"))
        .and(query_param("term", "talk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":results})))
        .expect(1)
        .mount(&mock)
        .await;
    let (first, second) = tokio::join!(
        podcasts.browse("US", "talk", 0),
        podcasts.browse("US", "talk", 25)
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!((first.shows.len(), first.next_offset), (25, Some(25)));
    assert_eq!(first.shows.last().unwrap().id, "25");
    assert_eq!(
        (second.shows[0].id.as_str(), second.next_offset),
        ("26", Some(50))
    );
    let last = podcasts.browse("US", "talk", 175).await.unwrap();
    assert_eq!((last.shows.len(), last.next_offset), (25, None));
    assert_eq!(last.shows.last().unwrap().id, "200");
    let beyond = podcasts.browse("US", "talk", u32::MAX).await.unwrap();
    assert!(beyond.shows.is_empty() && beyond.next_offset.is_none());
}
