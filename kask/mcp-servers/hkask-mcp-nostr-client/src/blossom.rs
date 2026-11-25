//! Blossom media upload (`BUD-02` `PUT /upload` + `BUD-11` `kind: 24242`
//! Nostr authorization) and local media preparation for NIP-92 `imeta` posts.
//!
//! This module holds no secret key and never signs: the editor signs the
//! short-lived `kind: 24242` token after operator approval (or directly when
//! `kask.nostr.autopublish` is on), and this module sends the file bytes only
//! to Blossom servers configured by the operator in `kask.nostr.blossom_servers`.

use std::io::Read as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use hkask_mcp_server::server::McpToolError;
use hkask_types::nostr_wire::{
    BlossomBlobSummary, LocalBlossomAnnouncement, MAX_BLOSSOM_UPLOAD_BYTES, MAX_MEDIA_ATTACHMENTS,
    NostrMediaAttachment, truncate_chars,
};
use nostr_sdk::prelude::Event;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::read::MAX_REASON_CHARS;
use crate::relays::{AddressPolicy, domain_is_public, ipv4_is_public, ipv6_is_public};

/// Maximum Blossom servers contacted per upload.
pub const MAX_BLOSSOM_SERVERS: usize = 4;

/// Per-upload HTTP timeout (120 seconds covers tens of megabytes on ordinary uplinks).
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Maximum bytes read from a Blossom JSON `BlobDescriptor` or error response.
const MAX_DESCRIPTOR_RESPONSE_BYTES: u64 = 64 * 1024;

/// Maximum bytes read from a local Blossom `/list/<pubkey>` JSON array response.
const MAX_LIST_RESPONSE_BYTES: u64 = 1024 * 1024;

/// Construct a hardened `reqwest::Client` with redirects disabled, ambient proxies
/// ignored, connect-time DNS validation, and `timeout`.
pub fn build_blossom_http_client(
    timeout: Duration,
    policy: AddressPolicy,
) -> Result<reqwest::Client, McpToolError> {
    let allow_loopback = policy == AddressPolicy::LoopbackAllowed;
    hkask_mcp_server::http_guard::build_ssrf_safe_client(timeout, allow_loopback)
        .map_err(|err| McpToolError::unavailable(format!("HTTP client init failed: {err}")))
}

pub(crate) async fn read_response_bytes_bounded(
    mut resp: reqwest::Response,
    max_bytes: u64,
) -> Result<Vec<u8>, BlobDownloadError> {
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(BlobDownloadError::TooLarge { size: len });
    }
    let cap = resp
        .content_length()
        .map(|n| n.min(max_bytes) as usize)
        .unwrap_or(0);
    let mut buf = Vec::with_capacity(cap.min(64 * 1024));
    while let Some(chunk) = resp.chunk().await.map_err(BlobDownloadError::ReadBody)? {
        let next_len = (buf.len() as u64).saturating_add(chunk.len() as u64);
        if next_len > max_bytes {
            return Err(BlobDownloadError::TooLarge { size: next_len });
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Validated Blossom server endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlossomTarget {
    /// Normalized server root shown to the operator (e.g. `https://blossom.primal.net`).
    pub base_url: String,
    /// `BUD-02` `PUT /upload` endpoint (`https://blossom.primal.net/upload`).
    pub upload_url: String,
    /// Lowercase `host` (or `host:port` when non-default) for the `BUD-11` `server` tag.
    pub server_domain: String,
}

/// Why a Blossom server URL is refused. No variant repeats the input text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlossomAddressError {
    Unparseable,
    NotHttps,
    NotPublic,
    HasCredentials,
    HasQueryOrFragment,
}

impl std::fmt::Display for BlossomAddressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unparseable => "not a valid HTTP(S) Blossom server URL",
            Self::NotHttps => "only https:// Blossom servers are contacted",
            Self::NotPublic => "the host is this machine or a private network",
            Self::HasCredentials => "the URL carries a user name or password",
            Self::HasQueryOrFragment => "a Blossom server URL cannot carry a query or fragment",
        })
    }
}

fn url_host_is_public(url: &reqwest::Url) -> bool {
    let Some(host_str) = url.host_str() else {
        return false;
    };
    let trimmed = host_str.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => ipv4_is_public(v4),
            IpAddr::V6(v6) => ipv6_is_public(v6),
        };
    }
    if let Ok(v4) = host_str.parse::<Ipv4Addr>() {
        return ipv4_is_public(v4);
    }
    if let Ok(v6) = trimmed.parse::<Ipv6Addr>() {
        return ipv6_is_public(v6);
    }
    domain_is_public(host_str)
}

fn url_host_is_loopback(url: &reqwest::Url) -> bool {
    let Some(host_str) = url.host_str() else {
        return false;
    };
    let trimmed = host_str.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_loopback(),
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
            }
        };
    }
    let lower = host_str.trim_end_matches('.').to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".localhost")
}

/// Validate one Blossom server URL against `policy`.
pub fn validate_blossom_server(
    raw: &str,
    policy: AddressPolicy,
) -> Result<BlossomTarget, BlossomAddressError> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| BlossomAddressError::Unparseable)?;
    if url.host_str().is_none() {
        return Err(BlossomAddressError::Unparseable);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(BlossomAddressError::HasCredentials);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(BlossomAddressError::HasQueryOrFragment);
    }
    if policy == AddressPolicy::LoopbackAllowed && url_host_is_loopback(&url) {
        if url.scheme() != "https" && url.scheme() != "http" {
            return Err(BlossomAddressError::Unparseable);
        }
        if url.port_or_known_default().unwrap_or(0) == 0 {
            return Err(BlossomAddressError::Unparseable);
        }
    } else {
        if url.scheme() != "https" {
            return Err(BlossomAddressError::NotHttps);
        }
        if !url_host_is_public(&url) {
            return Err(BlossomAddressError::NotPublic);
        }
    }

    let host = url
        .host_str()
        .ok_or(BlossomAddressError::Unparseable)?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let server_domain = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    };

    let clean_path = url.path().trim_end_matches('/');
    let origin = url.origin().ascii_serialization();
    let base_url = if clean_path.is_empty() {
        origin
    } else {
        format!("{origin}{clean_path}")
    };
    let upload_url = format!("{base_url}/upload");

    Ok(BlossomTarget {
        base_url,
        upload_url,
        server_domain,
    })
}

/// Validate a blob URL returned in a Blossom server's `BlobDescriptor`.
pub fn validate_returned_blob_url(
    raw: &str,
    policy: AddressPolicy,
) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("returned blob URL is empty or contains whitespace");
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| "returned blob URL is not a valid URL")?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("returned blob URL carries credentials");
    }
    if policy == AddressPolicy::LoopbackAllowed && url_host_is_loopback(&url) {
        if url.scheme() != "https" && url.scheme() != "http" {
            return Err("returned blob URL is not http(s)://");
        }
        if url.port_or_known_default().unwrap_or(0) == 0 {
            return Err("returned blob URL has an invalid port");
        }
    } else {
        if url.scheme() != "https" {
            return Err("returned blob URL is not https://");
        }
        if !url_host_is_public(&url) {
            return Err("returned blob URL points to a non-public host");
        }
    }
    Ok(url.to_string())
}

/// Report configured Blossom servers in `nostr_account` without contacting them.
pub fn report_blossom_servers(entries: &[String], policy: AddressPolicy) -> (Vec<Value>, usize) {
    let considered = &entries[..entries.len().min(MAX_BLOSSOM_SERVERS)];
    let skipped = entries.len() - considered.len();
    let reports = considered
        .iter()
        .enumerate()
        .map(|(idx, raw)| match validate_blossom_server(raw, policy) {
            Ok(target) => json!({
                "address": target.base_url,
                "state": "configured",
            }),
            Err(error) => json!({
                "position": idx + 1,
                "state": "refused",
                "reason": error.to_string(),
            }),
        })
        .collect();
    (reports, skipped)
}

/// Resolve usable Blossom targets for an upload.
pub fn resolve_blossom_targets(
    entries: &[String],
    policy: AddressPolicy,
) -> Result<Vec<BlossomTarget>, McpToolError> {
    let mut usable = Vec::new();
    let mut refused_reasons = Vec::new();
    for (idx, raw) in entries.iter().take(MAX_BLOSSOM_SERVERS).enumerate() {
        match validate_blossom_server(raw, policy) {
            Ok(target) => {
                if !usable
                    .iter()
                    .any(|seen: &BlossomTarget| seen.upload_url == target.upload_url)
                {
                    usable.push(target);
                }
            }
            Err(err) => refused_reasons.push(format!("#{} ({err})", idx + 1)),
        }
    }
    if usable.is_empty() {
        if entries.is_empty() {
            return Err(McpToolError::failed_precondition(
                "no Blossom media server is configured; set kask.nostr.blossom_servers in \
                 Settings > Kask > Nostr (for example https://blossom.primal.net) before \
                 publishing media_paths",
            ));
        }
        return Err(McpToolError::failed_precondition(format!(
            "no configured Blossom server is usable ({}); set an https:// public server in \
             kask.nostr.blossom_servers",
            refused_reasons.join(", ")
        )));
    }
    Ok(usable)
}

/// One local media file read, hashed and inspected before upload.
#[derive(Debug, Clone)]
pub struct PreparedMedia {
    pub path: String,
    pub mime_type: String,
    pub sha256: String,
    pub size: u64,
    pub dimensions: Option<(u32, u32)>,
    pub bytes: Vec<u8>,
}

impl PreparedMedia {
    pub fn summary(&self) -> BlossomBlobSummary {
        BlossomBlobSummary {
            path: self.path.clone(),
            mime_type: self.mime_type.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
        }
    }
}

fn detect_media_mime(bytes: &[u8]) -> Option<&'static str> {
    // Require verified magic-byte signatures so renamed non-media files cannot be uploaded.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"qt  " {
            return Some("video/quicktime");
        }
        if brand == b"M4A " {
            return Some("audio/mp4");
        }
        return Some("video/mp4");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some("audio/wav");
    }
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0)
    {
        return Some("audio/mpeg");
    }
    None
}

fn path_touches_sensitive_location(path: &Path) -> bool {
    const BLOCKED_ROOTS: &[&str] = &["/etc", "/proc", "/sys", "/dev", "/boot", "/root"];
    for root in BLOCKED_ROOTS {
        if path.starts_with(root) {
            return true;
        }
    }
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        let lower_name = file_name.to_ascii_lowercase();
        if lower_name.starts_with('.')
            || lower_name.starts_with('-')
            || matches!(
                lower_name.as_str(),
                "id_rsa"
                    | "id_ed25519"
                    | "id_ecdsa"
                    | "id_dsa"
                    | "authorized_keys"
                    | "known_hosts"
                    | "credentials"
                    | "cookies.sqlite"
            )
        {
            return true;
        }
    }
    const SENSITIVE_COMPONENTS: &[&str] = &[
        ".ssh",
        ".gnupg",
        ".aws",
        ".azure",
        ".gcloud",
        ".kube",
        ".docker",
        ".config",
        ".local",
        ".mozilla",
        ".pki",
        ".password-store",
        ".git",
        ".hg",
        ".svn",
        ".cargo",
        ".rustup",
        ".npm",
        ".env",
        ".netrc",
        ".npmrc",
        ".git-credentials",
    ];
    for component in path.components() {
        if let std::path::Component::Normal(os) = component
            && let Some(seg) = os.to_str()
        {
            let lower = seg.to_ascii_lowercase();
            if SENSITIVE_COMPONENTS.contains(&lower.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Best-effort probe of `(width, height)` via `ffprobe` for NIP-92 `dim WxH`,
/// bounded to 5 seconds so a malformed container never stalls the runtime.
async fn probe_dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut cmd = tokio::process::Command::new("ffprobe");
    cmd.env_clear();
    if let Some(sys_path) = std::env::var_os("PATH") {
        cmd.env("PATH", sys_path);
    }
    let child = cmd
        .stdin(std::process::Stdio::null())
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
            "--",
        ])
        .arg(path)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(5), child)
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.lines().find(|line| !line.trim().is_empty())?.trim();
    let (w, h) = line.split_once('x')?;
    let width: u32 = w.trim().parse().ok()?;
    let height: u32 = h.trim().parse().ok()?;
    if width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

fn read_verified_local_file_blocking(
    raw_trimmed: &str,
) -> Result<(Vec<u8>, std::path::PathBuf), McpToolError> {
    let path = Path::new(raw_trimmed);
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(McpToolError::invalid_argument(format!(
            "media path must not contain parent-directory traversal: {raw_trimmed}"
        )));
    }
    if path_touches_sensitive_location(path) {
        return Err(McpToolError::invalid_argument(format!(
            "media path targets a restricted dotfile, config, or system path: {raw_trimmed}"
        )));
    }
    let lmeta = std::fs::symlink_metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            McpToolError::not_found(format!("media file does not exist: {raw_trimmed}"))
        }
        _ => McpToolError::invalid_argument(format!(
            "media file cannot be read ({raw_trimmed}): {err}"
        )),
    })?;
    if lmeta.file_type().is_symlink() {
        return Err(McpToolError::invalid_argument(format!(
            "media path must not be a symlink: {raw_trimmed}"
        )));
    }
    if !lmeta.is_file() {
        return Err(McpToolError::invalid_argument(format!(
            "media path is not a regular file: {raw_trimmed}"
        )));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Ok(pmeta) = std::fs::symlink_metadata(parent)
        && pmeta.file_type().is_symlink()
    {
        return Err(McpToolError::invalid_argument(format!(
            "media path parent directory must not be a symlink: {raw_trimmed}"
        )));
    }

    let canonical = std::fs::canonicalize(path).map_err(|err| {
        McpToolError::invalid_argument(format!(
            "media path could not be canonicalized ({raw_trimmed}): {err}"
        ))
    })?;
    if path_touches_sensitive_location(&canonical) {
        return Err(McpToolError::invalid_argument(format!(
            "media path resolves to a restricted dotfile, config, or system path: {raw_trimmed}"
        )));
    }

    let mut open_opts = std::fs::OpenOptions::new();
    open_opts.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // O_NOFOLLOW on Linux = 0o400000 (0x20000).
        open_opts.custom_flags(0o400000);
    }
    let file = open_opts.open(path).map_err(|err| {
        McpToolError::invalid_argument(format!("failed to open media file {raw_trimmed}: {err}"))
    })?;
    let fmeta = file.metadata().map_err(|err| {
        McpToolError::invalid_argument(format!(
            "failed to inspect open media file {raw_trimmed}: {err}"
        ))
    })?;
    if !fmeta.is_file() {
        return Err(McpToolError::invalid_argument(format!(
            "media path is not a regular file: {raw_trimmed}"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if (lmeta.dev(), lmeta.ino()) != (fmeta.dev(), fmeta.ino()) {
            return Err(McpToolError::invalid_argument(format!(
                "media path changed during open (possible symlink swap): {raw_trimmed}"
            )));
        }
    }

    let size_hint = fmeta.len();
    if size_hint == 0 {
        return Err(McpToolError::invalid_argument(format!(
            "media file is empty: {raw_trimmed}"
        )));
    }
    if size_hint > MAX_BLOSSOM_UPLOAD_BYTES {
        return Err(McpToolError::invalid_argument(format!(
            "media file {raw_trimmed} is {size_hint} bytes, over the maximum of {MAX_BLOSSOM_UPLOAD_BYTES} bytes"
        )));
    }

    let mut bytes = Vec::with_capacity((size_hint as usize).min(1024 * 1024));
    file.take(MAX_BLOSSOM_UPLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            McpToolError::invalid_argument(format!(
                "failed to read media file {raw_trimmed}: {err}"
            ))
        })?;
    if bytes.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "media file is empty: {raw_trimmed}"
        )));
    }
    if bytes.len() as u64 > MAX_BLOSSOM_UPLOAD_BYTES {
        return Err(McpToolError::invalid_argument(format!(
            "media file {raw_trimmed} exceeds the maximum of {MAX_BLOSSOM_UPLOAD_BYTES} bytes"
        )));
    }
    Ok((bytes, canonical))
}

/// Validate, read, hash and inspect local media files before any network call.
pub async fn prepare_local_media(raw_paths: &[String]) -> Result<Vec<PreparedMedia>, McpToolError> {
    if raw_paths.len() > MAX_MEDIA_ATTACHMENTS {
        return Err(McpToolError::invalid_argument(format!(
            "media_paths: at most {MAX_MEDIA_ATTACHMENTS} files may be attached to one note"
        )));
    }
    let mut prepared = Vec::with_capacity(raw_paths.len());
    for raw in raw_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(McpToolError::invalid_argument(
                "media_paths: file path must not be empty",
            ));
        }
        if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
            return Err(McpToolError::invalid_argument(
                "media_paths: file path contains a NUL or control character",
            ));
        }
        let owned_trimmed = trimmed.to_string();
        let (bytes, canonical) =
            tokio::task::spawn_blocking(move || read_verified_local_file_blocking(&owned_trimmed))
                .await
                .map_err(|err| {
                    McpToolError::internal(format!("local media read task failed: {err}"))
                })??;
        let actual_size = bytes.len() as u64;
        let Some(mime_type) = detect_media_mime(&bytes) else {
            return Err(McpToolError::invalid_argument(format!(
                "unsupported media format for {trimmed}; supported formats are mp4, mov, webm, jpg, png, gif, webp, mp3, wav, ogg, m4a (verified by magic bytes)"
            )));
        };
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let dimensions = if mime_type.starts_with("video/") || mime_type.starts_with("image/") {
            probe_dimensions(&canonical).await
        } else {
            None
        };
        prepared.push(PreparedMedia {
            path: trimmed.to_string(),
            mime_type: mime_type.to_string(),
            sha256,
            size: actual_size,
            dimensions,
            bytes,
        });
    }
    Ok(prepared)
}

/// Verify the editor's signed `kind: 24242` event and encode it as a `BUD-11`
/// Encode a verified `kind: 24242` signed event JSON into a BUD-01 / BUD-11
/// `Authorization: Nostr <base64>` header value using standard padded Base64
/// (RFC 4648 §4, required by deployed Blossom servers such as `blossom.primal.net`).
pub fn encode_blossom_auth_header(signed_event: &Value) -> Result<String, McpToolError> {
    let compact = serde_json::to_string(signed_event).map_err(|err| {
        McpToolError::unavailable(format!("Blossom auth event did not serialize: {err}"))
    })?;
    let verified = hkask_nostr_core::Event::from_json(&compact).map_err(|err| {
        McpToolError::unavailable(format!("Blossom auth event failed verification: {err}"))
    })?;
    if verified.kind() != 24_242 {
        return Err(McpToolError::unavailable(format!(
            "Blossom auth event has unexpected kind {}",
            verified.kind()
        )));
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(verified.to_json());
    Ok(format!("Nostr {encoded}"))
}

fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut current = err.source();
    while let Some(cause) = current {
        parts.push(cause.to_string());
        current = cause.source();
    }
    parts.join(": ")
}

#[derive(Debug, Deserialize)]
struct BlobDescriptor {
    url: String,
    sha256: String,
}

/// Upload every prepared file to the configured Blossom targets (`BUD-02`
/// `PUT /upload`), verify the returned `sha256` and URL, and build the
/// `NIP-92` `imeta` attachments plus per-file upload reports.
pub async fn upload_prepared_media(
    files: &[PreparedMedia],
    targets: &[BlossomTarget],
    auth_header: &str,
    policy: AddressPolicy,
) -> Result<(Vec<NostrMediaAttachment>, Vec<Value>), McpToolError> {
    let client = build_blossom_http_client(UPLOAD_TIMEOUT, policy)?;

    let mut attachments = Vec::with_capacity(files.len());
    let mut reports = Vec::with_capacity(files.len());

    for file in files {
        let mut accepted_urls: Vec<String> = Vec::new();
        let mut server_outcomes: Vec<Value> = Vec::new();

        for target in targets {
            if let Some(preflight_err) = preflight_upload(&client, target, file, auth_header).await
            {
                server_outcomes.push(preflight_err);
                continue;
            }
            let response = client
                .put(&target.upload_url)
                .header("Authorization", auth_header)
                .header("Content-Type", &file.mime_type)
                .header("Content-Length", file.size)
                .header("X-SHA-256", &file.sha256)
                .body(file.bytes.clone())
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let reason_header = resp
                        .headers()
                        .get("x-reason")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| truncate_chars(s, MAX_REASON_CHARS).0);
                    let body_text = match read_response_bytes_bounded(
                        resp,
                        MAX_DESCRIPTOR_RESPONSE_BYTES,
                    )
                    .await
                    {
                        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        Err(err) => {
                            server_outcomes.push(json!({
                                "server": target.base_url,
                                "state": "rejected_response",
                                "reason": truncate_chars(&err.to_string(), MAX_REASON_CHARS).0,
                            }));
                            continue;
                        }
                    };

                    if status == reqwest::StatusCode::OK || status == reqwest::StatusCode::CREATED {
                        match serde_json::from_str::<BlobDescriptor>(&body_text) {
                            Ok(descriptor) => {
                                if descriptor.sha256.to_ascii_lowercase() != file.sha256 {
                                    server_outcomes.push(json!({
                                        "server": target.base_url,
                                        "state": "rejected_response",
                                        "reason": "server returned a different sha256 than the uploaded file",
                                    }));
                                    continue;
                                }
                                match validate_returned_blob_url(&descriptor.url, policy) {
                                    Ok(clean_url) => {
                                        if !accepted_urls.contains(&clean_url) {
                                            accepted_urls.push(clean_url.clone());
                                        }
                                        server_outcomes.push(json!({
                                            "server": target.base_url,
                                            "state": "uploaded",
                                            "status": status.as_u16(),
                                            "url": clean_url,
                                        }));
                                    }
                                    Err(reason) => {
                                        server_outcomes.push(json!({
                                            "server": target.base_url,
                                            "reason": reason,
                                            "state": "rejected_response",
                                        }));
                                    }
                                }
                            }
                            Err(_) => {
                                server_outcomes.push(json!({
                                    "server": target.base_url,
                                    "state": "rejected_response",
                                    "reason": "server did not return a valid BUD-02 JSON blob descriptor",
                                }));
                            }
                        }
                    } else {
                        let detail = reason_header.unwrap_or_else(|| {
                            truncate_chars(body_text.trim(), MAX_REASON_CHARS).0
                        });
                        server_outcomes.push(json!({
                            "server": target.base_url,
                            "state": "refused",
                            "status": status.as_u16(),
                            "reason": detail,
                        }));
                    }
                }
                Err(error) => {
                    // HTTP/2 servers behind CDNs (e.g. Cloudflare) that reject
                    // headers before reading a multi-megabyte body send
                    // RST_STREAM while reqwest is still streaming bytes. Probe
                    // with an empty body to surface any 4xx/5xx status + reason.
                    let mut diagnosed = None;
                    if let Ok(probe) = client
                        .put(&target.upload_url)
                        .header("Authorization", auth_header)
                        .header("Content-Type", &file.mime_type)
                        .header("Content-Length", 0u64)
                        .header("X-SHA-256", &file.sha256)
                        .timeout(Duration::from_secs(10))
                        .body(Vec::<u8>::new())
                        .send()
                        .await
                    {
                        let p_status = probe.status();
                        if !p_status.is_success() {
                            let p_reason = probe
                                .headers()
                                .get("x-reason")
                                .and_then(|v| v.to_str().ok())
                                .map(|s| truncate_chars(s, MAX_REASON_CHARS).0);
                            let p_body =
                                read_response_bytes_bounded(probe, MAX_DESCRIPTOR_RESPONSE_BYTES)
                                    .await
                                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                                    .unwrap_or_default();
                            let detail = p_reason.unwrap_or_else(|| {
                                truncate_chars(p_body.trim(), MAX_REASON_CHARS).0
                            });
                            diagnosed = Some(json!({
                                "server": target.base_url,
                                "state": "refused",
                                "status": p_status.as_u16(),
                                "reason": detail,
                            }));
                        }
                    }
                    if let Some(outcome) = diagnosed {
                        server_outcomes.push(outcome);
                    } else {
                        let chain = format_error_chain(&error);
                        server_outcomes.push(json!({
                            "server": target.base_url,
                            "state": "unreachable",
                            "reason": truncate_chars(&chain, MAX_REASON_CHARS).0,
                        }));
                    }
                }
            }
        }

        if accepted_urls.is_empty() {
            let detail = server_outcomes
                .iter()
                .map(|o| {
                    format!(
                        "{} {} ({})",
                        o["server"].as_str().unwrap_or("server"),
                        o["state"].as_str().unwrap_or("failed"),
                        o["reason"].as_str().unwrap_or("no detail")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(McpToolError::unavailable(format!(
                "Blossom upload failed for {}: {detail}; the note was not signed or published",
                file.path
            )));
        }

        let primary_url = accepted_urls.remove(0);
        let fallback_urls = accepted_urls;
        reports.push(json!({
            "path": file.path,
            "sha256": file.sha256,
            "mime_type": file.mime_type,
            "size": file.size,
            "dimensions": file.dimensions.map(|(w, h)| format!("{w}x{h}")),
            "url": primary_url,
            "fallback_urls": fallback_urls,
            "servers": server_outcomes,
        }));
        attachments.push(NostrMediaAttachment {
            url: primary_url,
            mime_type: file.mime_type.clone(),
            sha256: file.sha256.clone(),
            size: file.size,
            dimensions: file.dimensions,
            fallback_urls,
        });
    }

    Ok((attachments, reports))
}

/// What the Blossom sidecar's announcement file yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalBlossomState {
    NotAnnounced,
    InvalidAnnouncement,
    Announced { base_url: String, addr: SocketAddr },
}

/// Read `<data_dir>/mcp/nostr-blossom/listener.json`, admitting only
/// `http://127.0.0.1:<port>` with a non-zero port.
pub fn read_local_blossom(path: Option<&Path>) -> LocalBlossomState {
    let Some(path) = path else {
        return LocalBlossomState::NotAnnounced;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return LocalBlossomState::NotAnnounced;
    };
    let Ok(announcement) = serde_json::from_str::<LocalBlossomAnnouncement>(&text) else {
        return LocalBlossomState::InvalidAnnouncement;
    };
    let Some(host_port) = announcement.address.strip_prefix("http://") else {
        return LocalBlossomState::InvalidAnnouncement;
    };
    let Ok(addr) = host_port.parse::<SocketAddr>() else {
        return LocalBlossomState::InvalidAnnouncement;
    };
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || addr.port() == 0 {
        return LocalBlossomState::InvalidAnnouncement;
    }
    LocalBlossomState::Announced {
        base_url: format!("http://{addr}"),
        addr,
    }
}

/// Probe the local Blossom sidecar with a short TCP connect for `nostr_account`.
pub async fn probe_local_blossom(local: &LocalBlossomState) -> Value {
    match local {
        LocalBlossomState::NotAnnounced => json!({
            "state": "not_announced",
        }),
        LocalBlossomState::InvalidAnnouncement => json!({
            "state": "invalid_announcement",
        }),
        LocalBlossomState::Announced { base_url, addr } => {
            let connect =
                tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr))
                    .await;
            match connect {
                Ok(Ok(_stream)) => json!({
                    "state": "connected",
                    "address": base_url,
                }),
                Ok(Err(err)) => json!({
                    "state": "unreachable",
                    "address": base_url,
                    "reason": err.to_string(),
                }),
                Err(_) => json!({
                    "state": "timed_out",
                    "address": base_url,
                }),
            }
        }
    }
}

/// Resolved upload plan for `nostr_publish_note`:
/// - `local_target`: optional local Blossom sidecar (always staged first when available)
/// - `public_targets`: configured `https://` Blossom servers
/// - `public_base_url`: optional validated `https://` URL fronting the local sidecar
/// - `has_public_relays`: when true, loopback URLs are NEVER admitted into signed `imeta`
#[derive(Debug, Clone)]
pub struct BlossomPlan {
    pub local_target: Option<BlossomTarget>,
    pub public_targets: Vec<BlossomTarget>,
    pub public_base_url: Option<BlossomTarget>,
    pub has_public_relays: bool,
}

impl BlossomPlan {
    /// All targets that need to be included in the BUD-11 `kind: 24242` `server` tags.
    pub fn all_auth_targets(&self) -> Vec<BlossomTarget> {
        let mut all = Vec::new();
        if let Some(ref local) = self.local_target {
            all.push(local.clone());
        }
        for pub_t in &self.public_targets {
            if !all.iter().any(|t| t.upload_url == pub_t.upload_url) {
                all.push(pub_t.clone());
            }
        }
        all
    }
}

/// Build the Blossom upload plan and enforce the local-only vs. public URL safety gate
/// BEFORE any note is signed.
pub fn resolve_blossom_plan(
    public_entries: &[String],
    public_base_url: Option<&str>,
    local_blossom: &LocalBlossomState,
    has_public_relays: bool,
    policy: AddressPolicy,
) -> Result<BlossomPlan, McpToolError> {
    let local_target = match local_blossom {
        LocalBlossomState::Announced { base_url, .. } => {
            validate_blossom_server(base_url, AddressPolicy::LoopbackAllowed).ok()
        }
        LocalBlossomState::NotAnnounced | LocalBlossomState::InvalidAnnouncement => None,
    };

    let public_base_target = match public_base_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => Some(validate_blossom_server(raw, policy).map_err(|err| {
            McpToolError::failed_precondition(format!(
                "kask.nostr.blossom.public_base_url is not usable ({err})"
            ))
        })?),
        None => None,
    };

    let mut public_targets = Vec::new();
    let mut refused_reasons = Vec::new();
    for (idx, raw) in public_entries.iter().take(MAX_BLOSSOM_SERVERS).enumerate() {
        match validate_blossom_server(raw, policy) {
            Ok(target) => {
                if !public_targets
                    .iter()
                    .any(|seen: &BlossomTarget| seen.upload_url == target.upload_url)
                {
                    public_targets.push(target);
                }
            }
            Err(err) => refused_reasons.push(format!("#{} ({err})", idx + 1)),
        }
    }

    if has_public_relays {
        let has_public_url_source =
            !public_targets.is_empty() || (local_target.is_some() && public_base_target.is_some());
        if !has_public_url_source {
            let extra = if refused_reasons.is_empty() {
                String::new()
            } else {
                format!(" ({})", refused_reasons.join(", "))
            };
            return Err(McpToolError::failed_precondition(format!(
                "cannot publish media_paths to public seed_relays without a public https:// Blossom \
                 server{extra}; local 127.0.0.1 URLs are never broadcast to public relays. Configure \
                 kask.nostr.blossom_servers or kask.nostr.blossom.public_base_url, or clear \
                 seed_relays for local-only staging"
            )));
        }
    } else if local_target.is_none() && public_targets.is_empty() {
        return Err(McpToolError::failed_precondition(
            "no Blossom media server is available: the local Blossom sidecar is not announced and \
             no server is configured in kask.nostr.blossom_servers",
        ));
    }

    Ok(BlossomPlan {
        local_target,
        public_targets,
        public_base_url: public_base_target,
        has_public_relays,
    })
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "video/mp4" => ".mp4",
        "video/quicktime" => ".mov",
        "video/webm" => ".webm",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "audio/mpeg" => ".mp3",
        "audio/wav" => ".wav",
        "audio/ogg" => ".ogg",
        "audio/mp4" => ".m4a",
        _ => "",
    }
}

/// BUD-06 `HEAD /upload` preflight check before transmitting blob bytes.
///
/// Returns `None` when the server accepts the preflight (`200`/`204`) or does
/// not implement BUD-06 (`404`/`405`/`501`, graceful fallback to `PUT /upload`).
/// Returns `Some(report)` to fail fast on quota/MIME/auth/size rejections
/// without uploading up to 100 MiB over the wire.
pub async fn preflight_upload(
    client: &reqwest::Client,
    target: &BlossomTarget,
    file: &PreparedMedia,
    auth_header: &str,
) -> Option<Value> {
    let response = client
        .head(&target.upload_url)
        .header("Authorization", auth_header)
        .header("X-SHA-256", &file.sha256)
        .header("X-Content-Length", file.size)
        .header("X-Content-Type", &file.mime_type)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status == reqwest::StatusCode::OK
                || status == reqwest::StatusCode::NO_CONTENT
                || status == reqwest::StatusCode::NOT_FOUND
                || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
                || status == reqwest::StatusCode::NOT_IMPLEMENTED
            {
                return None;
            }
            let detail = resp
                .headers()
                .get("x-reason")
                .and_then(|v| v.to_str().ok())
                .map(|s| truncate_chars(s, MAX_REASON_CHARS).0)
                .unwrap_or_else(|| status.to_string());
            Some(json!({
                "server": target.base_url,
                "state": "refused",
                "status": status.as_u16(),
                "reason": detail,
                "preflight": true,
            }))
        }
        Err(error) => {
            let chain = format_error_chain(&error);
            Some(json!({
                "server": target.base_url,
                "state": "unreachable",
                "reason": truncate_chars(&chain, MAX_REASON_CHARS).0,
                "preflight": true,
            }))
        }
    }
}

/// Upload a single prepared blob to one Blossom target (`BUD-06` preflight + `PUT /upload`).
pub async fn upload_single_blob(
    client: &reqwest::Client,
    target: &BlossomTarget,
    file: &PreparedMedia,
    auth_header: &str,
    policy: AddressPolicy,
) -> Value {
    if let Some(preflight_err) = preflight_upload(client, target, file, auth_header).await {
        return preflight_err;
    }
    let response = client
        .put(&target.upload_url)
        .header("Authorization", auth_header)
        .header("Content-Type", &file.mime_type)
        .header("Content-Length", file.size)
        .header("X-SHA-256", &file.sha256)
        .body(file.bytes.clone())
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let reason_header = resp
                .headers()
                .get("x-reason")
                .and_then(|v| v.to_str().ok())
                .map(|s| truncate_chars(s, MAX_REASON_CHARS).0);
            let body_text =
                match read_response_bytes_bounded(resp, MAX_DESCRIPTOR_RESPONSE_BYTES).await {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Err(err) => {
                        return json!({
                            "server": target.base_url,
                            "state": "rejected_response",
                            "reason": truncate_chars(&err.to_string(), MAX_REASON_CHARS).0,
                        });
                    }
                };

            if status == reqwest::StatusCode::OK || status == reqwest::StatusCode::CREATED {
                match serde_json::from_str::<BlobDescriptor>(&body_text) {
                    Ok(descriptor) => {
                        if descriptor.sha256.to_ascii_lowercase() != file.sha256 {
                            return json!({
                                "server": target.base_url,
                                "state": "rejected_response",
                                "reason": "server returned a different sha256 than the uploaded file",
                            });
                        }
                        match validate_returned_blob_url(&descriptor.url, policy) {
                            Ok(clean_url) => json!({
                                "server": target.base_url,
                                "state": "uploaded",
                                "status": status.as_u16(),
                                "url": clean_url,
                            }),
                            Err(reason) => json!({
                                "server": target.base_url,
                                "state": "rejected_response",
                                "reason": reason,
                            }),
                        }
                    }
                    Err(_) => json!({
                        "server": target.base_url,
                        "state": "rejected_response",
                        "reason": "server did not return a valid BUD-02 JSON blob descriptor",
                    }),
                }
            } else {
                let detail = reason_header
                    .unwrap_or_else(|| truncate_chars(body_text.trim(), MAX_REASON_CHARS).0);
                json!({
                    "server": target.base_url,
                    "state": "refused",
                    "status": status.as_u16(),
                    "reason": detail,
                })
            }
        }
        Err(error) => {
            let chain = format_error_chain(&error);
            json!({
                "server": target.base_url,
                "state": "unreachable",
                "reason": truncate_chars(&chain, MAX_REASON_CHARS).0,
            })
        }
    }
}

/// Execute a `BlossomPlan`: stage to local Blossom first (if running), then upload
/// to configured public Blossom servers, and construct `imeta` attachments obeying
/// the public-relay URL safety gate.
pub async fn upload_prepared_media_with_plan(
    files: &[PreparedMedia],
    plan: &BlossomPlan,
    auth_header: &str,
    policy: AddressPolicy,
) -> Result<(Vec<NostrMediaAttachment>, Vec<Value>), McpToolError> {
    let local_client = build_blossom_http_client(UPLOAD_TIMEOUT, AddressPolicy::LoopbackAllowed)?;
    let public_client = build_blossom_http_client(UPLOAD_TIMEOUT, policy)?;

    let mut attachments = Vec::with_capacity(files.len());
    let mut reports = Vec::with_capacity(files.len());

    for file in files {
        let mut local_url: Option<String> = None;
        let mut public_urls: Vec<String> = Vec::new();
        let mut server_outcomes: Vec<Value> = Vec::new();

        // 1. Always stage to local Blossom sidecar first when available.
        if let Some(ref local) = plan.local_target {
            let outcome = upload_single_blob(
                &local_client,
                local,
                file,
                auth_header,
                AddressPolicy::LoopbackAllowed,
            )
            .await;
            if outcome["state"] == "uploaded"
                && let Some(u) = outcome["url"].as_str()
            {
                local_url = Some(u.to_string());
                if let Some(ref pub_base) = plan.public_base_url {
                    let ext = ext_for_mime(&file.mime_type);
                    let pub_url = format!("{}/{}{ext}", pub_base.base_url, file.sha256);
                    if !public_urls.contains(&pub_url) {
                        public_urls.push(pub_url);
                    }
                }
            }
            server_outcomes.push(outcome);
        }

        // 2. Upload to configured public Blossom servers.
        for target in &plan.public_targets {
            let outcome =
                upload_single_blob(&public_client, target, file, auth_header, policy).await;
            if outcome["state"] == "uploaded"
                && let Some(u) = outcome["url"].as_str()
                && !public_urls.contains(&u.to_string())
            {
                public_urls.push(u.to_string());
            }
            server_outcomes.push(outcome);
        }

        // 3. Select imeta URLs respecting the public-relay URL safety gate.
        let mut chosen_urls = if plan.has_public_relays {
            // CRITICAL: never allow a 127.0.0.1 URL into an event bound for public relays.
            public_urls
        } else if !public_urls.is_empty() {
            let mut merged = public_urls;
            if let Some(ref lu) = local_url
                && !merged.contains(lu)
            {
                merged.push(lu.clone());
            }
            merged
        } else if let Some(lu) = local_url.clone() {
            vec![lu]
        } else {
            Vec::new()
        };

        if chosen_urls.is_empty() {
            let detail = server_outcomes
                .iter()
                .map(|o| {
                    format!(
                        "{} {} ({})",
                        o["server"].as_str().unwrap_or("server"),
                        o["state"].as_str().unwrap_or("failed"),
                        o["reason"].as_str().unwrap_or("no detail")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(McpToolError::unavailable(format!(
                "Blossom upload failed for {}: {detail}; the note was not signed or published",
                file.path
            )));
        }

        let primary_url = chosen_urls.remove(0);
        let fallback_urls = chosen_urls;
        reports.push(json!({
            "path": file.path,
            "sha256": file.sha256,
            "mime_type": file.mime_type,
            "size": file.size,
            "dimensions": file.dimensions.map(|(w, h)| format!("{w}x{h}")),
            "url": primary_url,
            "fallback_urls": fallback_urls,
            "local_staged": local_url.is_some(),
            "servers": server_outcomes,
        }));
        attachments.push(NostrMediaAttachment {
            url: primary_url,
            mime_type: file.mime_type.clone(),
            sha256: file.sha256.clone(),
            size: file.size,
            dimensions: file.dimensions,
            fallback_urls,
        });
    }

    Ok((attachments, reports))
}

/// An `imeta` media reference extracted from a verified Nostr event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImetaCandidate {
    pub event_id: String,
    pub author: String,
    pub url: String,
    pub sha256: String,
    pub mime_type: String,
}

fn is_lower_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Extract `imeta` entries carrying both `url` and a 64-char hex `x` digest.
pub fn extract_imeta_candidates(event: &Event) -> Vec<ImetaCandidate> {
    let mut out = Vec::new();
    let event_id = event.id.to_hex();
    let author = event.pubkey.to_hex();
    for tag in event.tags.iter() {
        let slice = tag.as_slice();
        if slice.first().map(String::as_str) != Some("imeta") {
            continue;
        }
        let mut url = None;
        let mut sha256 = None;
        let mut mime_type = None;
        for item in &slice[1..] {
            if let Some(v) = item.strip_prefix("url ") {
                url = Some(v.trim().to_string());
            } else if let Some(v) = item.strip_prefix("x ") {
                let lower = v.trim().to_ascii_lowercase();
                if is_lower_sha256_hex(&lower) {
                    sha256 = Some(lower);
                }
            } else if let Some(v) = item.strip_prefix("m ") {
                mime_type = Some(v.trim().to_string());
            }
        }
        if let (Some(u), Some(x)) = (url, sha256) {
            out.push(ImetaCandidate {
                event_id: event_id.clone(),
                author: author.clone(),
                url: u,
                sha256: x,
                mime_type: mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
            });
        }
    }
    out
}

/// Why downloading or verifying an `imeta` / local Blossom blob failed.
#[derive(Debug, thiserror::Error)]
pub enum BlobDownloadError {
    #[error("{0}")]
    InvalidUrl(&'static str),
    #[error("fetch failed: {0}")]
    Fetch(reqwest::Error),
    #[error("HTTP {0}")]
    HttpStatus(u16),
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("remote blob size {size} exceeds {MAX_BLOSSOM_UPLOAD_BYTES} bytes")]
    TooLarge { size: u64 },
    #[error("reading body failed: {0}")]
    ReadBody(reqwest::Error),
    #[error("remote blob is empty")]
    Empty,
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    Sha256Mismatch { expected: String, actual: String },
}

/// Maximum HTTP redirects followed when fetching an `imeta` blob. Every
/// redirect target is re-validated against `policy` to block SSRF.
const MAX_BLOB_REDIRECTS: usize = 3;

/// Download a blob from an `imeta` URL under `policy` (`PublicOnly` in prod =
/// SSRF `public_https_only`), following up to 3 policy-validated redirects,
/// enforcing `MAX_BLOSSOM_UPLOAD_BYTES`, and verifying its SHA-256 against
/// `expected_sha256`.
pub async fn download_and_verify_blob(
    client: &reqwest::Client,
    raw_url: &str,
    expected_sha256: &str,
    policy: AddressPolicy,
) -> Result<(Vec<u8>, String), BlobDownloadError> {
    let mut current_url =
        validate_returned_blob_url(raw_url, policy).map_err(BlobDownloadError::InvalidUrl)?;
    let resp = {
        let mut hops = 0usize;
        loop {
            let r = client
                .get(&current_url)
                .send()
                .await
                .map_err(BlobDownloadError::Fetch)?;
            let status = r.status();
            if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                if hops >= MAX_BLOB_REDIRECTS {
                    return Err(BlobDownloadError::TooManyRedirects);
                }
                let Some(loc) = r
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                else {
                    return Err(BlobDownloadError::HttpStatus(status.as_u16()));
                };
                let resolved = reqwest::Url::parse(&current_url)
                    .and_then(|base| base.join(loc.trim()))
                    .map_err(|_| {
                        BlobDownloadError::InvalidUrl("redirect Location is not a valid URL")
                    })?;
                current_url = validate_returned_blob_url(resolved.as_str(), policy)
                    .map_err(BlobDownloadError::InvalidUrl)?;
                hops += 1;
                continue;
            }
            break r;
        }
    };
    if !resp.status().is_success() {
        return Err(BlobDownloadError::HttpStatus(resp.status().as_u16()));
    }
    if let Some(len) = resp.content_length()
        && len > MAX_BLOSSOM_UPLOAD_BYTES
    {
        return Err(BlobDownloadError::TooLarge { size: len });
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = read_response_bytes_bounded(resp, MAX_BLOSSOM_UPLOAD_BYTES).await?;
    if bytes.is_empty() {
        return Err(BlobDownloadError::Empty);
    }
    let actual_sha = hex::encode(Sha256::digest(&bytes));
    if actual_sha != expected_sha256 {
        return Err(BlobDownloadError::Sha256Mismatch {
            expected: expected_sha256.to_string(),
            actual: actual_sha,
        });
    }
    Ok((bytes, content_type))
}

#[derive(Debug, Deserialize)]
pub struct LocalListEntry {
    pub sha256: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    pub size: u64,
}

/// List blobs owned by `pubkey_hex` from the local Blossom sidecar (`GET /list/<pubkey>`).
pub async fn list_local_blobs(
    client: &reqwest::Client,
    local_base_url: &str,
    pubkey_hex: &str,
) -> Result<Vec<LocalListEntry>, McpToolError> {
    let url = format!("{local_base_url}/list/{pubkey_hex}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("local Blossom /list failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(McpToolError::unavailable(format!(
            "local Blossom /list returned HTTP {}",
            resp.status().as_u16()
        )));
    }
    let bytes = read_response_bytes_bounded(resp, MAX_LIST_RESPONSE_BYTES)
        .await
        .map_err(|e| McpToolError::unavailable(format!("local Blossom /list body error: {e}")))?;
    serde_json::from_slice::<Vec<LocalListEntry>>(&bytes)
        .map_err(|e| McpToolError::unavailable(format!("local Blossom /list JSON invalid: {e}")))
}

/// Append each uploaded media URL to `caption` when not already present so
/// Nostr clients render inline media and match the `imeta` tags (NIP-92).
pub fn compose_note_content(caption: &str, attachments: &[NostrMediaAttachment]) -> String {
    let mut text = caption.trim_end().to_string();
    for att in attachments {
        if !text.contains(&att.url) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&att.url);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_policy_requires_https_public_hosts_without_credentials() {
        let target =
            validate_blossom_server("https://Blossom.Primal.net/", AddressPolicy::PublicOnly)
                .expect("public https Blossom server accepted");
        assert_eq!(target.base_url, "https://blossom.primal.net");
        assert_eq!(target.upload_url, "https://blossom.primal.net/upload");
        assert_eq!(target.server_domain, "blossom.primal.net");

        assert_eq!(
            validate_blossom_server("http://blossom.primal.net", AddressPolicy::PublicOnly),
            Err(BlossomAddressError::NotHttps)
        );
        assert_eq!(
            validate_blossom_server("https://127.0.0.1:8080", AddressPolicy::PublicOnly),
            Err(BlossomAddressError::NotPublic)
        );
        assert_eq!(
            validate_blossom_server("https://localhost:8080", AddressPolicy::PublicOnly),
            Err(BlossomAddressError::NotPublic)
        );
        assert_eq!(
            validate_blossom_server("https://u:p@blossom.primal.net", AddressPolicy::PublicOnly),
            Err(BlossomAddressError::HasCredentials)
        );
        assert!(
            validate_blossom_server("http://127.0.0.1:18080", AddressPolicy::LoopbackAllowed)
                .is_ok()
        );
        for blocked in [
            "http://169.254.169.254",
            "https://169.254.169.254",
            "http://10.0.0.1:8080",
            "https://10.0.0.1:8080",
            "http://192.168.1.10:8080",
            "https://192.168.1.10:8080",
            "http://blossom.primal.net",
        ] {
            assert!(
                validate_blossom_server(blocked, AddressPolicy::LoopbackAllowed).is_err(),
                "{blocked} must remain blocked under LoopbackAllowed"
            );
            assert!(
                validate_returned_blob_url(blocked, AddressPolicy::LoopbackAllowed).is_err(),
                "returned blob URL {blocked} must remain blocked under LoopbackAllowed"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_local_media_rejects_symlinks_dotfiles_and_extension_spoofing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real_png = dir.path().join("real.png");
        std::fs::write(&real_png, b"\x89PNG\r\n\x1a\nvalid-png-header-bytes").expect("writes png");

        // 1. Symlink to a valid media file must be rejected.
        let link_png = dir.path().join("linked.png");
        std::os::unix::fs::symlink(&real_png, &link_png).expect("creates symlink");
        let sym_err = prepare_local_media(&[link_png.display().to_string()])
            .await
            .expect_err("symlink must be rejected");
        assert!(
            sym_err.to_string().to_ascii_lowercase().contains("symlink"),
            "expected symlink error, got: {sym_err}"
        );

        // 2. Sensitive dotdir / dotfile path must be rejected even with valid PNG bytes.
        let ssh_dir = dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).expect("creates .ssh");
        let dot_media = ssh_dir.join("key.png");
        std::fs::write(&dot_media, b"\x89PNG\r\n\x1a\nvalid-png-header-bytes")
            .expect("writes dotdir png");
        assert!(
            prepare_local_media(&[dot_media.display().to_string()])
                .await
                .is_err(),
            "sensitive .ssh path must be rejected"
        );
        let hidden_file = dir.path().join(".env.png");
        std::fs::write(&hidden_file, b"\x89PNG\r\n\x1a\nvalid-png-header-bytes")
            .expect("writes dotfile");
        assert!(
            prepare_local_media(&[hidden_file.display().to_string()])
                .await
                .is_err(),
            "hidden dotfile must be rejected"
        );

        // 3. Plaintext secret renamed to .mp4 without magic bytes must be rejected.
        let spoofed = dir.path().join("spoofed.mp4");
        std::fs::write(&spoofed, b"OPENROUTER_API_KEY=sk-secret-value").expect("writes spoofed");
        assert!(
            prepare_local_media(&[spoofed.display().to_string()])
                .await
                .is_err(),
            "extension-only spoof without media magic bytes must be rejected"
        );
    }

    #[tokio::test]
    async fn read_response_bytes_bounded_aborts_oversized_chunked_stream() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds loopback");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf).await;
                let hdr = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\n\r\n";
                let _ = stream.write_all(hdr).await;
                let chunk = [b'X'; 4096];
                let chunk_hdr = format!("{:X}\r\n", chunk.len());
                for _ in 0..8 {
                    if stream.write_all(chunk_hdr.as_bytes()).await.is_err() {
                        break;
                    }
                    if stream.write_all(&chunk).await.is_err() {
                        break;
                    }
                    if stream.write_all(b"\r\n").await.is_err() {
                        break;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n").await;
            }
        });

        let client =
            build_blossom_http_client(Duration::from_secs(5), AddressPolicy::LoopbackAllowed)
                .expect("client");
        let resp = client
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .expect("sends");
        let err = read_response_bytes_bounded(resp, 8192)
            .await
            .expect_err("stream exceeding 8 KiB cap must abort with TooLarge");
        assert!(
            matches!(err, BlobDownloadError::TooLarge { .. }),
            "expected TooLarge, got: {err:?}"
        );
    }

    #[test]
    fn compose_note_content_appends_missing_urls_without_duplicating_existing_ones() {
        let att = NostrMediaAttachment {
            url: "https://cdn.example/a.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            sha256: "a".repeat(64),
            size: 10,
            dimensions: None,
            fallback_urls: Vec::new(),
        };
        assert_eq!(
            compose_note_content("", std::slice::from_ref(&att)),
            "https://cdn.example/a.mp4"
        );
        assert_eq!(
            compose_note_content("Knife fight clip", std::slice::from_ref(&att)),
            "Knife fight clip\nhttps://cdn.example/a.mp4"
        );
        assert_eq!(
            compose_note_content("Watch https://cdn.example/a.mp4 now", &[att]),
            "Watch https://cdn.example/a.mp4 now"
        );
    }

    #[test]
    fn resolve_blossom_plan_refuses_loopback_only_when_seed_relays_present() {
        let local = LocalBlossomState::Announced {
            base_url: "http://127.0.0.1:41234".to_string(),
            addr: "127.0.0.1:41234".parse().unwrap(),
        };
        let err = resolve_blossom_plan(&[], None, &local, true, AddressPolicy::PublicOnly)
            .expect_err("must refuse publishing 127.0.0.1 URLs to public seed_relays");
        assert!(err.to_string().contains("public"));
    }

    #[test]
    fn resolve_blossom_plan_allows_loopback_when_local_only() {
        let local = LocalBlossomState::Announced {
            base_url: "http://127.0.0.1:41234".to_string(),
            addr: "127.0.0.1:41234".parse().unwrap(),
        };
        let plan = resolve_blossom_plan(&[], None, &local, false, AddressPolicy::PublicOnly)
            .expect("local-only staging allows loopback Blossom");
        assert!(plan.local_target.is_some());
        assert!(!plan.has_public_relays);
    }

    #[test]
    fn resolve_blossom_plan_accepts_public_base_url_without_blossom_servers() {
        let local = LocalBlossomState::Announced {
            base_url: "http://127.0.0.1:41234".to_string(),
            addr: "127.0.0.1:41234".parse().unwrap(),
        };
        let plan = resolve_blossom_plan(
            &[],
            Some("https://media.example.com"),
            &local,
            true,
            AddressPolicy::PublicOnly,
        )
        .expect("public_base_url fronting local sidecar satisfies public relay gate");
        assert!(plan.local_target.is_some());
        assert_eq!(
            plan.public_base_url.as_ref().unwrap().base_url,
            "https://media.example.com"
        );
    }

    #[tokio::test]
    async fn bud06_preflight_fails_fast_on_507_and_falls_through_on_405() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            for reply in [
                "HTTP/1.1 507 Insufficient Storage\r\nX-Reason: quota exceeded\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ] {
                let (mut sock, _) = listener.accept().await.expect("accept");
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                sock.write_all(reply.as_bytes()).await.expect("write");
            }
        });

        let client = reqwest::Client::new();
        let target = BlossomTarget {
            base_url: format!("http://{addr}"),
            upload_url: format!("http://{addr}/upload"),
            server_domain: addr.to_string(),
        };
        let file = PreparedMedia {
            path: "sample.png".into(),
            mime_type: "image/png".into(),
            sha256: "a".repeat(64),
            size: 1024,
            dimensions: None,
            bytes: vec![0u8; 16],
        };

        // 507 fails fast before PUT
        let rejected = preflight_upload(&client, &target, &file, "Nostr dGVzdA==")
            .await
            .expect("507 must fail fast");
        assert_eq!(rejected["state"], "refused");
        assert_eq!(rejected["status"], 507);
        assert_eq!(rejected["reason"], "quota exceeded");
        assert_eq!(rejected["preflight"], true);

        // 405 falls through gracefully (BUD-06 unsupported)
        let fallback = preflight_upload(&client, &target, &file, "Nostr dGVzdA==").await;
        assert!(fallback.is_none(), "405 must allow PUT fallback");

        server.await.expect("server");
    }
}
