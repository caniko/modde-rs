//! Shared download helpers reused across sources: streaming a response body to
//! disk with hash verification, retry/backoff, and parent-directory setup.

use std::path::Path;

use anyhow::Result;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

use crate::error::{SourceError, SourceResult, status_error};
use crate::traits::{DownloadHandle, ProgressCallback, VerifiedFile};

pub const MAX_RETRIES: u32 = 3;
pub const BACKOFF_BASE_MS: u64 = 1000;

/// Stream a response body to a file, reporting progress.
pub async fn stream_to_file(
    resp: reqwest::Response,
    dest: &Path,
    total_hint: u64,
    progress: &ProgressCallback,
) -> Result<u64> {
    let total = resp.content_length().unwrap_or(total_hint);
    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total);
    }

    file.flush().await?;
    Ok(downloaded)
}

/// Stream a response body to a file and verify Wabbajack-compatible hashes
/// while bytes are still hot in memory.
pub async fn stream_to_file_verified(
    resp: reqwest::Response,
    dest: &Path,
    expected_hash: u64,
    total_hint: u64,
    progress: &ProgressCallback,
) -> SourceResult<VerifiedFile> {
    let total = resp.content_length().unwrap_or(total_hint);
    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;
    let mut xxh64 = Xxh64::new(0);
    let mut xxh3 = Xxh3::new();

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        xxh64.update(&chunk);
        xxh3.update(&chunk);
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total);
    }
    file.flush().await?;

    let h64 = xxh64.digest();
    if h64 == expected_hash || xxh3.digest() == expected_hash {
        return Ok(VerifiedFile {
            path: dest.to_path_buf(),
            hash: expected_hash,
        });
    }

    let _ = tokio::fs::remove_file(dest).await;
    Err(SourceError::hash_mismatch(dest, expected_hash, h64))
}

/// Verify hash (xxh64 then xxh3 fallback) and return a `VerifiedFile`.
pub async fn verify_and_wrap(dest: &Path, expected_hash: u64) -> SourceResult<VerifiedFile> {
    modde_core::hash::verify_xxhash_compat(dest, expected_hash)
        .await
        .map_err(|error| match error {
            modde_core::CoreError::HashMismatch { .. } => {
                SourceError::HashMismatch { source: error }
            }
            error => SourceError::other(error),
        })?;
    Ok(VerifiedFile {
        path: dest.to_path_buf(),
        hash: expected_hash,
    })
}

/// Ensure parent directory exists.
pub async fn ensure_parent(dest: &Path) -> SourceResult<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

/// Execute an async operation with exponential backoff retries.
pub async fn with_retry<F, Fut, T>(label: &str, f: F) -> SourceResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = SourceResult<T>>,
{
    for attempt in 0..MAX_RETRIES {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if !e.is_retryable() {
                    return Err(e);
                }
                if attempt + 1 < MAX_RETRIES {
                    let base_delay = BACKOFF_BASE_MS * (1 << attempt);
                    // Add deterministic jitter: up to 50% of base delay, derived from attempt index
                    let jitter = base_delay / 4
                        + (base_delay / 2).wrapping_mul(u64::from(attempt) + 1)
                            % (base_delay / 2 + 1);
                    let delay = base_delay + jitter;
                    warn!(attempt = attempt + 1, delay_ms = delay, error = %e, "{label} failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                } else {
                    return Err(SourceError::other(anyhow::anyhow!(
                        "{label} failed after {MAX_RETRIES} attempts: {e:#}"
                    )));
                }
            }
        }
    }
    unreachable!("the 0..MAX_RETRIES loop always returns Ok or the final Err")
}

/// Simple download: GET, stream to file, verify hash, return `VerifiedFile`.
///
/// Handles `ensure_parent` internally — callers should not call it separately.
pub async fn simple_download(
    client: &reqwest::Client,
    handle: &DownloadHandle,
    dest: &Path,
    progress: &ProgressCallback,
) -> SourceResult<VerifiedFile> {
    ensure_parent(dest).await?;

    let resp = status_error(client.get(&handle.url).send().await?)?;

    let verified = stream_to_file_verified(
        resp,
        dest,
        handle.expected_hash,
        handle.size_hint.unwrap_or(0),
        progress,
    )
    .await?;
    debug!("download complete");
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use xxhash_rust::xxh3::xxh3_64;
    use xxhash_rust::xxh64::xxh64;

    use super::*;

    const BODY: &[u8] = b"payload";

    async fn serve() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        server
    }

    fn handle(server: &MockServer, route: &str, expected_hash: u64) -> DownloadHandle {
        DownloadHandle {
            url: format!("{}{route}", server.uri()),
            candidate_urls: Vec::new(),
            headers: std::collections::HashMap::new(),
            expected_hash,
            size_hint: None,
        }
    }

    #[tokio::test]
    async fn simple_download_verifies_either_hash_and_reports_progress() {
        let server = serve().await;
        let tmp = tempfile::tempdir().unwrap();
        let seen = Arc::new(AtomicU64::new(0));
        let progress: ProgressCallback = {
            let seen = seen.clone();
            Arc::new(move |done, _| seen.store(done, Ordering::Relaxed))
        };
        let client = reqwest::Client::new();

        for (name, hash) in [("a/xxh64", xxh64(BODY, 0)), ("b/xxh3", xxh3_64(BODY))] {
            let dest = tmp.path().join(name);
            let file = simple_download(&client, &handle(&server, "/file", hash), &dest, &progress)
                .await
                .unwrap();
            assert_eq!((file.path, file.hash), (dest.clone(), hash));
            assert_eq!(std::fs::read(&dest).unwrap(), BODY);
        }
        assert_eq!(seen.load(Ordering::Relaxed), BODY.len() as u64);

        let dest = tmp.path().join("bad");
        let err = simple_download(&client, &handle(&server, "/file", 1), &dest, &progress)
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::HashMismatch { .. }), "{err}");
        assert!(!dest.exists(), "a mismatched download must be removed");

        let missing = simple_download(&client, &handle(&server, "/gone", 1), &dest, &progress)
            .await
            .unwrap_err();
        assert!(!missing.is_retryable(), "{missing}");
    }

    #[tokio::test]
    async fn stream_to_file_writes_the_whole_body() {
        let server = serve().await;
        let tmp = tempfile::tempdir().unwrap();
        let resp = reqwest::get(format!("{}/file", server.uri()))
            .await
            .unwrap();
        let dest = tmp.path().join("out");
        let progress: ProgressCallback = Arc::new(|_, total| assert_eq!(total, 7));

        let written = stream_to_file(resp, &dest, 0, &progress).await.unwrap();
        assert_eq!(written, BODY.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    }

    #[tokio::test]
    async fn verify_and_wrap_distinguishes_mismatch_from_io_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("file");
        std::fs::write(&dest, BODY).unwrap();

        assert_eq!(
            verify_and_wrap(&dest, xxh64(BODY, 0)).await.unwrap().hash,
            xxh64(BODY, 0)
        );
        assert!(matches!(
            verify_and_wrap(&dest, 1).await,
            Err(SourceError::HashMismatch { .. })
        ));
        assert!(matches!(
            verify_and_wrap(&tmp.path().join("missing"), 1).await,
            Err(SourceError::Other(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn with_retry_retries_only_retryable_errors() {
        let calls = AtomicU32::new(0);
        let flaky = with_retry("flaky", || {
            let attempt = calls.fetch_add(1, Ordering::Relaxed) + 1;
            async move {
                if attempt < MAX_RETRIES {
                    Err(SourceError::other(anyhow::anyhow!("transient")))
                } else {
                    Ok(attempt)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(flaky, MAX_RETRIES);

        let down = with_retry("down", || async {
            Err::<(), _>(SourceError::other(anyhow::anyhow!("offline")))
        })
        .await
        .unwrap_err();
        assert!(
            format!("{down:#}").contains("down failed after 3 attempts: offline"),
            "{down:#}"
        );

        calls.store(0, Ordering::Relaxed);
        let fatal = with_retry("fatal", || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Err::<(), _>(SourceError::hash_mismatch("x", 1, 2)) }
        })
        .await
        .unwrap_err();
        assert!(matches!(fatal, SourceError::HashMismatch { .. }));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
