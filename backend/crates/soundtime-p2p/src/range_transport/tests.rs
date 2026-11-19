use super::*;
use iroh::{endpoint::presets::Minimal, Endpoint, EndpointAddr};
use iroh_blobs::store::mem::MemStore;
use tokio::sync::oneshot;

async fn fetch_allowed(connection: &Connection, request: &WindowRequest) -> Result<VerifiedWindow> {
    let (_keep_active, grant) = watch::channel(true);
    fetch_window(connection, request, grant).await
}

async fn endpoint() -> Result<Endpoint> {
    Ok(Endpoint::builder(Minimal)
        .clear_ip_transports()
        .clear_relay_transports()
        .portmapper_config(iroh::endpoint::PortmapperConfig::Disabled)
        .bind_addr("127.0.0.1:0")?
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?)
}

async fn connect(client: &Endpoint, server: &Endpoint) -> Result<(Connection, Connection)> {
    let address = EndpointAddr::new(server.id()).with_ip_addr(server.bound_sockets()[0]);
    let (outgoing, incoming) = tokio::join!(client.connect(address, ALPN), async {
        server
            .accept()
            .await
            .context("endpoint closed")?
            .await
            .map_err(anyhow::Error::from)
    });
    Ok((outgoing?, incoming?))
}

#[tokio::test]
async fn verified_seek_and_independent_publication_after_revocation() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let origin = endpoint().await?;
        let reader = endpoint().await?;
        let (outgoing, incoming) = connect(&reader, &origin).await?;
        let store = MemStore::new();
        let fixture: Vec<u8> = (0..2_000_015u32).map(|v| (v ^ (v >> 13)) as u8).collect();
        let hash = store.blobs().add_slice(&fixture).await?.hash;
        let first = WindowRequest {
            origin: origin.id(),
            publication: Uuid::new_v4(),
            revision: 2,
            hash,
            file_size: fixture.len() as u64,
            offset: fixture.len() as u64 - u64::from(WINDOW_BYTES),
            length: WINDOW_BYTES,
        };
        let second = WindowRequest {
            publication: Uuid::new_v4(),
            ..first.clone()
        };
        let (first_access, first_grant) = watch::channel(true);
        let (_second_access, second_grant) = watch::channel(true);
        let authorize = |peer, request: &WindowRequest| {
            if peer != reader.id()
                || request.origin != first.origin
                || request.revision != first.revision
                || request.hash != hash
                || request.file_size != first.file_size
            {
                return None;
            }
            if request.publication == first.publication {
                Some(first_grant.clone())
            } else if request.publication == second.publication {
                Some(second_grant.clone())
            } else {
                None
            }
        };
        let (data, served) = tokio::join!(
            fetch_allowed(&outgoing, &first),
            serve_window(&incoming, &store, authorize)
        );
        served?;
        let verified = data?;
        assert_eq!(verified.data.as_ref(), &fixture[first.offset as usize..]);
        assert!(
            verified.response_bytes > verified.data.len()
                && verified.response_bytes <= MAX_RESPONSE
        );

        // Suspend the origin while reading storage to revoke an actual in-flight request.
        let (encoding, started) = oneshot::channel();
        let slow_provider = serve_with(&incoming, authorize, |_| async {
            let _ = encoding.send(());
            std::future::pending().await
        });
        let revoke = async {
            started.await?;
            first_access.send(false)?;
            Ok::<_, anyhow::Error>(())
        };
        let (data, served, revoked) =
            tokio::join!(fetch_allowed(&outgoing, &first), slow_provider, revoke);
        revoked?;
        assert!(data.is_err());
        assert!(served.unwrap_err().to_string().contains("revoked"));

        let (data, served) = tokio::join!(
            fetch_allowed(&outgoing, &first),
            serve_window(&incoming, &store, authorize)
        );
        assert!(data.is_err() && served.is_err());
        let (data, served) = tokio::join!(
            fetch_allowed(&outgoing, &second),
            serve_window(&incoming, &store, authorize)
        );
        served?;
        assert_eq!(data?.data, verified.data);
        // This reader learns about a local ban after the provider has finished
        // writing a valid window and before its own read future resumes.
        let (local_policy, local_grant) = watch::channel(true);
        let mut waiting = Box::pin(fetch_window(&outgoing, &second, local_grant));
        tokio::select! {
            biased;
            served = serve_window(&incoming, &store, authorize) => served?,
            result = &mut waiting => panic!("window completed before provider: {result:?}"),
        }
        local_policy.send(false)?;
        assert!(waiting
            .await
            .unwrap_err()
            .to_string()
            .contains("local publication grant revoked"));

        let stale = WindowRequest {
            revision: 1,
            ..second
        };
        let (data, served) = tokio::join!(
            fetch_allowed(&outgoing, &stale),
            serve_window(&incoming, &store, authorize)
        );
        assert!(data.is_err() && served.is_err());
        Ok::<_, anyhow::Error>(())
    })
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn corrupt_proof_and_cancelled_reader_release_the_stream() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let origin = endpoint().await?;
        let reader = endpoint().await?;
        let (outgoing, incoming) = connect(&reader, &origin).await?;
        let store = MemStore::new();
        let hash = store.blobs().add_slice(vec![19; 300_000]).await?.hash;
        let request = WindowRequest {
            origin: origin.id(),
            publication: Uuid::new_v4(),
            revision: 1,
            hash,
            file_size: 300_000,
            offset: 240_123,
            length: 2_000,
        };
        let (_access, grant) = watch::channel(true);
        let authorize = |_, _: &WindowRequest| Some(grant.clone());
        let malicious = serve_with(&incoming, authorize, |request| async {
            let mut encoded = encode_window(&store, request).await?;
            *encoded.last_mut().unwrap() ^= 1;
            Ok(encoded)
        });
        let (data, served) = tokio::join!(fetch_allowed(&outgoing, &request), malicious);
        served?;
        assert!(data.is_err(), "corrupt window was released");

        let (encoding, started) = oneshot::channel();
        let slow_provider = serve_with(&incoming, authorize, |_| async {
            let _ = encoding.send(());
            std::future::pending().await
        });
        let cancel = async {
            let mut fetching = Box::pin(fetch_allowed(&outgoing, &request));
            tokio::select! {
                result = &mut fetching => panic!("slow source unexpectedly finished: {result:?}"),
                ready = started => ready.unwrap(),
            }
            drop(fetching);
        };
        let (_, result) = tokio::join!(cancel, slow_provider);
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("reader disconnected"));
        assert!(WindowRequest {
            offset: u64::MAX,
            ..request.clone()
        }
        .range()
        .is_err());
        assert!(WindowRequest {
            length: WINDOW_BYTES + 1,
            ..request
        }
        .range()
        .is_err());
        Ok::<_, anyhow::Error>(())
    })
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn another_authenticated_supplier_can_finish_only_the_missing_windows() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let origin = endpoint().await?;
        let mirror = endpoint().await?;
        let reader = endpoint().await?;
        let (first, first_server) = connect(&reader, &origin).await?;
        let (second, second_server) = connect(&reader, &mirror).await?;
        let contents: Vec<_> = (0..(WINDOW_BYTES * 3 + 17))
            .map(|i| (i ^ (i >> 12)) as u8)
            .collect();
        let store1 = MemStore::new();
        let store2 = MemStore::new();
        let hash = store1.blobs().add_slice(&contents).await?.hash;
        assert_eq!(store2.blobs().add_slice(&contents).await?.hash, hash);
        let (_active, grant) = watch::channel(true);
        let initial = WindowRequest {
            origin: origin.id(),
            publication: Uuid::new_v4(),
            revision: 1,
            hash,
            file_size: contents.len() as u64,
            offset: 0,
            length: WINDOW_BYTES,
        };
        let mut delivered = Vec::new();
        let (data, served) = tokio::join!(
            fetch_allowed(&first, &initial),
            serve_window(&first_server, &store1, |_, _| Some(grant.clone()))
        );
        served?;
        delivered.extend_from_slice(&data?.data);
        first_server.close(1u32.into(), b"provider lost");
        let mut req = initial;
        req.offset = u64::from(WINDOW_BYTES);
        assert!(fetch_allowed(&first, &req).await.is_err());
        let mut supplied_by_mirror = Vec::new();
        while req.offset < req.file_size {
            req.length = (req.file_size - req.offset).min(u64::from(WINDOW_BYTES)) as u32;
            let (data, served) = tokio::join!(
                fetch_allowed(&second, &req),
                serve_window(&second_server, &store2, |authenticated, requested| {
                    assert_eq!(authenticated, reader.id());
                    assert_eq!(requested, &req);
                    supplied_by_mirror.push((requested.offset, requested.length));
                    Some(grant.clone())
                })
            );
            served?;
            delivered.extend_from_slice(&data?.data);
            req.offset += u64::from(req.length);
        }
        assert_eq!(supplied_by_mirror[0].0, u64::from(WINDOW_BYTES));
        assert_eq!(
            supplied_by_mirror
                .iter()
                .map(|(_, len)| u64::from(*len))
                .sum::<u64>(),
            req.file_size - u64::from(WINDOW_BYTES)
        );
        assert_eq!(delivered, contents);
        Ok::<_, anyhow::Error>(())
    })
    .await?
}
