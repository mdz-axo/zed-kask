//! Operator-set Blossom sidecar configuration, read once from the environment.

use std::collections::BTreeSet;
use std::path::PathBuf;

use hkask_types::nostr_wire::NOSTR_BLOSSOM_SERVER;
use serde::Serialize;

/// Default disk quota for local Blossom blobs: 10 GiB. `0` disables the quota.
pub const DEFAULT_BLOSSOM_MAX_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// A config entry the operator wrote that this server could not use.
/// Never repeats the raw input text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefusedEntry {
    pub field: &'static str,
    pub position: usize,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    /// `0` means unlimited.
    pub max_bytes: u64,
    /// Optional public `https://` base URL (e.g. Tailscale Funnel / reverse proxy).
    pub public_base_url: Option<String>,
    /// Public host extracted from `public_base_url` (lowercase `host` or `host:port`),
    /// admitted in `Host` header checks alongside loopback.
    pub public_host: Option<String>,
    /// Allowlisted 32-byte x-only pubkeys permitted to upload/delete.
    pub allowed_authors: BTreeSet<[u8; 32]>,
    pub refused: Vec<RefusedEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 0,
            max_bytes: DEFAULT_BLOSSOM_MAX_BYTES,
            public_base_url: None,
            public_host: None,
            allowed_authors: BTreeSet::new(),
            refused: Vec::new(),
        }
    }
}

pub fn default_store_dir() -> PathBuf {
    hkask_types::agent_paths::resolve_under_data_dir(&hkask_types::agent_paths::mcp_server_subdir(
        NOSTR_BLOSSOM_SERVER,
        "index.db",
    ))
    .parent()
    .expect("index.db has a parent dir")
    .to_path_buf()
}

pub fn parse_pubkey(raw: &str) -> Option<[u8; 32]> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("npub1") {
        return hkask_nostr_core::decode_npub(trimmed).ok();
    }
    if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes = hex::decode(trimmed).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        return Some(arr);
    }
    None
}

pub fn validate_public_base_url(raw: &str) -> Result<(String, String), &'static str> {
    let trimmed = raw.trim();
    let parsed = url::Url::parse(trimmed).map_err(|_| "not a valid URL")?;
    if parsed.scheme() != "https" {
        return Err("public_base_url must use https://");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("public_base_url must not carry credentials");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("public_base_url must not carry a query or fragment");
    }
    let host = parsed
        .host_str()
        .ok_or("public_base_url has no host")?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host == "127.0.0.1" || host == "::1" {
        return Err("public_base_url must name a public host, not loopback");
    }
    let host_port = match parsed.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    };
    let clean_path = parsed.path().trim_end_matches('/');
    let origin = parsed.origin().ascii_serialization();
    let base = if clean_path.is_empty() {
        origin
    } else {
        format!("{origin}{clean_path}")
    };
    Ok((base, host_port))
}

impl Config {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        if let Ok(raw_port) = std::env::var("HKASK_NOSTR_BLOSSOM_PORT") {
            let trimmed = raw_port.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<u16>() {
                    Ok(port) => cfg.port = port,
                    Err(_) => cfg.refused.push(RefusedEntry {
                        field: "port",
                        position: 0,
                        reason: "not a valid TCP port (0..=65535)".to_string(),
                    }),
                }
            }
        }

        if let Ok(raw_max) = std::env::var("HKASK_NOSTR_BLOSSOM_MAX_BYTES") {
            let trimmed = raw_max.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<u64>() {
                    Ok(max_bytes) => cfg.max_bytes = max_bytes,
                    Err(_) => cfg.refused.push(RefusedEntry {
                        field: "max_bytes",
                        position: 0,
                        reason: "not a valid byte count".to_string(),
                    }),
                }
            }
        }

        if let Ok(raw_url) = std::env::var("HKASK_NOSTR_BLOSSOM_PUBLIC_URL") {
            let trimmed = raw_url.trim();
            if !trimmed.is_empty() {
                match validate_public_base_url(trimmed) {
                    Ok((base, host)) => {
                        cfg.public_base_url = Some(base);
                        cfg.public_host = Some(host);
                    }
                    Err(reason) => cfg.refused.push(RefusedEntry {
                        field: "public_base_url",
                        position: 0,
                        reason: reason.to_string(),
                    }),
                }
            }
        }

        if let Ok(raw_authors) = std::env::var("HKASK_NOSTR_BLOSSOM_AUTHORS") {
            for (idx, entry) in raw_authors.split(',').enumerate() {
                let trimmed = entry.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match parse_pubkey(trimmed) {
                    Some(pk) => {
                        cfg.allowed_authors.insert(pk);
                    }
                    None => cfg.refused.push(RefusedEntry {
                        field: "allowed_authors",
                        position: idx + 1,
                        reason: "not an npub or 64-character hex public key".to_string(),
                    }),
                }
            }
        }

        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_public_base_url_accepts_https_and_rejects_loopback_or_http() {
        assert_eq!(
            validate_public_base_url("https://media.example.com/"),
            Ok((
                "https://media.example.com".to_string(),
                "media.example.com".to_string()
            ))
        );
        assert!(validate_public_base_url("http://media.example.com").is_err());
        assert!(validate_public_base_url("https://127.0.0.1:8080").is_err());
        assert!(validate_public_base_url("https://u:p@media.example.com").is_err());
    }
}
