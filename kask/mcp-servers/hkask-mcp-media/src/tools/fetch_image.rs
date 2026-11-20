//! One labelled-click image fetch, for reading surfaces.
//!
//! `media_fetch_image` is the fetch behind the nostr panel's labelled
//! click: one image over HTTPS from a public host, into a content-addressed
//! cache under the artifacts dir (`media-mcp/image-cache` — the
//! `agent_paths::mcp_artifacts_subdir` route, which names `cache` as an
//! artifact type), never into the gallery: one-off reading material is
//! not a curated asset.
//!
//! Bounds (operator approval 2026-11-16): https only, public hosts only
//! (no loopback, private, or link-local destination, no embedded
//! credentials — the same `validate_download_url` preflight every other
//! download path runs), no redirects, a 60 s connection bound, and a
//! 10 MiB byte cap. The response must declare an `image/*` type and carry
//! bytes the `image` crate can measure, so a lying content type never
//! reaches the cache.
//!
//! The cache is content-addressed: the image bytes live at
//! `<sha256(bytes)>.<ext>` and a per-URL sidecar records which content a
//! URL fetched, so the name of a cached file is checkable against its own
//! bytes. A hit re-hashes the file and refetches on mismatch — a corrupted
//! cache entry heals instead of showing.

use std::path::{Path, PathBuf};

use crate::types::MediaFetchImageRequest;
use crate::*;
use hkask_types::agent_paths;
use hkask_types::hashing::{sha256_bytes, sha256_file};

/// The most bytes one fetched image may carry (operator-approved cap).
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Where the cache lives, under the artifacts dir. Not the gallery: one-off
/// reading material never becomes a curated asset.
fn image_cache_dir() -> Result<PathBuf, McpToolError> {
    let dir = agent_paths::resolve_under_artifacts_dir(&agent_paths::mcp_artifacts_subdir(
        "media",
        "image-cache",
    ));
    std::fs::create_dir_all(&dir).map_err(|error| {
        McpToolError::internal(format!("create the image cache directory: {error}"))
    })?;
    Ok(dir)
}

/// The scheme policy for one image URL. `allow_fixture` admits plain `http`
/// on loopback for the local test fixtures — the same carve-out
/// [`crate::assets::validate_download_url`] carries for the host; a
/// production build passes `false` and fetches `https` only.
fn image_url_scheme_admitted(parsed: &reqwest::Url, allow_fixture: bool) -> bool {
    if parsed.scheme() == "https" {
        return true;
    }
    allow_fixture
        && parsed.scheme() == "http"
        && matches!(parsed.host_str(), Some("127.0.0.1" | "::1" | "[::1]"))
}

/// The URL gate, before any network contact: the scheme policy and the
/// no-credentials rule. Each refusal names its rule — never the URL, which
/// is third-party text that may carry credentials (the SSRF layer's own
/// rule: a rejection never retains the address it refused).
fn require_image_url(url: &str, allow_fixture: bool) -> Result<(), McpToolError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| McpToolError::invalid_argument(format!("not a URL: {error}")))?;
    if !image_url_scheme_admitted(&parsed, allow_fixture) {
        return Err(McpToolError::invalid_argument(format!(
            "only https image URLs are fetched, not {}",
            parsed.scheme()
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(McpToolError::invalid_argument(
            "the URL carries a user name or password: image URLs with embedded credentials are refused"
                .to_string(),
        ));
    }
    Ok(())
}

/// One fetched image: where it sits locally, what it claims to be, how
/// big it is, and whether this call contacted the network.
#[derive(Debug)]
struct FetchedImage {
    path: PathBuf,
    content_type: String,
    bytes: u64,
    cached: bool,
}

/// The per-URL cache sidecar: which content a URL fetched, and what the
/// server declared it to be.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CacheMeta {
    content_sha256: String,
    content_type: String,
}

impl CacheMeta {
    fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// The file name a fetched image's bytes live at: the hash of those bytes,
/// so the name is checkable against the content, and the declared type's
/// subtype as the extension.
fn content_file_name(content_sha256: &str, content_type: &str) -> String {
    let subtype = content_type
        .strip_prefix("image/")
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .take(8)
        .collect::<String>();
    let extension = if subtype.is_empty() { "img" } else { &subtype };
    format!("{content_sha256}.{extension}")
}

/// Write bytes under `name` in `dir`, through a sibling temp file and a
/// rename, so the cache never holds a partial image under a
/// content-addressed name.
fn persist(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, McpToolError> {
    let final_path = dir.join(name);
    let temp_path = dir.join(format!(".{name}.tmp"));
    let write =
        std::fs::write(&temp_path, bytes).and_then(|()| std::fs::rename(&temp_path, &final_path));
    if let Err(error) = write {
        let _ = std::fs::remove_file(&temp_path);
        return Err(McpToolError::internal(format!(
            "persist the fetched image {}: {error}",
            final_path.display()
        )));
    }
    Ok(final_path)
}

/// Fetch one image, or serve it from the cache when this URL's content is
/// already there. [`FetchedImage`] is the whole result — the tool adds the
/// JSON shape, nothing else.
async fn fetch_image_cached(url: &str) -> Result<FetchedImage, McpToolError> {
    require_image_url(url, cfg!(test))?;
    // The public-host preflight (no loopback, private, or link-local
    // destination, host resolves). Its own message names the URL, so the
    // refusal the caller reads is fixed text; the detail goes to the log.
    crate::assets::validate_download_url(url, cfg!(test))
        .await
        .map_err(|error| {
            tracing::warn!(
                target: "hkask.mcp.media",
                %url,
                %error,
                "image fetch refused by the URL preflight"
            );
            McpToolError::invalid_argument(
                "the image URL is refused: only a public host is fetched — no loopback, private, or link-local address, no embedded credentials, and the host must resolve"
                    .to_string(),
            )
        })?;
    let cache_dir = image_cache_dir()?;
    let url_key = sha256_bytes(url.as_bytes());
    let meta_path = cache_dir.join(format!("{url_key}.json"));

    if let Some(meta) = CacheMeta::read(&meta_path) {
        let content_path =
            cache_dir.join(content_file_name(&meta.content_sha256, &meta.content_type));
        let stored = std::fs::metadata(&content_path)
            .ok()
            .map(|metadata| metadata.len())
            .filter(|_| sha256_file(&content_path).is_ok_and(|hash| hash == meta.content_sha256));
        if let Some(bytes) = stored {
            return Ok(FetchedImage {
                path: content_path,
                content_type: meta.content_type,
                bytes,
                cached: true,
            });
        }
        // A sidecar whose content is gone or changed is a miss, not a
        // refusal: the next fetch rewrites both and heals the cache.
    }

    let asset = crate::assets::download_asset_with_limit(url, MAX_IMAGE_BYTES, cfg!(test))
        .await
        .map_err(map_media_error)?;
    let Some(content_type) = asset.content_type else {
        return Err(McpToolError::invalid_argument(
            "the server sent no content type; only image/* is fetched".to_string(),
        ));
    };
    if !content_type.starts_with("image/") {
        return Err(McpToolError::invalid_argument(format!(
            "the server sent {content_type}; only image/* is fetched"
        )));
    }
    let (width, height) = crate::assets::infer_image_dimensions(&asset.bytes);
    if width == 0 || height == 0 {
        return Err(McpToolError::invalid_argument(
            "the server sent bytes that are not a decodable image".to_string(),
        ));
    }
    let content_sha256 = sha256_bytes(&asset.bytes);
    let path = persist(
        &cache_dir,
        &content_file_name(&content_sha256, &content_type),
        &asset.bytes,
    )?;
    let meta = CacheMeta {
        content_sha256,
        content_type,
    };
    std::fs::write(
        &meta_path,
        serde_json::to_string(&meta).map_err(|error| {
            McpToolError::internal(format!("serialize the image cache sidecar: {error}"))
        })?,
    )
    .map_err(|error| {
        McpToolError::internal(format!(
            "write the image cache sidecar {}: {error}",
            meta_path.display()
        ))
    })?;
    Ok(FetchedImage {
        path,
        content_type: meta.content_type,
        bytes: asset.bytes.len() as u64,
        cached: false,
    })
}

#[tool_router(router = fetch_image_router, vis = "pub")]
impl MediaServer {
    /// Fetch one image for a reading surface, or serve it from the cache.
    ///
    /// This tool contacts the network once per uncached URL. A repeated
    /// URL is served from the local cache without contacting the network.
    #[tool(
        description = "Fetch one image over HTTPS from a public host into a content-addressed cache under the artifacts directory and return its local path, content type, size, and whether it came from the cache. Contacts the network once per uncached URL (never for a cached one); refuses non-https schemes, non-public hosts, embedded credentials, redirects, connections that stall past 60 s, responses over 10 MiB, and content types other than image/*. Not the gallery: fetched images are reading material, not curated assets."
    )]
    pub async fn media_fetch_image(
        &self,
        Parameters(MediaFetchImageRequest { url }): Parameters<MediaFetchImageRequest>,
    ) -> Result<String, McpToolError> {
        let url = url.trim().to_string();
        execute_tool(self, "media_fetch_image", async move {
            if url.is_empty() {
                return Err(McpToolError::invalid_argument("url must not be empty"));
            }
            let fetched = fetch_image_cached(&url).await?;
            Ok(serde_json::json!({
                "url": url,
                "path": fetched.path.to_string_lossy(),
                "content_type": fetched.content_type,
                "bytes": fetched.bytes,
                "cached": fetched.cached,
            }))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Points `HKASK_ARTIFACTS_DIR` at a fresh temp dir for one test and
    /// restores the prior value on drop (the `ARTIFACTS_ENV_LOCK`
    /// discipline: the tests that share the env var serialize on that
    /// lock, which each test takes first).
    struct ArtifactsEnvGuard(Option<String>);
    impl ArtifactsEnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let prior = std::env::var("HKASK_ARTIFACTS_DIR").ok();
            unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", path) };
            Self(prior)
        }
    }
    impl Drop for ArtifactsEnvGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
                None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
            }
        }
    }

    /// A local PNG the fixture can serve, and the fetch tool can cache.
    fn png_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::new(2, 2)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode a 2x2 PNG");
        bytes
    }

    /// One HTTP fixture: serves `body` as `content_type` on loopback,
    /// counting every connection it serves. The fetch tool's test-build
    /// loopback carve-out (`cfg!(test)` in `validate_download_url`) is what
    /// admits this address; a production build refuses it before any
    /// connection.
    struct ServedImage {
        addr: std::net::SocketAddr,
        served: std::sync::Arc<AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl ServedImage {
        fn start(content_type: &'static str, body: Vec<u8>) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fixture");
            let addr = listener.local_addr().expect("fixture address");
            let served = std::sync::Arc::new(AtomicUsize::new(0));
            let counter = std::sync::Arc::clone(&served);
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = std::sync::Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                use std::io::{Read, Write};
                while !stopped.load(Ordering::SeqCst) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        break;
                    };
                    let mut buffer = [0_u8; 512];
                    let _ = stream.read(&mut buffer);
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            });
            Self {
                addr,
                served,
                stop,
                thread: Some(thread),
            }
        }

        fn url(&self, path: &str) -> String {
            // Plain http on loopback: admitted only where the fixture
            // carve-out reaches (test builds); a production build refuses
            // this URL at the scheme gate.
            format!("http://{}/{path}", self.addr)
        }
    }
    impl Drop for ServedImage {
        fn drop(&mut self) {
            // Stop, then wake the accept loop so the thread can join.
            self.stop.store(true, Ordering::SeqCst);
            let _ = std::net::TcpStream::connect(self.addr);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// expect: One uncached URL contacts the network once and publishes a
    /// content-addressed file plus its sidecar; the same URL again is
    /// served from the cache without a second connection.
    #[tokio::test]
    async fn an_image_is_fetched_once_and_served_from_the_cache_after() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let image = ServedImage::start("image/png", png_bytes());
        let url = image.url("photo.png");

        let first = fetch_image_cached(&url).await.expect("the first fetch");
        assert!(!first.cached, "an uncached URL contacts the network");
        assert_eq!(first.content_type, "image/png");
        assert_eq!(first.bytes, png_bytes().len() as u64);
        assert!(first.path.exists(), "the returned path exists");
        assert_eq!(
            first.path.file_name().and_then(|name| name.to_str()),
            Some(format!("{}.png", sha256_bytes(&png_bytes())).as_str()),
            "the image lives under the hash of its own bytes"
        );

        let second = fetch_image_cached(&url).await.expect("the second fetch");
        assert!(second.cached, "a repeated URL is served from the cache");
        assert_eq!(second.path, first.path);

        assert_eq!(
            image.served.load(Ordering::SeqCst),
            1,
            "the cache served the second fetch without a connection"
        );
        let cache_dir = image_cache_dir().expect("cache dir");
        let files: Vec<_> = std::fs::read_dir(&cache_dir)
            .expect("read the cache dir")
            .collect();
        assert_eq!(files.len(), 2, "one image file and one sidecar, no more");
    }

    /// expect: Only https is fetched — the production scheme policy. The
    /// fixture carve-out admits plain http, but only on loopback, and the
    /// refusals here happen before any connection, so the fixture counts
    /// zero.
    #[tokio::test]
    async fn nothing_but_https_is_fetched() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let image = ServedImage::start("image/png", png_bytes());

        for url in [
            "http://example.com/photo.png".to_string(),
            format!("ftp://{}/photo.png", image.addr),
            "file:///etc/passwd".to_string(),
        ] {
            let refused = fetch_image_cached(&url)
                .await
                .expect_err("non-https must be refused");
            assert!(
                refused.message.contains("https"),
                "the refusal names the https rule: {refused}"
            );
        }
        assert_eq!(
            image.served.load(Ordering::SeqCst),
            0,
            "every refusal happened before any connection"
        );
    }

    /// expect: The production scheme policy is https only — the fixture
    /// carve-out admits plain http on loopback and nothing else.
    #[test]
    fn the_scheme_policy_admits_only_https_except_the_test_fixture() {
        let parse = |url: &str| reqwest::Url::parse(url).expect("parse");
        for url in ["https://example.com/x.png", "https://127.0.0.1/x.png"] {
            assert!(
                image_url_scheme_admitted(&parse(url), false),
                "production admits https anywhere: {url}"
            );
        }
        for url in [
            "http://example.com/x.png",
            "http://127.0.0.1/x.png",
            "ftp://example.com/x.png",
            "file:///etc/passwd",
        ] {
            assert!(
                !image_url_scheme_admitted(&parse(url), false),
                "production refuses everything but https: {url}"
            );
        }
        assert!(
            image_url_scheme_admitted(&parse("http://127.0.0.1/x.png"), true),
            "the fixture carve-out admits plain http on loopback"
        );
        assert!(
            !image_url_scheme_admitted(&parse("http://example.com/x.png"), true),
            "the fixture carve-out admits no public plain http"
        );
    }

    /// expect: A URL carrying credentials is refused — the carve-out that
    /// admits loopback in test builds never admits an address with a user
    /// name or password in it.
    #[tokio::test]
    async fn an_address_with_credentials_is_refused() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let image = ServedImage::start("image/png", png_bytes());
        let url = format!("http://user:pass@{}/photo.png", image.addr);

        let refused = fetch_image_cached(&url)
            .await
            .expect_err("credentials must be refused");
        assert!(
            refused.message.contains("credentials"),
            "the refusal names the credentials rule: {refused}"
        );
        assert_eq!(image.served.load(Ordering::SeqCst), 0);
    }

    /// expect: A response that is not an image is refused before anything
    /// is cached.
    #[tokio::test]
    async fn a_non_image_response_is_refused_and_not_cached() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let image = ServedImage::start("text/html", b"<p>not an image</p>".to_vec());
        let url = image.url("page");

        let refused = fetch_image_cached(&url)
            .await
            .expect_err("a non-image response must be refused");
        assert!(
            refused.message.contains("image/"),
            "the refusal names the image rule: {refused}"
        );
        let cache_dir = image_cache_dir().expect("cache dir");
        assert_eq!(
            std::fs::read_dir(&cache_dir)
                .expect("read the cache dir")
                .count(),
            0,
            "a refused response leaves no cache entry"
        );
    }

    /// expect: A declared size over the cap is refused by the download
    /// bound, before the body is read.
    #[tokio::test]
    async fn an_oversized_image_is_refused() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());

        // A fixture that declares more than the cap: the client refuses at
        // the content-length check, so no body is read and its size does
        // not matter.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let addr = listener.local_addr().expect("fixture address");
        let thread = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 512];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_IMAGE_BYTES + 1
                )
                .as_bytes(),
            );
        });
        let url = format!("http://{addr}/big.png");

        let refused = fetch_image_cached(&url)
            .await
            .expect_err("an oversized image must be refused");
        assert!(
            refused.message.contains("exceeds"),
            "the refusal names the cap: {refused}"
        );
        let _ = thread.join();
    }

    /// expect: A cache entry whose bytes changed under its name is
    /// refetched — the hash in the file name is the entry's integrity, not
    /// a decoration.
    #[tokio::test]
    async fn a_corrupted_cache_entry_is_refetched() {
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir().expect("artifacts dir");
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let image = ServedImage::start("image/png", png_bytes());
        let url = image.url("photo.png");

        let first = fetch_image_cached(&url).await.expect("the first fetch");
        std::fs::write(&first.path, b"corrupted").expect("corrupt the cached image");

        let healed = fetch_image_cached(&url).await.expect("the healing fetch");
        assert!(
            !healed.cached,
            "a corrupted entry is a miss, not a served image"
        );
        assert_eq!(healed.path, first.path);
        assert_eq!(
            std::fs::read(&first.path).expect("the refetched image"),
            png_bytes()
        );
    }
}
