//! Bounded native release transport; trust is established by `Update::verify`, not HTTP.

use std::time::Duration;

#[derive(Clone, Copy)]
struct DownloadLimits {
    max_bytes: usize,
    connect: Duration,
    stale: Duration,
    total: Duration,
}

#[derive(Clone, Copy)]
enum UrlPolicy {
    PublicHttps,
    #[cfg(test)]
    Loopback,
}

impl UrlPolicy {
    fn validate(self, url: &url::Url) -> Result<(), String> {
        match self {
            Self::PublicHttps => super::release_policy::validate_url(url.as_str()).map(|_| ()),
            #[cfg(test)]
            Self::Loopback => {
                let loopback = match url.host() {
                    Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                    Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                    _ => false,
                };
                if loopback && matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty() && url.password().is_none()
                    && url.query().is_none() && url.fragment().is_none() {
                    Ok(())
                } else {
                    Err("Test download URL must be uncredentialed literal loopback".into())
                }
            }
        }
    }
}

async fn fetch_bytes(url: url::Url, limits: DownloadLimits, policy: UrlPolicy, progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    tokio::time::timeout(limits.total, fetch_http(url, limits, policy, progress))
        .await.map_err(|_| "Update download total timeout")?
}

async fn fetch_http(url: url::Url, limits: DownloadLimits, policy: UrlPolicy, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    policy.validate(&url)?;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(limits.connect)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 || policy.validate(attempt.url()).is_err() {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }));
    if matches!(policy, UrlPolicy::PublicHttps) {
        builder = builder.dns_resolver(std::sync::Arc::new(super::tools::PublicDnsResolver));
    }
    let client = builder
        .build()
        .map_err(|_| "Could not initialize update download")?;
    let response = tokio::time::timeout(limits.stale, client.get(url).send())
        .await.map_err(|_| "Update download stalled")?
        .map_err(|_| "Download request failed")?;
    if !response.status().is_success() {
        return Err("Update server returned an unsuccessful status".into());
    }
    if response.content_length().is_some_and(|length| length > limits.max_bytes as u64) {
        return Err("Update exceeds size limit".into());
    }
    let total = response.content_length();
    let mut response = response;
    let mut bytes = Vec::new();
    progress(0, total);
    while let Some(chunk) = tokio::time::timeout(limits.stale, response.chunk())
        .await.map_err(|_| "Update download stalled")?
        .map_err(|_| "Download body failed")? {
        append_chunk(&mut bytes, &chunk, limits.max_bytes)?;
        progress(bytes.len() as u64, total);
    }
    if total.is_some_and(|expected| expected != bytes.len() as u64) {
        return Err("Update download length mismatch".into());
    }
    Ok(bytes)
}

pub(super) async fn download(
    url: &url::Url,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    fetch_bytes(url.clone(), DownloadLimits {
        max_bytes: 512 * 1024 * 1024,
        connect: Duration::from_secs(15),
        stale: Duration::from_secs(30),
        total: Duration::from_secs(15 * 60),
    }, UrlPolicy::PublicHttps, progress).await
}

/// Fetch public static metadata with a tighter budget than the installer download.
pub(super) async fn download_manifest(url: &url::Url) -> Result<serde_json::Value, String> {
    fetch_manifest(url.clone(), UrlPolicy::PublicHttps).await
}

async fn fetch_manifest(url: url::Url, policy: UrlPolicy) -> Result<serde_json::Value, String> {
    let bytes = fetch_bytes(url, DownloadLimits {
        max_bytes: 1024 * 1024,
        connect: Duration::from_secs(10),
        stale: Duration::from_secs(15),
        total: Duration::from_secs(30),
    }, policy, |_, _| {}).await?;
    // Deserialize only after the complete response has passed the streaming size cap.
    serde_json::from_slice(&bytes).map_err(|_| "Invalid update manifest JSON".into())
}

fn append_chunk(bytes: &mut Vec<u8>, chunk: &[u8], max_bytes: usize) -> Result<(), String> {
    let length = bytes.len().checked_add(chunk.len()).ok_or("Update exceeds size limit")?;
    if length > max_bytes {
        return Err("Update exceeds size limit".into());
    }
    // Never allocate from untrusted Content-Length, and check the cap before growing.
    bytes.try_reserve(chunk.len()).map_err(|_| "Not enough memory for update")?;
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn limits() -> DownloadLimits {
        DownloadLimits { max_bytes: 8, connect: Duration::from_secs(1), stale: Duration::from_millis(150), total: Duration::from_secs(2) }
    }

    async fn server(response: &'static [u8]) -> (url::Url, tokio::task::JoinHandle<()>) {
        scripted_server(vec![(Duration::ZERO, response.to_vec())]).await
    }

    async fn scripted_server(steps: Vec<(Duration, Vec<u8>)>) -> (url::Url, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/package", listener.local_addr().unwrap()).parse().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                    return;
                }
                request.push(byte[0]);
                assert!(request.len() < 16384);
            }
            for (delay, bytes) in steps {
                tokio::time::sleep(delay).await;
                if stream.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        });
        (url, task)
    }

    #[tokio::test]
    async fn rejects_manifest_over_one_mib_without_content_length_before_parsing() {
        // Invalid JSON deliberately proves the streaming cap wins over deserialization.
        let mut response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        response.extend(vec![b'x'; 1024 * 1024 + 1]);
        let (url, task) = scripted_server(vec![(Duration::ZERO, response)]).await;
        let result = fetch_manifest(url, UrlPolicy::Loopback).await;
        task.await.unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("size limit"), "{error}");
    }

    #[tokio::test]
    async fn rejects_manifest_advertising_over_one_mib_before_reading_body() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n").await;
        let error = fetch_manifest(url, UrlPolicy::Loopback).await.unwrap_err();
        task.await.unwrap();
        assert!(error.contains("size limit"), "{error}");
    }

    #[tokio::test]
    async fn rejects_chunked_manifest_over_one_mib_before_parsing() {
        let mut response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n100000\r\n".to_vec();
        response.extend(vec![b'x'; 1024 * 1024]);
        response.extend_from_slice(b"\r\n1\r\nx\r\n0\r\n\r\n");
        let (url, task) = scripted_server(vec![(Duration::ZERO, response)]).await;
        let error = fetch_manifest(url, UrlPolicy::Loopback).await.unwrap_err();
        task.await.unwrap();
        assert!(error.contains("size limit"), "{error}");
    }

    #[tokio::test]
    async fn accepts_manifest_exactly_one_mib() {
        let mut body = br#"{"version":"0.3.1"}"#.to_vec();
        body.resize(1024 * 1024, b' ');
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        response.extend(body);
        let (url, task) = scripted_server(vec![(Duration::ZERO, response)]).await;
        let raw_json = fetch_manifest(url, UrlPolicy::Loopback).await.unwrap();
        task.await.unwrap();
        assert_eq!(raw_json["version"], "0.3.1");
    }

    #[tokio::test]
    async fn rejects_invalid_manifest_json_below_the_cap() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json").await;
        let error = fetch_manifest(url, UrlPolicy::Loopback).await.unwrap_err();
        task.await.unwrap();
        assert_eq!(error, "Invalid update manifest JSON");
    }

    #[tokio::test]
    async fn production_manifest_wrapper_rejects_loopback_before_connecting() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/manifest.json", listener.local_addr().unwrap()).parse().unwrap();
        assert!(download_manifest(&url).await.is_err());
        assert!(tokio::time::timeout(Duration::from_millis(50), listener.accept()).await.is_err());
    }

    #[tokio::test]
    async fn bounded_manifest_feeds_official_release_metadata_parser() {
        use base64::Engine;
        let encode = |value: &str| base64::engine::general_purpose::STANDARD.encode(value);
        // Public throwaway vectors from the updater's offline tests; never install anything.
        let key = encode("untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe");
        let signature = encode("untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==");
        let metadata = serde_json::json!({
            "version": "2.0.0",
            "notes": "Bounded release notes",
            "platforms": {
                "manifest-bound-target": {
                    "url": "https://updates.example.com/package",
                    "signature": signature
                },
                "other-target": {
                    "url": "https://updates.example.com/wrong-package",
                    "signature": "wrong-signature"
                }
            }
        });
        let body = serde_json::to_vec(&metadata).unwrap();
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        response.extend(body);
        let (url, task) = scripted_server(vec![(Duration::ZERO, response)]).await;
        let raw_json = fetch_manifest(url, UrlPolicy::Loopback).await.unwrap();
        task.await.unwrap();

        // Parser resmi diuji tanpa membuat event loop GUI di executable unit test.
        super::super::release_policy::validate_key(&key).unwrap();
        let release: tauri_plugin_updater::RemoteRelease = serde_json::from_value(raw_json.clone()).unwrap();
        assert_eq!(raw_json, metadata);
        assert_eq!(release.version.to_string(), "2.0.0");
        assert_eq!(release.download_url("manifest-bound-target").unwrap().as_str(), "https://updates.example.com/package");
        assert_eq!(release.signature("manifest-bound-target").unwrap(), &signature);
        assert_eq!(release.notes.as_deref(), Some("Bounded release notes"));
        assert!(release.download_url("absent-target").is_err());
        assert!(super::super::release_policy::is_newer(env!("CARGO_PKG_VERSION"), &release.version.to_string()));
        for version in ["0.2.9", env!("CARGO_PKG_VERSION"), "0.3.0+metadata"] {
            assert!(!super::super::release_policy::is_newer(env!("CARGO_PKG_VERSION"), version));
        }
        let mut malformed = raw_json;
        malformed["version"] = serde_json::json!("not-semver");
        assert!(serde_json::from_value::<tauri_plugin_updater::RemoteRelease>(malformed).is_err());
    }

    #[tokio::test]
    async fn downloads_valid_body_with_cumulative_progress() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\npackage!").await;
        let mut progress = Vec::new();
        let bytes = fetch_bytes(url, limits(), UrlPolicy::Loopback, |received, total| {
            progress.push((received, total));
        }).await.unwrap();
        task.await.unwrap();
        assert_eq!(bytes, b"package!");
        assert_eq!(progress.first(), Some(&(0, Some(8))));
        assert_eq!(progress.last(), Some(&(8, Some(8))));
        assert!(progress.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[tokio::test]
    async fn rejects_chunked_overflow_without_reporting_over_limit_progress() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n").await;
        let mut progress = Vec::new();
        let result = fetch_bytes(url, limits(), UrlPolicy::Loopback, |received, total| {
            progress.push((received, total));
        }).await;
        task.await.unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("size limit"), "{error}");
        assert_eq!(progress.first(), Some(&(0, None)));
        assert!(progress.iter().all(|(received, total)| *received <= 8 && total.is_none()));
    }

    #[tokio::test]
    async fn accepts_chunked_body_exactly_at_the_cap() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n1234\r\n4\r\n5678\r\n0\r\n\r\n").await;
        let mut progress = Vec::new();
        let bytes = fetch_bytes(url, limits(), UrlPolicy::Loopback, |received, total| {
            progress.push((received, total));
        }).await.unwrap();
        task.await.unwrap();
        assert_eq!(bytes, b"12345678");
        assert_eq!(progress.last(), Some(&(8, None)));
    }

    #[tokio::test]
    async fn rejects_oversized_length_before_a_stalled_body_or_progress() {
        let (url, task) = scripted_server(vec![
            (Duration::ZERO, b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n".to_vec()),
            (Duration::from_secs(1), b"123456789".to_vec()),
        ]).await;
        let mut reported = false;
        let result = tokio::time::timeout(Duration::from_millis(500), fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| reported = true)).await;
        task.abort();
        let error = result.expect("must reject before body arrives").unwrap_err();
        assert!(error.contains("size limit"), "{error}");
        assert!(!reported);
    }

    #[tokio::test]
    async fn rejects_stalled_body() {
        let (url, task) = scripted_server(vec![
            (Duration::ZERO, b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nx".to_vec()),
            (Duration::from_millis(800), b"xxxxxxx".to_vec()),
        ]).await;
        let result = tokio::time::timeout(Duration::from_millis(500), fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| {})).await;
        task.abort();
        let error = result.expect("stale timeout must bound body reads").unwrap_err();
        assert!(error.contains("stalled"), "{error}");
    }

    #[tokio::test]
    async fn rejects_stalled_response_headers() {
        let (url, task) = scripted_server(vec![
            (Duration::from_millis(800), b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()),
        ]).await;
        let result = tokio::time::timeout(Duration::from_millis(500), fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| {})).await;
        task.abort();
        let error = result.expect("stale timeout must bound headers").unwrap_err();
        assert!(error.contains("stalled"), "{error}");
    }

    #[tokio::test]
    async fn total_timeout_bounds_a_continuously_active_body() {
        let mut steps = vec![(Duration::ZERO, b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n".to_vec())];
        steps.extend((0..8).map(|_| (Duration::from_millis(80), b"x".to_vec())));
        let (url, task) = scripted_server(steps).await;
        let mut short_limits = limits();
        short_limits.stale = Duration::from_millis(300);
        short_limits.total = Duration::from_millis(350);
        let result = fetch_bytes(url, short_limits, UrlPolicy::Loopback, |_, _| {}).await;
        task.abort();
        let error = result.unwrap_err();
        assert!(error.contains("total timeout"), "{error}");
    }

    #[tokio::test]
    async fn production_policy_rejects_initial_loopback_before_connecting() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/package", listener.local_addr().unwrap()).parse().unwrap();
        let mut reported = false;
        let result = tokio::time::timeout(Duration::from_millis(500), fetch_bytes(url, limits(), UrlPolicy::PublicHttps, |_, _| reported = true)).await;
        assert!(result.expect("URL must be rejected before connecting").is_err());
        assert!(!reported);
        assert!(tokio::time::timeout(Duration::from_millis(50), listener.accept()).await.is_err());
    }

    #[tokio::test]
    async fn rejects_credentialed_redirect_before_connecting_to_target() {
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let response = format!("HTTP/1.1 302 Found\r\nLocation: http://user:secret@{}/payload\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.local_addr().unwrap());
        let (url, task) = scripted_server(vec![(Duration::ZERO, response.into_bytes())]).await;
        let result = tokio::time::timeout(Duration::from_millis(500), fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| {})).await;
        task.await.unwrap();
        assert!(result.expect("redirect must be rejected before connecting").is_err());
        assert!(tokio::time::timeout(Duration::from_millis(50), target.accept()).await.is_err());
    }

    #[tokio::test]
    async fn follows_allowed_loopback_redirect() {
        let (target_url, target_task) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
        let response = format!("HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let (url, task) = scripted_server(vec![(Duration::ZERO, response.into_bytes())]).await;
        let bytes = fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| {}).await.unwrap();
        task.await.unwrap();
        target_task.await.unwrap();
        assert_eq!(bytes, b"ok");
    }

    #[tokio::test]
    async fn rejects_oversized_advertised_length_without_reading_body() {
        let (url, task) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 536870913\r\nConnection: close\r\n\r\n").await;
        let error = fetch_bytes(url, limits(), UrlPolicy::Loopback, |_, _| {}).await.unwrap_err();
        task.await.unwrap();
        assert!(error.contains("size limit"), "{error}");
    }

    #[test]
    fn rejects_over_limit_chunk_before_growing_the_buffer() {
        let mut bytes = vec![1, 2, 3];
        let capacity = bytes.capacity();
        let error = append_chunk(&mut bytes, &[4, 5], 4).unwrap_err();
        assert!(error.contains("size limit"));
        assert_eq!(bytes, [1, 2, 3]);
        assert_eq!(bytes.capacity(), capacity);
    }
}
