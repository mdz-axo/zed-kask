//! SSRF-safe URL and resolved-IP validation for Cashu mints and LNURL endpoints (Threat T5).
//!
//! Enforces strict public `https://` URLs with no embedded credentials,
//! rejecting loopback, RFC1918 private networks, link-local / cloud metadata
//! (`169.254.169.254`), CGNAT (`100.64.0.0/10`), IPv6 ULA/link-local, and
//! IPv4-mapped IPv6 addresses targeting non-public IPv4 ranges.
//!
//! Loopback (`http://127.0.0.1:<port>`, `http://localhost:<port>`,
//! `http://[::1]:<port>`) is permitted **only** when `allow_loopback` is
//! explicitly `true` (regtest or test fixtures).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use thiserror::Error;
use url::Url;

/// A URL that has passed the D110 SSRF perimeter check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValidatedUrl {
    normalized: String,
    host: String,
    port: u16,
    is_loopback: bool,
}

impl ValidatedUrl {
    pub fn as_str(&self) -> &str {
        &self.normalized
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_loopback(&self) -> bool {
        self.is_loopback
    }
}

/// Typed refusal reasons from URL/IP SSRF validation. Never echoes credentials.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UrlError {
    #[error("URL is empty or contains control characters")]
    EmptyOrControlChars,
    #[error("URL could not be parsed")]
    InvalidUrl,
    #[error("URL must not contain embedded userinfo credentials")]
    EmbeddedCredentials,
    #[error("URL scheme must be https (http loopback is permitted only in regtest)")]
    DisallowedScheme,
    #[error("URL has no host")]
    MissingHost,
    #[error("loopback URL requires an explicit non-zero port")]
    LoopbackRequiresPort,
    #[error("URL host is not a public routable address")]
    NonPublicHost,
    #[error("resolved IP address is not publicly routable")]
    NonPublicIp,
}

fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    let is_cgnat = octets[0] == 100 && (octets[1] & 0b1100_0000) == 64;
    let is_benchmarking = octets[0] == 198 && (octets[1] == 18 || octets[1] == 19);
    let is_documentation = (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113);
    let is_reserved_high = octets[0] >= 240;

    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || is_cgnat
        || is_benchmarking
        || is_documentation
        || is_reserved_high
}

fn is_non_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_non_public_ipv4(mapped);
    }
    let segs = ip.segments();
    let is_ula = (segs[0] & 0xfe00) == 0xfc00;
    let is_link_local = (segs[0] & 0xffc0) == 0xfe80;
    let is_documentation = segs[0] == 0x2001 && segs[1] == 0x0db8;

    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || is_ula
        || is_link_local
        || is_documentation
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// Validate a DNS-resolved IP address before opening a TCP/TLS connection.
pub fn validate_resolved_ip(ip: IpAddr, allow_loopback: bool) -> Result<(), UrlError> {
    if allow_loopback && is_loopback_ip(ip) {
        return Ok(());
    }
    let blocked = match ip {
        IpAddr::V4(v4) => is_non_public_ipv4(v4),
        IpAddr::V6(v6) => is_non_public_ipv6(v6),
    };
    if blocked {
        return Err(UrlError::NonPublicIp);
    }
    Ok(())
}

fn is_loopback_hostname(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
}

fn is_disallowed_local_hostname(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".localdomain")
        || !host.contains('.')
}

/// Validate an external Cashu mint or LNURL URL under the D110 SSRF perimeter.
pub fn validate_public_https_url(
    raw: &str,
    allow_loopback: bool,
) -> Result<ValidatedUrl, UrlError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control()) {
        return Err(UrlError::EmptyOrControlChars);
    }

    let parsed = Url::parse(trimmed).map_err(|_| UrlError::InvalidUrl)?;

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(UrlError::EmbeddedCredentials);
    }

    let scheme = parsed.scheme();
    let host_raw = parsed.host_str().ok_or(UrlError::MissingHost)?;
    let host = host_raw.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(UrlError::MissingHost);
    }

    // Strip IPv6 brackets for IpAddr parsing.
    let host_unbracketed = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&host);

    if allow_loopback && is_loopback_hostname(&host) {
        if scheme != "http" && scheme != "https" {
            return Err(UrlError::DisallowedScheme);
        }
        let port = parsed.port().ok_or(UrlError::LoopbackRequiresPort)?;
        if port == 0 {
            return Err(UrlError::LoopbackRequiresPort);
        }
        return Ok(ValidatedUrl {
            normalized: parsed.to_string().trim_end_matches('/').to_string(),
            host: host_unbracketed.to_string(),
            port,
            is_loopback: true,
        });
    }

    if scheme != "https" {
        return Err(UrlError::DisallowedScheme);
    }

    if let Ok(ip) = host_unbracketed.parse::<IpAddr>() {
        validate_resolved_ip(ip, false).map_err(|_| UrlError::NonPublicHost)?;
    } else if is_disallowed_local_hostname(&host) {
        return Err(UrlError::NonPublicHost);
    }

    let port = parsed.port_or_known_default().unwrap_or(443);

    Ok(ValidatedUrl {
        normalized: parsed.to_string().trim_end_matches('/').to_string(),
        host: host_unbracketed.to_string(),
        port,
        is_loopback: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_https_url_is_accepted_and_normalized() {
        let ok = validate_public_https_url("https://mint.example.com/", false)
            .expect("public https URL accepted");
        assert_eq!(ok.as_str(), "https://mint.example.com");
        assert_eq!(ok.host(), "mint.example.com");
        assert_eq!(ok.port(), 443);
        assert!(!ok.is_loopback());
    }

    #[test]
    fn ssrf_targets_are_rejected_in_public_mode() {
        let blocked = [
            "http://mint.example.com",
            "https://127.0.0.1:3338",
            "https://localhost:3338",
            "https://sub.localhost/mint",
            "https://169.254.169.254/latest/meta-data/",
            "https://10.0.0.1/mint",
            "https://172.16.0.1/mint",
            "https://192.168.1.1/mint",
            "https://100.64.0.1/mint",
            "https://[::1]:3338",
            "https://[fc00::1]/mint",
            "https://[fe80::1]/mint",
            "https://[::ffff:127.0.0.1]/mint",
            "https://[::ffff:169.254.169.254]/mint",
            "https://user:pass@mint.example.com",
            "https://internal-host/mint",
            "https://service.local/mint",
            "https://metadata.google.internal/mint",
        ];
        for url in blocked {
            assert!(
                validate_public_https_url(url, false).is_err(),
                "expected SSRF target to be rejected: {url}"
            );
        }
    }

    #[test]
    fn regtest_loopback_mode_allows_explicit_port_loopback_only() {
        let ok = validate_public_https_url("http://127.0.0.1:3338", true)
            .expect("loopback with port allowed in regtest");
        assert!(ok.is_loopback());
        assert_eq!(ok.port(), 3338);

        assert_eq!(
            validate_public_https_url("http://127.0.0.1", true),
            Err(UrlError::LoopbackRequiresPort)
        );
        // Non-loopback private/metadata IPs are STILL blocked in regtest mode.
        assert!(validate_public_https_url("https://169.254.169.254/latest", true).is_err());
        assert!(validate_public_https_url("https://10.0.0.5/mint", true).is_err());
        assert!(validate_public_https_url("http://mint.example.com", true).is_err());
    }

    #[test]
    fn resolved_ip_check_blocks_private_and_metadata_ips() {
        assert!(validate_resolved_ip("1.1.1.1".parse().expect("ip"), false).is_ok());
        assert!(validate_resolved_ip("169.254.169.254".parse().expect("ip"), false).is_err());
        assert!(validate_resolved_ip("127.0.0.1".parse().expect("ip"), false).is_err());
        assert!(validate_resolved_ip("127.0.0.1".parse().expect("ip"), true).is_ok());
        assert!(validate_resolved_ip("::ffff:10.0.0.1".parse().expect("ip"), true).is_err());
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn rfc1918_link_local_loopback_and_cgnat_are_always_blocked_in_public_mode(
                b in any::<u8>(),
                c in any::<u8>(),
                d in any::<u8>(),
            ) {
                let blocked_v4 = [
                    Ipv4Addr::new(10, b, c, d),
                    Ipv4Addr::new(127, b, c, d),
                    Ipv4Addr::new(169, 254, c, d),
                    Ipv4Addr::new(192, 168, c, d),
                    Ipv4Addr::new(172, 16 + (b % 16), c, d),
                    Ipv4Addr::new(100, 64 + (b % 64), c, d),
                ];
                for v4 in blocked_v4 {
                    prop_assert!(validate_resolved_ip(IpAddr::V4(v4), false).is_err());
                    let mapped = v4.to_ipv6_mapped();
                    prop_assert!(validate_resolved_ip(IpAddr::V6(mapped), false).is_err());
                }
            }
        }
    }
}
