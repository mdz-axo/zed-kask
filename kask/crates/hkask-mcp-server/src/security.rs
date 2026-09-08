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

/// Check one literal or resolved destination address against the config.
///
/// IPv6 addresses that embed an IPv4 destination (the deprecated
/// IPv4-compatible form and the NAT64 well-known prefix 64:ff9b::/96) are
/// unmasked and the embedded IPv4 destination is checked as IPv4, so neither
/// address family can re-spell a forbidden destination as the other.
/// `to_canonical` already unmaps the IPv4-mapped form (::ffff:a.b.c.d) before
/// this runs. Native IPv6 categories (loopback, ULA, link-local, unspecified)
/// are checked on the untranslated address first so their messages stay exact.
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
            if !config.allow_private_ips && is_private_ip(&IpAddr::V6(v6)) {
                return Err(SecurityError::PrivateIpNotAllowed(v6.to_string()));
            }
            if let Some(embedded) = embedded_ipv4(v6) {
                policy_check_ipv4(embedded, config)?;
            }
            Ok(())
        }
    }
}

fn policy_check_ipv4(
    v4: std::net::Ipv4Addr,
    config: &UrlValidationConfig,
) -> Result<(), SecurityError> {
    // 0.0.0.0/8 ("this network") is never a real destination, and on Linux
    // connecting to 0.0.0.0 routes to loopback — it re-enters exactly the
    // services this gate exists to protect. Gated by the private-IP allowance
    // so the permissive (user-curated RSS) policy is unchanged.
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

/// The IPv4 destination embedded in a non-mapped IPv6 address, if any.
///
/// Covers the deprecated IPv4-compatible form (::a.b.c.d, all of the first
/// six segments zero, excluding unspecified `::`) and the NAT64 well-known
/// prefix (64:ff9b::/96). The IPv4-mapped form is handled earlier by
/// `to_canonical`.
fn embedded_ipv4(v6: std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    let segments = v6.segments();
    let compatible =
        v6.segments()[0..6].iter().all(|&segment| segment == 0) && !v6.is_unspecified();
    let nat64 = segments[0] == 0x64
        && segments[1] == 0xff9b
        && segments[2..6].iter().all(|&segment| segment == 0);
    if compatible || nat64 {
        Some(std::net::Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        ))
    } else {
        None
    }
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

    validate_resolved_addresses_core(hostname, &resolved, config)
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
    match ip.to_canonical() {
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
}
