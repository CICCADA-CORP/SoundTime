//! Browsers on HTTPS cannot reliably open a broadcaster's HTTP-only stream.
//! This endpoint accepts a station UUID and forwards only continuous audio.
use super::{access, directory::Directory};
use crate::auth::middleware::MediaListener;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::stream;
use reqwest::{redirect::Policy, Client};
use soundtime_db::AppState;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::time::{timeout, Instant};
use url::{Host, Url};
use uuid::Uuid;

const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const RECHECK: Duration = Duration::from_secs(15);

pub(super) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(a == 0
                || a >= 224
                || v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_documentation()
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && ((b == 0 && c == 0) || (b == 88 && c == 99)))
                || (a == 198 && matches!(b, 18 | 19)))
        }
        IpAddr::V6(v) => {
            let [first, second, ..] = v.segments();
            first & 0xe000 == 0x2000
                && first != 0x2002
                && !(first == 0x2001 && (second < 0x200 || second == 0xdb8))
                && !(first == 0x3fff && second < 0x1000)
        }
    }
}

async fn broadcaster(url: &Url) -> Result<reqwest::Response, StatusCode> {
    let bad = StatusCode::BAD_GATEWAY;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad);
    }
    let port = url.port_or_known_default().ok_or(bad)?;
    let mut client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(OPEN_TIMEOUT)
        .read_timeout(Duration::from_secs(15))
        .user_agent("SoundTime-Webradio/1.0");
    match url.host().ok_or(bad)? {
        Host::Domain(host) => {
            let name = host.trim_end_matches('.');
            if !name.contains('.')
                || [".local", ".localhost", ".internal", ".home.arpa"]
                    .iter()
                    .any(|s| name.ends_with(s))
            {
                return Err(bad);
            }
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| bad)?
                .take(32)
                .collect();
            if addrs.is_empty() || addrs.iter().any(|a| !public_address(a.ip())) {
                return Err(bad);
            }
            // Only these validated addresses can serve this request, including after a DNS change.
            client = client.resolve_to_addrs(host, &addrs);
        }
        Host::Ipv4(v) if public_address(v.into()) => {}
        Host::Ipv6(v) if public_address(v.into()) => {}
        _ => return Err(bad),
    }
    client
        .build()
        .map_err(|_| bad)?
        .get(url.clone())
        .send()
        .await
        .map_err(|_| bad)
}

async fn open(mut url: Url) -> Result<(reqwest::Response, &'static str), StatusCode> {
    for _ in 0..=3 {
        let response = broadcaster(&url).await?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|h| h.to_str().ok())
                .ok_or(StatusCode::BAD_GATEWAY)?;
            url = url.join(location).map_err(|_| StatusCode::BAD_GATEWAY)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(StatusCode::BAD_GATEWAY);
        }
        let value = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let mime = match value.as_str() {
            "audio/mpeg" | "audio/mp3" | "audio/mpeg3" => "audio/mpeg",
            "audio/aac" | "audio/aacp" => "audio/aac",
            "audio/ogg" | "application/ogg" => "audio/ogg",
            "audio/flac" | "audio/x-flac" => "audio/flac",
            "audio/opus" => "audio/opus",
            "audio/mp4" | "audio/x-m4a" => "audio/mp4",
            "audio/webm" => "audio/webm",
            "audio/wav" | "audio/wave" | "audio/x-wav" => "audio/wav",
            _ => return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
        };
        return Ok((response, mime));
    }
    Err(StatusCode::BAD_GATEWAY)
}

pub async fn stream_station(
    State(state): State<Arc<AppState>>,
    listener: Option<Extension<MediaListener>>,
    Extension(directory): Extension<Arc<Directory>>,
    Path(id): Path<Uuid>,
) -> Result<Response, StatusCode> {
    let user = listener.map(|Extension(MediaListener(id))| id);
    if !access(&state.db, user).await?.enabled {
        return Err(StatusCode::FORBIDDEN);
    }
    let station = directory.locate(id).await?;
    let url = Url::parse(&station.url).map_err(|_| StatusCode::BAD_GATEWAY)?;
    if station.hls || url.scheme() != "http" {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let (response, mime) = timeout(OPEN_TIMEOUT, open(url))
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)??;
    let body = stream::try_unfold(
        (response, state, Instant::now() + RECHECK),
        move |(mut source, state, mut next)| async move {
            if Instant::now() >= next {
                if !access(&state.db, user).await.is_ok_and(|v| v.enabled) {
                    return Ok(None);
                }
                next = Instant::now() + RECHECK;
            }
            let data = source.chunk().await.map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>(data.map(|chunk| (chunk, (source, state, next))))
        },
    );
    Ok((
        [
            (header::CONTENT_TYPE.as_str(), mime),
            (header::CACHE_CONTROL.as_str(), "private, no-store"),
            ("x-accel-buffering", "no"),
        ],
        Body::from_stream(body),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_only_opens_public_unicast_addresses() {
        for ip in [
            "0.0.0.0",
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.100.100.200",
            "198.19.0.1",
            "192.0.2.8",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::a",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "2002:7f00:1::",
            "2001:db8::1",
            "3fff::1",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(ip.parse().unwrap()), "{ip}");
        }
    }
}
