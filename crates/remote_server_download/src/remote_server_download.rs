//! Downloads the zed-kask remote-server binary to the local cache for SSH
//! development. Extracted from the (now-deleted) `auto_update` crate so that
//! the remote-server download path survives without the app self-update
//! machinery.
//!
//! The download hits the same release-asset endpoint as upstream Zed's
//! auto-updater (`/releases/<channel>/<version>/asset`), resolved via the
//! `HttpClientWithUrl` base URL. For zed-kask this should point at a
//! zed-kask-controlled artifact host; the URL is configurable via the
//! `http_client` base URL setting.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context as _, Result, bail, ensure};
use client::Client;
use gpui::AsyncApp;
use http_client::{HttpClient, HttpClientWithUrl, StatusCode, Url};
use paths::remote_servers_dir;
use release_channel::ReleaseChannel;
use semver::Version;
use serde::{Deserialize, Serialize};
use smol::fs as sfs;
use smol::io::{AsyncReadExt, AsyncWriteExt};
use smol::stream::StreamExt;

const REMOTE_SERVER_CACHE_LIMIT: usize = 5;
const MAX_RELEASE_REDIRECTS: usize = 5;
const MAX_REMOTE_SERVER_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_SHA256_SIDECAR_BYTES: usize = 4 * 1024;
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

const ALLOWED_RELEASE_ASSET_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "releases.githubusercontent.com",
    "codeload.github.com",
];

/// A release asset returned by the release endpoint.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReleaseAsset {
    pub version: String,
    pub url: String,
    #[serde(default, alias = "sha256sum", skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Serialize)]
struct AssetQuery<'a> {
    asset: &'a str,
    os: &'a str,
    arch: &'a str,
}

/// Download the remote-server binary for the given platform/channel/version
/// to the local cache, returning the path to the cached `.gz` file.
///
/// `set_status` is called with human-readable progress strings.
pub async fn download_remote_server_release(
    client: Arc<Client>,
    release_channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
    set_status: impl Fn(&str, &mut AsyncApp) + Send + 'static,
    cx: &mut AsyncApp,
) -> Result<PathBuf> {
    set_status("Fetching remote server release", cx);
    validate_cache_path_segment("os", os)?;
    validate_cache_path_segment("arch", arch)?;
    let release = get_release_asset(
        &client,
        release_channel,
        version,
        "zed-remote-server",
        os,
        arch,
        cx,
    )
    .await?;

    let servers_dir = remote_servers_dir();
    let (platform_dir, version_path) =
        build_version_cache_path(servers_dir, release_channel, os, arch, &release.version)?;
    sfs::create_dir_all(&platform_dir)
        .await
        .with_context(|| format!("creating remote server cache dir {:?}", platform_dir))?;

    let http_client = client.http_client();

    if sfs::metadata(&version_path).await.is_err() {
        log::info!(
            "downloading zed-remote-server {os} {arch} version {}",
            release.version
        );
        set_status("Downloading remote server", cx);
        download_remote_server_binary(&version_path, release, http_client).await?;
    }

    if let Err(error) =
        cleanup_remote_server_cache(&platform_dir, &version_path, REMOTE_SERVER_CACHE_LIMIT).await
    {
        log::warn!(
            "Failed to clean up remote server cache in {:?}: {error:#}",
            platform_dir
        );
    }

    Ok(version_path)
}

/// Get the download URL for the remote-server binary without downloading it.
///
/// Always returns `Ok(None)` so remote transports (`ssh`, `docker`) fall back
/// to `download_remote_server_release` locally — where HTTPS host allowlisting,
/// redirect validation, and SHA-256 digest verification are enforced before
/// uploading the verified archive — rather than executing an unverified `curl -L`
/// payload on the remote host.
pub async fn get_remote_server_release_url(
    client: Arc<Client>,
    _channel: ReleaseChannel,
    _version: Option<Version>,
    os: &str,
    arch: &str,
    _cx: &mut AsyncApp,
) -> Result<Option<String>> {
    validate_cache_path_segment("os", os)?;
    validate_cache_path_segment("arch", arch)?;
    validate_release_endpoint_base_url(&client.http_client().base_url())?;
    Ok(None)
}

async fn get_release_asset(
    client: &Arc<Client>,
    release_channel: ReleaseChannel,
    version: Option<Version>,
    asset: &str,
    os: &str,
    arch: &str,
    _cx: &mut AsyncApp,
) -> Result<ReleaseAsset> {
    validate_cache_path_segment("os", os)?;
    validate_cache_path_segment("arch", arch)?;

    let version = if let Some(mut version) = version {
        version.pre = semver::Prerelease::EMPTY;
        version.build = semver::BuildMetadata::EMPTY;
        version.to_string()
    } else {
        "latest".to_string()
    };
    validate_cache_path_segment("version", &version)?;

    let http_client = client.http_client();
    validate_release_endpoint_base_url(&http_client.base_url())?;

    let path = format!("/releases/{}/{}/asset", release_channel.dev_name(), version);
    let url = http_client.build_zed_cloud_url_with_query(&path, AssetQuery { os, arch, asset })?;

    let mut response = http_client
        .get(url.as_str(), Default::default(), false)
        .await?;
    let mut body = Vec::new();
    response
        .body_mut()
        .take((MAX_SHA256_SIDECAR_BYTES * 4) as u64)
        .read_to_end(&mut body)
        .await?;

    ensure!(
        response.status().is_success(),
        "failed to fetch release: {:?}",
        String::from_utf8_lossy(&body),
    );

    let release: ReleaseAsset = serde_json::from_slice(body.as_slice()).with_context(|| {
        format!(
            "error deserializing release {:?}",
            String::from_utf8_lossy(&body),
        )
    })?;
    validate_cache_path_segment("release.version", &release.version)?;
    validate_release_asset_url(&release.url)?;
    Ok(release)
}

fn validate_cache_path_segment(label: &str, segment: &str) -> Result<()> {
    ensure!(
        !segment.is_empty() && segment.len() <= 128,
        "invalid {label} segment length"
    );
    ensure!(
        segment != "." && segment != ".." && !segment.starts_with('-'),
        "invalid {label} segment {segment:?}"
    );
    ensure!(
        segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-')),
        "invalid characters in {label} segment {segment:?}"
    );
    Ok(())
}

fn build_version_cache_path(
    servers_dir: &Path,
    release_channel: ReleaseChannel,
    os: &str,
    arch: &str,
    release_version: &str,
) -> Result<(PathBuf, PathBuf)> {
    validate_cache_path_segment("os", os)?;
    validate_cache_path_segment("arch", arch)?;
    validate_cache_path_segment("release.version", release_version)?;

    let channel_dir = servers_dir.join(release_channel.dev_name());
    let platform_dir = channel_dir.join(format!("{os}-{arch}"));
    let version_path = platform_dir.join(format!("{release_version}.gz"));
    ensure!(
        version_path.parent() == Some(platform_dir.as_path())
            && platform_dir.parent() == Some(channel_dir.as_path())
            && version_path.starts_with(servers_dir),
        "remote server cache path escaped base directory"
    );
    Ok((platform_dir, version_path))
}

fn validate_release_endpoint_base_url(base_url: &str) -> Result<Url> {
    let parsed =
        Url::parse(base_url).with_context(|| format!("invalid release server URL {base_url:?}"))?;
    ensure!(
        parsed.scheme() == "https",
        "release server URL must use https://, got {}",
        parsed.scheme()
    );
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "release server URL must not embed credentials"
    );
    let host = parsed
        .host_str()
        .context("release server URL is missing host")?
        .to_ascii_lowercase();
    if host == "zed.dev" || host.ends_with(".zed.dev") {
        bail!(
            "zed-kask refuses upstream Zed Cloud release server {host:?} (D7 source-built artifact boundary)"
        );
    }
    Ok(parsed)
}

fn validate_release_asset_url(raw_url: &str) -> Result<Url> {
    let parsed =
        Url::parse(raw_url).with_context(|| format!("invalid release asset URL {raw_url:?}"))?;
    ensure!(
        parsed.scheme() == "https",
        "release asset URL must use https://, got {}",
        parsed.scheme()
    );
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none() && parsed.fragment().is_none(),
        "release asset URL must not contain credentials or fragments"
    );
    let host = parsed
        .host_str()
        .context("release asset URL is missing host")?
        .to_ascii_lowercase();
    ensure!(
        ALLOWED_RELEASE_ASSET_HOSTS.contains(&host.as_str()),
        "release asset host {host:?} is not an allowed official GitHub release host"
    );
    Ok(parsed)
}

async fn fetch_verified_https_bytes(
    client: &HttpClientWithUrl,
    initial_url: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let mut current = validate_release_asset_url(initial_url)?;
    for _hop in 0..=MAX_RELEASE_REDIRECTS {
        let mut response = client
            .get(current.as_str(), Default::default(), false)
            .await?;
        let status = response.status();
        if matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            let location = response
                .headers()
                .get(http_client::http::header::LOCATION)
                .context("redirect response missing Location header")?
                .to_str()
                .context("redirect Location header is not valid ASCII")?;
            let next = current
                .join(location)
                .with_context(|| format!("invalid redirect Location {location:?}"))?;
            current = validate_release_asset_url(next.as_str())?;
            continue;
        }

        ensure!(
            status.is_success(),
            "failed to download remote server release from {current}: {status:?}"
        );
        let mut body = Vec::new();
        response
            .body_mut()
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut body)
            .await?;
        ensure!(
            body.len() <= max_bytes,
            "downloaded remote server artifact exceeded {max_bytes} byte limit"
        );
        return Ok(body);
    }
    bail!("exceeded maximum redirect hops ({MAX_RELEASE_REDIRECTS}) while downloading release")
}

fn parse_sha256_digest(text: &str) -> Result<String> {
    let token = text
        .split_whitespace()
        .next()
        .context("empty SHA-256 digest")?;
    ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid SHA-256 digest {token:?}: expected 64 ASCII hex characters"
    );
    Ok(token.to_ascii_lowercase())
}

async fn resolve_expected_sha256(
    release: &ReleaseAsset,
    client: &HttpClientWithUrl,
) -> Result<String> {
    if let Some(digest) = release.sha256.as_deref() {
        return parse_sha256_digest(digest);
    }
    let sidecar_url = format!("{}.sha256", release.url);
    let sidecar_bytes = fetch_verified_https_bytes(client, &sidecar_url, MAX_SHA256_SIDECAR_BYTES)
        .await
        .context("release asset is missing required SHA-256 digest and .sha256 sidecar")?;
    let sidecar_text =
        std::str::from_utf8(&sidecar_bytes).context(".sha256 sidecar is not valid UTF-8")?;
    parse_sha256_digest(sidecar_text)
}

async fn download_remote_server_binary(
    target_path: &PathBuf,
    release: ReleaseAsset,
    client: Arc<HttpClientWithUrl>,
) -> Result<()> {
    validate_cache_path_segment("release.version", &release.version)?;
    validate_release_asset_url(&release.url)?;
    let expected_sha256 = resolve_expected_sha256(&release, &client).await?;
    let archive_bytes =
        fetch_verified_https_bytes(&client, &release.url, MAX_REMOTE_SERVER_ARCHIVE_BYTES).await?;
    ensure!(
        archive_bytes.len() > GZIP_MAGIC.len() && archive_bytes.starts_with(&GZIP_MAGIC),
        "downloaded remote server archive is empty or missing gzip header"
    );
    let actual_sha256 = sha256_hex(&archive_bytes);
    ensure!(
        actual_sha256 == expected_sha256,
        "remote server archive SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
    );

    let staging_dir = target_path
        .parent()
        .context("remote server target path has no parent directory")?;
    sfs::create_dir_all(staging_dir)
        .await
        .with_context(|| format!("creating staging directory {:?}", staging_dir))?;
    let temp = tempfile::Builder::new()
        .prefix(".zed-remote-server-")
        .tempfile_in(staging_dir)?;
    let mut temp_file = sfs::File::create(temp.path()).await?;
    temp_file.write_all(&archive_bytes).await?;
    temp_file.flush().await?;
    temp_file.sync_all().await?;
    drop(temp_file);
    sfs::rename(temp.path(), target_path).await?;

    Ok(())
}

/// FIPS 180-4 SHA-256 digest encoded as 64 lowercase hexadecimal characters.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64) * 8;
    let rem = (data.len() + 9) % 64;
    let pad_zeros = if rem == 0 { 0 } else { 64 - rem };
    let mut padded = Vec::with_capacity(data.len() + 9 + pad_zeros);
    padded.extend_from_slice(data);
    padded.push(0x80);
    padded.resize(padded.len() + pad_zeros, 0);
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (slot, delta) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(delta);
        }
    }

    let mut hex = String::with_capacity(64);
    for word in state {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{word:08x}");
    }
    hex
}

async fn cleanup_remote_server_cache(
    platform_dir: &Path,
    keep_path: &Path,
    limit: usize,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let mut entries = sfs::read_dir(platform_dir).await?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("gz")) {
            continue;
        }

        let mtime = if path == keep_path {
            now
        } else {
            sfs::metadata(&path)
                .await
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };

        candidates.push((path, mtime));
    }

    if candidates.len() <= limit {
        return Ok(());
    }

    candidates.sort_by(|(path_a, time_a), (path_b, time_b)| {
        time_b.cmp(time_a).then_with(|| path_a.cmp(path_b))
    });

    for (index, (path, _)) in candidates.into_iter().enumerate() {
        if index < limit || path == keep_path {
            continue;
        }

        if let Err(error) = sfs::remove_file(&path).await {
            log::warn!(
                "Failed to remove old remote server archive {:?}: {}",
                path,
                error
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_client::{AsyncBody, Response, StatusCode, Url, http};
    use std::collections::HashMap;

    struct StubHttpClient {
        responses: HashMap<String, (StatusCode, Option<String>, Vec<u8>)>,
    }

    impl HttpClient for StubHttpClient {
        fn user_agent(&self) -> Option<&http::HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            req: http::Request<AsyncBody>,
        ) -> smol::future::Boxed<anyhow::Result<Response<AsyncBody>>> {
            let url = req.uri().to_string();
            let entry = self.responses.get(&url).cloned();
            Box::pin(async move {
                let (status, location, body) =
                    entry.ok_or_else(|| anyhow::anyhow!("unexpected URL {url}"))?;
                let mut builder = Response::builder().status(status);
                if let Some(loc) = location {
                    builder = builder.header(http::header::LOCATION, loc);
                }
                Ok(builder.body(AsyncBody::from(body))?)
            })
        }
    }

    #[test]
    fn rejects_path_traversal_and_invalid_segments() {
        let base = Path::new("/tmp/remote_servers");
        for bad_version in [
            "",
            ".",
            "..",
            "../../../../tmp/overwrite",
            "0.1.0/../../evil",
            "0.1.0\\..\\evil",
            "-flag",
            "0.1.0\0bad",
            "0.1.0 space",
        ] {
            assert!(
                build_version_cache_path(
                    base,
                    ReleaseChannel::Stable,
                    "linux",
                    "x86_64",
                    bad_version
                )
                .is_err(),
                "expected bad version {bad_version:?} to be rejected"
            );
        }
        assert!(
            build_version_cache_path(base, ReleaseChannel::Stable, "../linux", "x86_64", "0.1.0")
                .is_err()
        );
        assert!(
            build_version_cache_path(base, ReleaseChannel::Stable, "linux", "x86_64/..", "0.1.0")
                .is_err()
        );
    }

    #[test]
    fn download_refuses_http_untrusted_host_empty_and_unverified_payloads() {
        smol::block_on(async {
            let temp_dir = tempfile::tempdir().unwrap();
            let target = temp_dir.path().join("0.1.0.gz");

            let mut responses = HashMap::new();
            responses.insert(
                "http://github.com/org/repo/releases/download/v0.1.0/zed-remote-server.gz".into(),
                (StatusCode::OK, None, vec![0x1f, 0x8b, 0x08, 0x00]),
            );
            responses.insert(
                "https://evil.example/zed-remote-server.gz".into(),
                (StatusCode::OK, None, vec![0x1f, 0x8b, 0x08, 0x00]),
            );
            responses.insert(
                "https://github.com/org/repo/releases/download/v0.1.0/unverified.gz".into(),
                (StatusCode::OK, None, vec![0x1f, 0x8b, 0x08, 0x00]),
            );
            let client = Arc::new(HttpClientWithUrl::new(
                Arc::new(StubHttpClient { responses }),
                "https://github.com",
                None,
            ));

            assert!(
                download_remote_server_binary(
                    &target,
                    ReleaseAsset {
                        version: "0.1.0".into(),
                        url: "http://github.com/org/repo/releases/download/v0.1.0/zed-remote-server.gz".into(),
                        sha256: None,
                    },
                    client.clone(),
                )
                .await
                .is_err(),
                "plain HTTP release URL must be rejected"
            );

            assert!(
                download_remote_server_binary(
                    &target,
                    ReleaseAsset {
                        version: "0.1.0".into(),
                        url: "https://evil.example/zed-remote-server.gz".into(),
                        sha256: None,
                    },
                    client.clone(),
                )
                .await
                .is_err(),
                "non-GitHub host must be rejected"
            );

            let target_unverified = temp_dir.path().join("unverified.gz");
            assert!(
                download_remote_server_binary(
                    &target_unverified,
                    ReleaseAsset {
                        version: "0.1.0".into(),
                        url: "https://github.com/org/repo/releases/download/v0.1.0/unverified.gz"
                            .into(),
                        sha256: None,
                    },
                    client,
                )
                .await
                .is_err(),
                "unverified binary without SHA-256 must be rejected"
            );
        });
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn validates_release_urls_redirects_upstream_refusal_and_sha256_integrity() {
        assert!(validate_release_endpoint_base_url("https://zed.dev").is_err());
        assert!(validate_release_endpoint_base_url("https://cloud.zed.dev").is_err());
        assert!(validate_release_endpoint_base_url("http://releases.example.com").is_err());
        assert!(validate_release_endpoint_base_url("https://releases.example.com").is_ok());

        smol::block_on(async {
            let temp_dir = tempfile::tempdir().unwrap();
            let valid_archive = vec![0x1f, 0x8b, 0x08, 0x00, 0xaa, 0xbb, 0xcc, 0xdd];
            let valid_digest = sha256_hex(&valid_archive);

            let asset_url =
                "https://github.com/org/repo/releases/download/v0.1.0/zed-remote-server.gz";
            let cdn_url = "https://objects.githubusercontent.com/github-production-release-asset/zed-remote-server.gz";
            let sidecar_url = format!("{asset_url}.sha256");

            let mut responses = HashMap::new();
            responses.insert(
                asset_url.into(),
                (StatusCode::FOUND, Some(cdn_url.into()), Vec::new()),
            );
            responses.insert(
                cdn_url.into(),
                (StatusCode::OK, None, valid_archive.clone()),
            );
            responses.insert(
                sidecar_url,
                (
                    StatusCode::OK,
                    None,
                    format!("{valid_digest}  zed-remote-server.gz\n").into_bytes(),
                ),
            );
            let evil_redirect_url =
                "https://github.com/org/repo/releases/download/v0.1.0/redirect-evil.gz";
            responses.insert(
                evil_redirect_url.into(),
                (
                    StatusCode::FOUND,
                    Some("https://evil.example/payload.gz".into()),
                    Vec::new(),
                ),
            );
            responses.insert(
                format!("{evil_redirect_url}.sha256"),
                (StatusCode::OK, None, valid_digest.clone().into_bytes()),
            );

            let client = Arc::new(HttpClientWithUrl::new(
                Arc::new(StubHttpClient { responses }),
                "https://releases.example.com",
                None,
            ));

            let target_ok = temp_dir.path().join("0.1.0.gz");
            download_remote_server_binary(
                &target_ok,
                ReleaseAsset {
                    version: "0.1.0".into(),
                    url: asset_url.into(),
                    sha256: None,
                },
                client.clone(),
            )
            .await
            .expect("verified GitHub release asset should succeed");
            assert_eq!(std::fs::read(&target_ok).unwrap(), valid_archive);

            let target_bad_hash = temp_dir.path().join("tampered.gz");
            assert!(
                download_remote_server_binary(
                    &target_bad_hash,
                    ReleaseAsset {
                        version: "0.1.0".into(),
                        url: asset_url.into(),
                        sha256: Some("0".repeat(64)),
                    },
                    client.clone(),
                )
                .await
                .is_err()
            );
            assert!(!target_bad_hash.exists());

            let target_evil_redirect = temp_dir.path().join("evil_redirect.gz");
            assert!(
                download_remote_server_binary(
                    &target_evil_redirect,
                    ReleaseAsset {
                        version: "0.1.0".into(),
                        url: evil_redirect_url.into(),
                        sha256: Some(valid_digest),
                    },
                    client,
                )
                .await
                .is_err()
            );
            assert!(!target_evil_redirect.exists());
        });
    }
}
