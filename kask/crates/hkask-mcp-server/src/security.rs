//! MCP security — URL validation for tool endpoints
//!
//! Provides SSRF protection for MCP tool invocations:
//! - URL validation (scheme, credentials, private IP, loopback)
//! - DNS resolution to defeat hostname-based SSRF bypasses (CWE-918/441)

use std::net::{IpAddr, SocketAddr};

/// URL validation error types
#[derive(Debug, thiserror::Error)]
pub(crate) enum SecurityError {
    #[error("Non-HTTP(S) scheme not allowed: {0}")]
    DisallowedScheme(String),

    #[error("URL contains embedded credentials (user:pass@host): {0}")]
    EmbeddedCredentials(String),

    #[error("Private IP address not allowed: {0}")]
    PrivateIpNotAllowed(String),

    #[error("Loopback address not allowed: {0}")]
    LoopbackNotAllowed(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// URL validation configuration
///
/// Controls whether private IP ranges and loopback addresses are allowed.
/// The default (`UrlValidationConfig::default()`) is strict: both are
/// rejected. Use `UrlValidationConfig::permissive()` to allow both, for
/// user-curated URL lists like RSS subscriptions where the user has
/// explicitly chosen to fetch from a local network address.
#[derive(Debug, Clone, Default)]
pub(crate) struct UrlValidationConfig {
    /// Allow private IP addresses (10.x, 172.16-31.x, 192.168.x, 169.254.x)
    pub allow_private_ips: bool,
    /// Allow loopback addresses (127.x.x.x, ::1)
    pub allow_loopback: bool,
}

impl UrlValidationConfig {
    /// Permissive config: allows private IPs and loopback.
    ///
    /// Use this for user-curated URL lists (e.g., RSS subscriptions) where
    /// the user has explicitly chosen to fetch from a local address (e.g.,
    /// a self-hosted RSS aggregator at `http://localhost:4000/feed.xml`).
    /// Do NOT use this for arbitrary user-supplied URLs from untrusted
    /// sources (e.g., `web_extract` tool input).
    #[must_use]
    pub fn permissive() -> Self {
        Self {
            allow_private_ips: true,
            allow_loopback: true,
        }
    }
}

/// Validate a URL for use in MCP web/scholar requests.
///
/// Checks:
/// - Rejects non-HTTP(S) schemes
/// - Rejects URLs with embedded credentials (user:pass@host)
/// - Rejects private IPs unless explicitly permitted
/// - Rejects loopback addresses unless explicitly permitted
pub(crate) fn validate_url(
    raw_url: &str,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    let scheme_end = raw_url
        .find("://")
        .ok_or_else(|| SecurityError::InvalidUrl("No scheme separator '://' found".to_string()))?;
    let scheme = &raw_url[..scheme_end];
    if scheme != "http" && scheme != "https" {
        return Err(SecurityError::DisallowedScheme(scheme.to_string()));
    }

    let after_scheme = &raw_url[scheme_end + 3..];
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host_part = authority.split('@').next_back().unwrap_or(authority);
    if host_part != authority {
        return Err(SecurityError::EmbeddedCredentials(raw_url.to_string()));
    }

    let host = host_part.split(':').next().unwrap_or(host_part);
    let bracket_close = host.rfind(']');
    let hostname = if host.starts_with('[') {
        bracket_close
            .map(|i| &host[1..i])
            .ok_or_else(|| SecurityError::InvalidUrl("Malformed IPv6 address".to_string()))?
    } else {
        host
    };

    let ip: Option<IpAddr> = hostname.parse().ok();

    if let Some(ip) = ip {
        if ip.is_loopback() && !config.allow_loopback {
            return Err(SecurityError::LoopbackNotAllowed(ip.to_string()));
        }
        if is_private_ip(&ip) && !config.allow_private_ips {
            return Err(SecurityError::PrivateIpNotAllowed(ip.to_string()));
        }
    }

    // Return the hostname so callers that want DNS-level SSRF protection
    // (defeating hostname-based bypasses where a non-literal hostname
    // resolves to a private/loopback IP) can resolve it via
    // [`validate_url_with_dns`]. The sync [`validate_url`] only checks
    // literal-IP hostnames; a hostname like `attacker.example` resolving to
    // 127.0.0.1 or 169.254.169.254 bypasses the literal check above.
    let _ = hostname;
    Ok(())
}

/// DNS-resolved SSRF validation (CWE-918/441).
///
/// This is the async, defense-in-depth companion to [`validate_url`]. It runs
/// the sync checks first (scheme, embedded credentials, literal-IP blocklist),
/// then resolves the hostname via `tokio::net::lookup_host` and rejects if any
/// resolved address is loopback or private (unless the config permits it).
///
/// This closes the hostname-bypass gap in [`validate_url`]: a non-literal
/// hostname (e.g. `attacker.example` resolving to `127.0.0.1` or
/// `169.254.169.254`) passes the literal-IP check but is caught here.
///
/// A TOCTOU remains between this resolve and the downstream `reqwest` connect
/// (DNS rebinding), but the gap closed here is the absence of any DNS step at
/// all — the pre-fix code never resolved the hostname. A custom reqwest
/// connector that re-checks the resolved IP at connect time would close the
/// TOCTOU; that is future hardening, not in scope for this fix.
pub(crate) async fn validate_url_with_dns(
    raw_url: &str,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    // Run the sync checks first (scheme, credentials, literal-IP blocklist).
    validate_url(raw_url, config)?;

    // Re-parse to extract the hostname for DNS resolution. This mirrors the
    // parsing in validate_url; if validate_url succeeded, this parse is safe.
    let scheme_end = raw_url
        .find("://")
        .ok_or_else(|| SecurityError::InvalidUrl("No scheme separator '://' found".to_string()))?;
    let after_scheme = &raw_url[scheme_end + 3..];
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host_part = authority.split('@').next_back().unwrap_or(authority);
    let host = host_part.split(':').next().unwrap_or(host_part);
    let hostname = if host.starts_with('[') {
        let bracket_close = host
            .rfind(']')
            .ok_or_else(|| SecurityError::InvalidUrl("Malformed IPv6 address".to_string()))?;
        &host[1..bracket_close]
    } else {
        host
    };

    // Literal-IP hostnames were already checked by validate_url. Only resolve
    // non-literal hostnames (DNS names) — resolving a literal IP is redundant
    // and would re-check what validate_url already covered.
    if hostname.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    // Resolve the hostname. `lookup_host` requires a `host:port` pair; use a
    // dummy port (the resolved IPs are what we check, not the port). If DNS
    // fails, treat it as an invalid URL (the fetch would fail anyway).
    let resolve_target: String = format!("{hostname}:0");
    let resolved: Vec<SocketAddr> = tokio::net::lookup_host(&resolve_target)
        .await
        .map_err(|e| {
            SecurityError::InvalidUrl(format!("DNS resolution failed for {hostname}: {e}"))
        })?
        .collect();

    if resolved.is_empty() {
        return Err(SecurityError::InvalidUrl(format!(
            "DNS returned no addresses for {hostname}"
        )));
    }

    for addr in &resolved {
        let ip = addr.ip();
        if ip.is_loopback() && !config.allow_loopback {
            return Err(SecurityError::LoopbackNotAllowed(format!(
                "{hostname} resolves to loopback {ip}"
            )));
        }
        if is_private_ip(&ip) && !config.allow_private_ips {
            return Err(SecurityError::PrivateIpNotAllowed(format!(
                "{hostname} resolves to private IP {ip}"
            )));
        }
    }

    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 10
                || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            // fc00::/7 — Unique Local Addresses (includes fc00:: through fdff:...)
            let is_ula = (segments[0] & 0xfe00) == 0xfc00;
            // fe80::/10 — Link-Local addresses
            let is_link_local = (segments[0] & 0xffc0) == 0xfe80;
            is_ula || is_link_local
        }
    }
}

//
// The check is gated behind an env var so it doesn't run on
// every `cargo test` invocation (it walks the whole workspace).
// CI sets `HKASK_RUN_MCP_GATE_AUDIT=1` to enable it.
