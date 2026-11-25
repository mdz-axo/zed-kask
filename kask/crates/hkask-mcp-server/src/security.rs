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

    // Never retain the rejected URL: this error crosses into the MCP tool result.
    #[error("URL contains embedded credentials (user:pass@host)")]
    EmbeddedCredentials,

    #[error("Private IP address not allowed: {0}")]
    PrivateIpNotAllowed(String),

    #[error("Loopback address not allowed: {0}")]
    LoopbackNotAllowed(String),

    #[error("Unspecified destination address not allowed: {0}")]
    UnspecifiedAddressNotAllowed(String),

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

/// Parse a URL via the WHATWG `url::Url` parser and return `(scheme, host)`
/// after scheme and embedded-credential checks.
///
/// Using `url::Url::parse` (matching `reqwest`'s parser) eliminates authority
/// splitting differentials (`http://127.0.0.1?x`, `http://127.0.0.1#x`) and
/// canonicalizes IPv4 shorthands (`127.1`, `0x7f000001`, `2130706433`).
fn parse_url_for_ssrf(raw_url: &str) -> Result<(String, String), SecurityError> {
    let trimmed = raw_url.trim();
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| SecurityError::InvalidUrl(format!("Malformed URL: {e}")))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(SecurityError::DisallowedScheme(scheme.to_string()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SecurityError::EmbeddedCredentials);
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| SecurityError::InvalidUrl("URL has no host".to_string()))?;
    let hostname = hkask_types::net_guard::normalize_host(host_raw);
    if hostname.is_empty() {
        return Err(SecurityError::InvalidUrl("URL has empty host".to_string()));
    }
    Ok((scheme.to_string(), hostname))
}

/// Check one literal or resolved destination address against the config.
///
/// Delegates to `hkask_types::net_guard` so IPv4, IPv4-mapped IPv6,
/// IPv4-compatible IPv6, NAT64, CGNAT, documentation, benchmarking, multicast,
/// and reserved ranges share one canonical classifier across the workspace.
fn policy_check_address(
    address: &IpAddr,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    match address.to_canonical() {
        IpAddr::V4(v4) => policy_check_ipv4(v4, config),
        IpAddr::V6(v6) => {
            if !config.allow_private_ips && v6.is_unspecified() {
                return Err(SecurityError::UnspecifiedAddressNotAllowed(v6.to_string()));
            }
            if !config.allow_loopback && v6.is_loopback() {
                return Err(SecurityError::LoopbackNotAllowed(v6.to_string()));
            }
            if let Some(embedded) = hkask_types::net_guard::embedded_ipv4(v6) {
                policy_check_ipv4(embedded, config)?;
            }
            if !config.allow_private_ips && is_private_ip(&IpAddr::V6(v6)) {
                return Err(SecurityError::PrivateIpNotAllowed(v6.to_string()));
            }
            Ok(())
        }
    }
}

fn policy_check_ipv4(
    v4: std::net::Ipv4Addr,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    if !config.allow_private_ips && v4.octets()[0] == 0 {
        return Err(SecurityError::UnspecifiedAddressNotAllowed(v4.to_string()));
    }
    if !config.allow_loopback && v4.is_loopback() {
        return Err(SecurityError::LoopbackNotAllowed(v4.to_string()));
    }
    if !config.allow_private_ips && is_private_ip(&IpAddr::V4(v4)) {
        return Err(SecurityError::PrivateIpNotAllowed(v4.to_string()));
    }
    Ok(())
}

/// Validate a URL for use in MCP web/scholar requests.
///
/// Checks:
/// - Rejects non-HTTP(S) schemes
/// - Rejects URLs with embedded credentials (user:pass@host)
/// - Rejects private IPs unless explicitly permitted
/// - Rejects loopback addresses unless explicitly permitted
/// - Rejects unspecified destinations (0.0.0.0/8, ::) under the private gate
/// - Applies the same policy to IPv4, IPv4-mapped IPv6, and IPv4 embedded in
///   IPv6-compatible/NAT64 addresses
pub(crate) fn validate_url(
    raw_url: &str,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    let (_scheme, hostname) = parse_url_for_ssrf(raw_url)?;

    let ip: Option<IpAddr> = hostname.parse().ok();

    if let Some(ip) = ip {
        policy_check_address(&ip, config)?;
    }

    // NOTE: this sync check only catches *literal-IP* hostnames. A
    // non-literal hostname (e.g. `attacker.example` resolving to `127.0.0.1`
    // or `169.254.169.254`) passes this check because `hostname.parse()`
    // returns `None` for DNS names. Use [`validate_url_with_dns`] for
    // defense-in-depth DNS resolution that closes this gap, or pair
    // [`validate_tool_url_literal`] with a connect-time validating resolver
    // (see the research crate's raw-fetch client).
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
/// A TOCTOU remains between this resolve and the downstream connect for any
/// consumer that fetches with its own uncontrolled client (DNS rebinding:
/// the resolver may answer differently at connect time). Consumers that need
/// the validated destination to be the connected one must re-validate at
/// connect time — the research crate's raw-fetch client does this with a
/// validating `reqwest::dns::Resolve` implementation
/// ([`validate_resolved_addresses`]), so its gap is closed at the transport.
/// Other consumers of this function keep the documented gap.
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

    validate_resolved_addresses_core(&hostname, &resolved, config)
}

/// Strictly validate an already-resolved address list for one hostname.
///
/// The address-policy core of [`validate_url_with_dns`], shared with
/// connect-time validating DNS resolvers (the research crate's raw-fetch
/// client) that gate the exact addresses a connection will use. The hostname
/// is folded into the error message so the caller can see which name resolved
/// to the forbidden destination.
pub(crate) fn validate_resolved_addresses_core(
    hostname: &str,
    resolved: &[SocketAddr],
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    if resolved.is_empty() {
        return Err(SecurityError::InvalidUrl(format!(
            "DNS returned no addresses for {hostname}"
        )));
    }

    for address in resolved {
        policy_check_address(&address.ip(), config)
            .map_err(|error| with_hostname(hostname, error))?;
    }

    Ok(())
}

/// Fold the hostname into a resolved-address policy error so DNS-path errors
/// name the offending host ("x resolves to loopback 127.0.0.1") while literal
/// errors keep the bare address form.
fn with_hostname(hostname: &str, error: SecurityError) -> SecurityError {
    match error {
        SecurityError::LoopbackNotAllowed(address) => {
            SecurityError::LoopbackNotAllowed(format!("{hostname} resolves to loopback {address}"))
        }
        SecurityError::PrivateIpNotAllowed(address) => SecurityError::PrivateIpNotAllowed(format!(
            "{hostname} resolves to private IP {address}"
        )),
        SecurityError::UnspecifiedAddressNotAllowed(address) => {
            SecurityError::UnspecifiedAddressNotAllowed(format!(
                "{hostname} resolves to unspecified address {address}"
            ))
        }
        other => other,
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    hkask_types::net_guard::is_private_or_reserved_ip(*ip)
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

/// Validate a tool URL with the strict config, synchronously, literal-IPs only.
///
/// For contexts that cannot await — e.g. a reqwest redirect-policy callback
/// that must re-run the destination checks on every redirect hop. This covers
/// scheme, embedded credentials, and literal-IP destinations; it does NOT
/// resolve hostnames. Any caller using it to gate redirects MUST pair it with
/// a connect-time validating DNS resolver
/// ([`validate_resolved_addresses`]) so DNS-named redirect targets cannot
/// reach forbidden addresses; literal-IP redirect targets are fully covered
/// here because no resolution occurs for them.
#[must_use = "result must be used"]
pub fn validate_tool_url_literal(url: &str) -> Result<(), McpToolError> {
    validate_url(url, &UrlValidationConfig::default())
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

/// Strictly validate a resolved address list for one hostname.
///
/// For connect-time DNS resolvers that gate the exact destination a
/// connection will use: the addresses passed here ARE the addresses the
/// caller connects to, so this closes the resolve-to-connect (DNS-rebinding)
/// gap that pre-validation alone cannot.
#[must_use = "result must be used"]
pub fn validate_resolved_addresses(
    hostname: &str,
    addresses: &[SocketAddr],
) -> Result<(), McpToolError> {
    validate_resolved_addresses_core(hostname, addresses, &UrlValidationConfig::default())
        .map_err(|e| McpToolError::invalid_argument(format!("URL validation failed: {e}")))
}

#[cfg(feature = "http-guard")]
pub mod http_guard {
    //! Connect-time DNS validation and bounded response readers for `reqwest`
    //! clients, closing DNS-rebinding TOCTOU and unbounded-body OOM surfaces.

    use std::io;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use reqwest::dns::{Addrs, Name, Resolve, Resolving};

    use super::{UrlValidationConfig, validate_resolved_addresses_core};

    /// `reqwest::dns::Resolve` implementation that rejects non-public resolved
    /// addresses at connect time (optionally admitting loopback for local
    /// fixtures).
    #[derive(Debug, Clone, Copy)]
    pub struct ValidatingDnsResolver {
        allow_loopback: bool,
    }

    impl ValidatingDnsResolver {
        #[must_use]
        pub const fn strict() -> Self {
            Self {
                allow_loopback: false,
            }
        }

        #[must_use]
        pub const fn with_loopback(allow_loopback: bool) -> Self {
            Self { allow_loopback }
        }
    }

    impl Resolve for ValidatingDnsResolver {
        fn resolve(&self, name: Name) -> Resolving {
            let host = name.as_str().to_string();
            let config = UrlValidationConfig {
                allow_private_ips: false,
                allow_loopback: self.allow_loopback,
            };
            Box::pin(async move {
                let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                    .await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                    .collect();
                validate_resolved_addresses_core(&host, &resolved, &config).map_err(
                    |e| -> Box<dyn std::error::Error + Send + Sync> {
                        Box::new(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            e.to_string(),
                        ))
                    },
                )?;
                let addrs: Addrs = Box::new(resolved.into_iter());
                Ok(addrs)
            })
        }
    }

    /// Build an SSRF-hardened `reqwest::Client` with `.no_proxy()`, redirects
    /// disabled (`Policy::none()`), a connect-time [`ValidatingDnsResolver`],
    /// and the supplied timeout.
    pub fn build_ssrf_safe_client(
        timeout: Duration,
        allow_loopback: bool,
    ) -> Result<reqwest::Client, reqwest::Error> {
        reqwest::Client::builder()
            .no_proxy()
            .redirect_policy(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(ValidatingDnsResolver::with_loopback(
                allow_loopback,
            )))
            .timeout(timeout)
            .build()
    }

    /// Read a `reqwest::Response` body into memory up to `max_bytes`, failing
    /// before allocating unbounded buffers when `Content-Length` or streamed
    /// chunks exceed the cap.
    pub async fn read_response_bytes_bounded(
        mut resp: reqwest::Response,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        if let Some(len) = resp.content_length()
            && len > max_bytes as u64
        {
            return Err(format!(
                "response Content-Length {len} exceeds {max_bytes}-byte limit"
            ));
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("response body read failed: {e}"))?
        {
            if out.len().saturating_add(chunk.len()) > max_bytes {
                return Err(format!("response body exceeded {max_bytes}-byte limit"));
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "Untrusted URLs cannot reach private services by spelling IPv4 as IPv6" [P4]
    #[tokio::test]
    async fn strict_validation_rejects_mapped_private_destinations() {
        for address in [
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:172.16.0.1",
            "::ffff:192.168.1.1",
            "::ffff:169.254.169.254",
            "::ffff:7f00:1",
        ] {
            let url = format!("http://[{address}]/");
            assert!(
                validate_tool_url_with_dns(&url).await.is_err(),
                "strict validation admitted {url}"
            );
            assert!(validate_tool_url_permissive(&url).is_ok());
        }
        assert!(
            validate_tool_url_with_dns("https://[::ffff:8.8.8.8]/")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn mapped_ipv4_preserves_the_native_address_policy() {
        for first in 0..=255 {
            for second in [0, 16, 31, 32, 168, 254] {
                let address = format!("{first}.{second}.0.1");
                let native = validate_tool_url_with_dns(&format!("http://{address}/")).await;
                let mapped =
                    validate_tool_url_with_dns(&format!("http://[::ffff:{address}]/")).await;
                assert_eq!(
                    native.is_err(),
                    mapped.is_err(),
                    "policy differed for {address}"
                );
            }
        }
    }

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

    /// expect: Reject a URL with user-info without repeating its credentials in the MCP error. [P1]
    #[tokio::test]
    async fn tool_url_rejects_embedded_credentials_without_echoing_them() {
        let error = validate_tool_url_with_dns("https://alice:synthetic-secret@example.com/clip")
            .await
            .expect_err("embedded credentials must be refused before DNS/network");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        assert!(error.message.contains("embedded credentials"));
        assert!(!error.message.contains("alice"));
        assert!(!error.message.contains("synthetic-secret"));
    }

    #[test]
    fn parse_url_for_ssrf_rejects_embedded_credentials() {
        assert!(matches!(
            parse_url_for_ssrf("https://user:pass@example.com"),
            Err(SecurityError::EmbeddedCredentials)
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

    /// expect: "A URL that connects to loopback by spelling 'this network'
    /// must be rejected." [P4]
    #[test]
    fn strict_validation_rejects_unspecified_destination() {
        for url in [
            "http://0.0.0.0/",
            "http://0.0.0.0:6379/",
            "http://0.1.2.3/",
            "http://[::]/",
        ] {
            let strict = validate_tool_url_literal(url);
            assert!(strict.is_err(), "strict validation admitted {url}");
            assert!(
                strict.unwrap_err().message.contains("Unspecified"),
                "error must name the unspecified rejection: {url}"
            );
            // The permissive (user-curated RSS) policy is unchanged: local
            // destinations remain allowed when the user chose them.
            assert!(
                validate_tool_url_permissive(url).is_ok(),
                "permissive validation must keep allowing {url}"
            );
        }
    }

    /// expect: "Neither address family can re-spell a forbidden IPv4
    /// destination." [P4]
    #[test]
    fn strict_validation_rejects_nat64_and_compatible_respellings() {
        // NAT64 well-known prefix embedding 127.0.0.1 / 10.0.0.1.
        assert!(validate_tool_url_literal("http://[64:ff9b::7f00:1]/").is_err());
        assert!(validate_tool_url_literal("http://[64:ff9b::a00:1]/").is_err());
        // NAT64 embedding a public address stays reachable.
        assert!(validate_tool_url_literal("http://[64:ff9b::808:808]/").is_ok());
        // Deprecated IPv4-compatible form embedding loopback/private.
        assert!(validate_tool_url_literal("http://[::7f00:1]/").is_err());
        assert!(validate_tool_url_literal("http://[::a00:1]/").is_err());
        // ::1 itself is native v6 loopback, not a compatible-form embedding —
        // its rejection message must stay the loopback one.
        let error = validate_tool_url_literal("http://[::1]/").unwrap_err();
        assert!(error.message.contains("Loopback"), "::1: {error}");
    }

    /// expect: "A validating resolver rejects the exact address list a
    /// connection would use." [P4]
    #[test]
    fn validate_resolved_addresses_rejects_any_forbidden_member() {
        let public: std::net::SocketAddr = "8.8.8.8:443".parse().unwrap();
        let loopback: std::net::SocketAddr = "127.0.0.1:443".parse().unwrap();
        let private: std::net::SocketAddr = "169.254.169.254:80".parse().unwrap();

        let ok = validate_resolved_addresses("host", &[public]);
        assert!(ok.is_ok(), "a public-only resolution must pass");

        let error = validate_resolved_addresses("host", &[public, loopback]).unwrap_err();
        assert!(
            error.message.contains("host resolves to loopback"),
            "mixed resolution must name the host and address: {error}"
        );
        assert!(
            validate_resolved_addresses("host", &[private]).is_err(),
            "private resolution must be rejected"
        );
        assert!(
            validate_resolved_addresses("host", &[]).is_err(),
            "empty resolution must be rejected"
        );
    }

    /// expect: "URL authority parser differentials and IPv4 shorthands cannot
    /// bypass literal SSRF validation (SEC-06)." [P4]
    #[test]
    fn literal_validation_rejects_query_fragment_differentials_and_ipv4_shorthands() {
        for bypass in [
            "http://127.0.0.1?x",
            "http://127.0.0.1#x",
            "http://127.1/",
            "http://0x7f000001/",
            "http://2130706433/",
            "http://100.64.0.1/",
            "http://240.0.0.1/",
        ] {
            assert!(
                validate_tool_url_literal(bypass).is_err(),
                "literal SSRF validation admitted {bypass}"
            );
        }
    }
}
