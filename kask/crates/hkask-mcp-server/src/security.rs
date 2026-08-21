//! MCP security — URL validation for tool endpoints
//!
//! Provides SSRF protection for MCP tool invocations:
//! - URL validation (scheme, credentials, private IP, loopback)
//! - DNS resolution to defeat hostname-based SSRF bypasses (CWE-918/441)

use std::net::{IpAddr, SocketAddr};

use crate::server::McpToolError;

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

/// Parse a URL and return (scheme, hostname) after basic structural checks.
///
/// Shared by [`validate_url`] (sync, literal-IP only) and
/// [`validate_url_with_dns`] (async, DNS-resolved). Extracting this avoids
/// duplicating the URL-parsing logic across the two functions.
///
/// Returns:
/// - `Ok((scheme, hostname))` if the URL has a valid scheme separator and
///   no embedded credentials.
/// - `Err(DisallowedScheme)` if the scheme is not http/https.
/// - `Err(EmbeddedCredentials)` if the authority contains `user:pass@`.
/// - `Err(InvalidUrl)` if the URL is malformed (no `://`, bad IPv6 brackets).
fn parse_url_for_ssrf(raw_url: &str) -> Result<(&str, &str), SecurityError> {
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

    // Bracketed IPv6 (e.g. `[::1]:8080`) must be extracted before stripping the
    // port — splitting on ':' first would truncate to "[" and lose the address.
    let hostname = if let Some(rest) = host_part.strip_prefix('[') {
        let bracket_close = rest
            .find(']')
            .ok_or_else(|| SecurityError::InvalidUrl("Malformed IPv6 address".to_string()))?;
        &rest[..bracket_close]
    } else {
        host_part.split(':').next().unwrap_or(host_part)
    };

    Ok((scheme, hostname))
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
    let (_scheme, hostname) = parse_url_for_ssrf(raw_url)?;

    let ip: Option<IpAddr> = hostname.parse().ok();

    if let Some(ip) = ip {
        if ip.is_loopback() && !config.allow_loopback {
            return Err(SecurityError::LoopbackNotAllowed(ip.to_string()));
        }
        if is_private_ip(&ip) && !config.allow_private_ips {
            return Err(SecurityError::PrivateIpNotAllowed(ip.to_string()));
        }
    }

    // NOTE: this sync check only catches *literal-IP* hostnames. A
    // non-literal hostname (e.g. `attacker.example` resolving to `127.0.0.1`
    // or `169.254.169.254`) passes this check because `hostname.parse()`
    // returns `None` for DNS names. Use [`validate_url_with_dns`] for
    // defense-in-depth DNS resolution that closes this gap.
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

    // Extract the hostname via the shared parser (same logic as validate_url,
    // no duplication). If validate_url succeeded, this parse is safe.
    let (_scheme, hostname) = parse_url_for_ssrf(raw_url)?;

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

// ── Public SSRF entry points (re-exported via `server`) ──────────────────
// These wrap the pub(crate) `validate_url*` with the `McpToolError` adaptation
// and the default/permissive config. They live here, next to the impl, so all
// SSRF defense (parsing, literal-IP blocklist, DNS resolution, and the
// MCP-facing entry points) is in one module — previously the wrappers were in
// `validation.rs`, forcing a bounce between two files to follow one concern.

/// Validate a tool URL with DNS resolution (async, defense-in-depth).
///
/// Recommended SSRF entry point for any tool that accepts an untrusted URL and
/// fetches it: runs the sync checks (scheme, credentials, literal-IP blocklist)
/// then resolves the hostname, rejecting if any resolved IP is loopback or
/// private. Closes the hostname-bypass gap in the sync baseline. A TOCTOU (DNS
/// rebinding) between this resolve and the downstream connect remains; closing
/// it needs a custom reqwest connector (future hardening).
#[must_use = "result must be used"]
pub async fn validate_tool_url_with_dns(url: &str) -> Result<(), McpToolError> {
    validate_url_with_dns(url, &UrlValidationConfig::default())
        .await
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

/// Validate a tool URL with permissive SSRF config (allows private IPs + loopback).
///
/// For user-curated URL lists (e.g. RSS subscriptions) where the user has
/// explicitly chosen a local address. Do NOT use for arbitrary untrusted URLs.
#[must_use = "result must be used"]
pub fn validate_tool_url_permissive(url: &str) -> Result<(), McpToolError> {
    validate_url(url, &UrlValidationConfig::permissive())
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_private_ip_flags_ipv4_rfc1918_and_link_local() {
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.255.255.255".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.31.255.255".parse().unwrap()));
        assert!(!is_private_ip(&"172.15.0.1".parse().unwrap()));
        assert!(!is_private_ip(&"172.32.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.169.254".parse().unwrap())); // link-local / metadata endpoint
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_flags_ipv6_ula_and_link_local() {
        assert!(is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(is_private_ip(&"fd12:3456::1".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(!is_private_ip(&"2001:4860:4860::8888".parse().unwrap()));
        assert!(!is_private_ip(&"::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_loopback_is_a_separate_gate() {
        // 127.0.0.1 / ::1 are loopback, not "private" per is_private_ip —
        // validate_url checks is_loopback() separately (gated by allow_loopback).
        assert!(!is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(!is_private_ip(&"::1".parse().unwrap()));
    }

    #[test]
    fn parse_url_for_ssrf_rejects_non_http_schemes() {
        assert!(matches!(
            parse_url_for_ssrf("file:///etc/passwd"),
            Err(SecurityError::DisallowedScheme(_))
        ));
        assert!(matches!(
            parse_url_for_ssrf("gopher://x"),
            Err(SecurityError::DisallowedScheme(_))
        ));
        assert!(parse_url_for_ssrf("https://example.com").is_ok());
        assert!(parse_url_for_ssrf("http://example.com").is_ok());
    }

    #[test]
    fn parse_url_for_ssrf_rejects_embedded_credentials() {
        assert!(matches!(
            parse_url_for_ssrf("https://user:pass@example.com"),
            Err(SecurityError::EmbeddedCredentials(_))
        ));
        // an `@` in the path is not authority credentials
        assert!(parse_url_for_ssrf("https://example.com/path@x").is_ok());
    }

    #[test]
    fn parse_url_for_ssrf_handles_ipv6_brackets() {
        let (scheme, host) = parse_url_for_ssrf("http://[::1]:8080/").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(host, "::1");
        assert!(matches!(
            parse_url_for_ssrf("http://[::1"),
            Err(SecurityError::InvalidUrl(_))
        ));
    }

    #[test]
    fn validate_url_rejects_literal_private_and_loopback() {
        let strict = UrlValidationConfig::default();
        assert!(validate_url("http://10.0.0.1", &strict).is_err());
        assert!(validate_url("http://169.254.169.254", &strict).is_err());
        assert!(validate_url("http://127.0.0.1", &strict).is_err());
        assert!(validate_url("http://[::1]", &strict).is_err());
        assert!(validate_url("http://8.8.8.8", &strict).is_ok());
    }

    #[test]
    fn validate_url_permissive_allows_private_and_loopback() {
        let permissive = UrlValidationConfig::permissive();
        assert!(validate_url("http://10.0.0.1", &permissive).is_ok());
        assert!(validate_url("http://127.0.0.1", &permissive).is_ok());
        assert!(validate_url("http://169.254.169.254", &permissive).is_ok());
        // permissive still rejects bad schemes / embedded creds
        assert!(validate_url("file:///etc/passwd", &permissive).is_err());
        assert!(validate_url("https://user:pass@host", &permissive).is_err());
    }

    #[test]
    fn validate_url_hostname_passes_literal_check() {
        // A non-literal hostname is not an IP — the sync check cannot catch a
        // DNS-rebind to 127.0.0.1; that gap is validate_url_with_dns's job (which
        // needs a real resolver and isn't unit-testable without DNS).
        let strict = UrlValidationConfig::default();
        assert!(validate_url("https://example.com", &strict).is_ok());
    }
}
