//! Canonical IP, hostname, and host classification for SSRF defense.
//!
//! Single source of truth shared across `hkask-mcp-server`, `hkask-wallet-core`,
//! `kask_bridge`, and every network-reaching MCP server so address blocklists
//! cannot drift between crates.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Host;

/// Extract an embedded IPv4 address from an IPv6 representation, if any.
///
/// Covers:
/// - IPv4-mapped (`::ffff:a.b.c.d`)
/// - Deprecated IPv4-compatible (`::a.b.c.d`, excluding `::` and `::1`)
/// - NAT64 well-known prefix (`64:ff9b::/96`)
#[must_use]
pub fn embedded_ipv4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(mapped) = v6.to_ipv4_mapped() {
        return Some(mapped);
    }
    let segs = v6.segments();
    let is_compatible =
        segs[0..6].iter().all(|&s| s == 0) && !v6.is_unspecified() && !v6.is_loopback();
    let is_nat64 = segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..6].iter().all(|&s| s == 0);
    if is_compatible || is_nat64 {
        Some(Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            segs[6] as u8,
            (segs[7] >> 8) as u8,
            segs[7] as u8,
        ))
    } else {
        None
    }
}

/// Whether `ip` is a loopback destination (IPv4 `127.0.0.0/8`, IPv6 `::1`, or
/// IPv6-embedded IPv4 loopback).
#[must_use]
pub fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || embedded_ipv4(v6).is_some_and(|v4| v4.is_loopback()),
    }
}

/// Whether `ip` is an unspecified / "this network" destination (`0.0.0.0/8`,
/// `::`, or IPv6-embedded `0.0.0.0/8`).
#[must_use]
pub fn is_unspecified_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.octets()[0] == 0,
        IpAddr::V6(v6) => {
            v6.is_unspecified() || embedded_ipv4(v6).is_some_and(|v4| v4.octets()[0] == 0)
        }
    }
}

/// Whether `ip` falls in any non-publicly-routable IPv4 range:
/// `0.0.0.0/8`, loopback, RFC1918 private, link-local (`169.254/16`),
/// CGNAT (`100.64/10`), IETF protocol assignments (`192.0.0/24`),
/// documentation (`192.0.2/24`, `198.51.100/24`, `203.0.113/24`),
/// benchmarking (`198.18/15`), multicast (`224/4`), reserved/broadcast (`240/4`).
#[must_use]
pub fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let is_this_network = o[0] == 0;
    let is_cgnat = o[0] == 100 && (o[1] & 0b1100_0000) == 64;
    let is_ietf_protocol = o[0] == 192 && o[1] == 0 && o[2] == 0;
    let is_benchmarking = o[0] == 198 && (o[1] & 0xfe) == 18;
    let is_documentation = (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113);
    let is_reserved = o[0] >= 240;

    is_this_network
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || is_cgnat
        || is_ietf_protocol
        || is_benchmarking
        || is_documentation
        || is_reserved
}

/// Whether `ip` uses the NAT64 well-known prefix (`64:ff9b::/96`).
#[must_use]
pub fn is_nat64_ipv6(ip: Ipv6Addr) -> bool {
    let segs = ip.segments();
    segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..6].iter().all(|&s| s == 0)
}

/// Whether `ip` falls in any non-publicly-routable IPv6 range:
/// loopback (`::1`), unspecified (`::`), multicast (`ff00::/8`), ULA (`fc00::/7`),
/// link-local (`fe80::/10`), documentation (`2001:db8::/32`), 6to4 (`2002::/16`),
/// or any IPv4-mapped/compatible/NAT64 embedding a non-public IPv4.
#[must_use]
pub fn is_non_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_non_public_ipv4(v4);
    }
    let segs = ip.segments();
    let is_ula = (segs[0] & 0xfe00) == 0xfc00;
    let is_link_local = (segs[0] & 0xffc0) == 0xfe80;
    let is_documentation = segs[0] == 0x2001 && segs[1] == 0x0db8;
    let is_6to4 = segs[0] == 0x2002;

    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || is_ula
        || is_link_local
        || is_documentation
        || is_6to4
        || embedded_ipv4(ip).is_some_and(is_non_public_ipv4)
}

/// Whether `ip` is a publicly routable IPv4 address.
#[must_use]
pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !is_non_public_ipv4(ip)
}

/// Whether `ip` is a publicly routable IPv6 address.
#[must_use]
pub fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    !is_non_public_ipv6(ip)
}

/// Whether `ip` is a publicly routable IP address.
#[must_use]
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

/// Whether `ip` is non-public *excluding* loopback and unspecified (used when
/// callers gate loopback and unspecified with distinct error variants).
#[must_use]
pub fn is_private_or_reserved_ip(ip: IpAddr) -> bool {
    !is_public_ip(ip) && !is_loopback_ip(ip) && !is_unspecified_ip(ip)
}

/// Normalize a hostname by trimming trailing dots, stripping optional IPv6
/// brackets, and lowercasing ASCII.
#[must_use]
pub fn normalize_host(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&trimmed)
        .to_string()
}

/// Whether `host` names a loopback destination (`localhost`, `*.localhost`, or
/// any loopback IP literal such as `127.0.0.1`, `::1`, `[::1]`).
#[must_use]
pub fn is_loopback_hostname(host: &str) -> bool {
    let norm = normalize_host(host);
    if norm == "localhost" || norm.ends_with(".localhost") {
        return true;
    }
    norm.parse::<IpAddr>().is_ok_and(is_loopback_ip)
}

/// Whether a DNS hostname is outside reserved local/internal suffixes
/// (`localhost`, `local`, `internal`, `localdomain`, `home.arpa`, `lan`).
#[must_use]
pub fn is_public_hostname(host: &str) -> bool {
    let norm = normalize_host(host);
    if norm.is_empty() {
        return false;
    }
    for suffix in [
        "localhost",
        "local",
        "internal",
        "localdomain",
        "home.arpa",
        "lan",
    ] {
        if norm == suffix || norm.ends_with(&format!(".{suffix}")) {
            return false;
        }
    }
    true
}

/// Like [`is_public_hostname`], additionally rejecting single-label hostnames
/// without a dot (for strict public HTTPS/LNURL/mint endpoints).
#[must_use]
pub fn is_public_fqdn(host: &str) -> bool {
    let norm = normalize_host(host);
    is_public_hostname(&norm) && norm.contains('.')
}

/// Whether a parsed `url::Host` is publicly routable (literal IP or public domain).
#[must_use]
pub fn is_public_host(host: Host<&str>) -> bool {
    match host {
        Host::Domain(name) => is_public_hostname(name),
        Host::Ipv4(v4) => is_public_ipv4(v4),
        Host::Ipv6(v6) => is_public_ipv6(v6),
    }
}

/// Whether a parsed `url::Host` is loopback.
#[must_use]
pub fn is_loopback_host(host: Host<&str>) -> bool {
    match host {
        Host::Domain(name) => {
            let norm = normalize_host(name);
            norm == "localhost" || norm.ends_with(".localhost")
        }
        Host::Ipv4(v4) => v4.is_loopback(),
        Host::Ipv6(v6) => is_loopback_ip(IpAddr::V6(v6)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_blocks_all_non_public_ranges() {
        for blocked in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            let ip: IpAddr = blocked.parse().unwrap();
            assert!(!is_public_ip(ip), "{blocked} must be non-public");
        }
        for public in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "172.32.0.1"] {
            let ip: IpAddr = public.parse().unwrap();
            assert!(is_public_ip(ip), "{public} must be public");
        }
    }

    #[test]
    fn ipv6_blocks_ula_link_local_nat64_and_mapped_private() {
        for blocked in [
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "2001:db8::1",
            "2002:7f00:0001::1",
            "64:ff9b::127.0.0.1",
            "64:ff9b::169.254.169.254",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:100.64.0.1",
            "::127.0.0.1",
            "ff02::1",
        ] {
            let ip: IpAddr = blocked.parse().unwrap();
            assert!(!is_public_ip(ip), "{blocked} must be non-public");
        }
        for public in [
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            let ip: IpAddr = public.parse().unwrap();
            assert!(is_public_ip(ip), "{public} must be public");
        }
    }

    #[test]
    fn hostnames_reject_local_and_internal_suffixes() {
        for bad in [
            "",
            "localhost",
            "LOCALHOST.",
            "api.localhost",
            "printer.local",
            "metadata.internal",
            "box.localdomain",
            "router.home.arpa",
            "nas.lan",
        ] {
            assert!(!is_public_hostname(bad), "{bad} must be rejected");
        }
        assert!(is_public_hostname("example.com"));
        assert!(is_public_fqdn("example.com"));
        assert!(!is_public_fqdn("intranet"));
    }
}
