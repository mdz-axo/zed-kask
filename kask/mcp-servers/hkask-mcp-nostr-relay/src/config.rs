//! Operator configuration, read once at startup.
//!
//! The agent cannot change any of this: there is no tool that writes it.
//! A value that does not parse is never guessed at. It is left out and named
//! in `warnings`, which `nostr_relay_status` reports.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use nostr_sdk::prelude::{PublicKey, RelayUrl, RelayUrlScheme};
use url::Host;

/// What the server runs with.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Loopback port for the relay listener. 0 lets the OS pick a free one.
    pub port: u16,
    /// Authors whose events are admitted. Empty admits nothing.
    pub authors: BTreeSet<PublicKey>,
    /// Public relays `nostr_relay_sync` reads from.
    pub sync_relays: Vec<RelayUrl>,
    /// Configuration that was present and could not be used.
    pub warnings: Vec<String>,
}

/// Why a relay address is not used for sync.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayAddressError {
    #[error("not a relay address")]
    Unparseable,
    #[error("only wss:// relays are used for sync")]
    NotWss,
    #[error("the host is this machine or a private network")]
    NotPublic,
    #[error("the address carries a user name or password")]
    HasCredentials,
}

/// Whether `raw` looks like a bech32 Nostr secret key (`nsec1` or `ncryptsec1`,
/// with or without a `nostr:` URI prefix, case-insensitive).
pub fn is_secret_key_like(raw: &str) -> bool {
    let trimmed = raw.trim();
    let stripped = match trimmed.get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case("nostr:") => trimmed[6..].trim(),
        _ => trimmed,
    };
    let lower = stripped.to_ascii_lowercase();
    lower.starts_with("nsec1")
        || lower.starts_with("ncryptsec1")
        || lower.contains("nsec1")
        || lower.contains("ncryptsec1")
}

/// Scrub secret-key material (`nsec1...` / `ncryptsec1...`) and URL userinfo
/// credentials before formatting an invalid input into a warning or error.
pub fn redact_sensitive_input(raw: &str) -> String {
    if is_secret_key_like(raw) {
        return "<redacted nsec>".to_string();
    }
    let trimmed = raw.trim();
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let tail = &rest[authority_end..];
        if let Some((_userinfo, host_part)) = authority.rsplit_once('@') {
            return format!("{scheme}://<redacted>@{host_part}{tail}");
        }
    }
    trimmed.to_string()
}

fn entries(raw: Option<&str>) -> impl Iterator<Item = &str> {
    raw.unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
}

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    let is_cgnat = octets[0] == 100 && (octets[1] & 0xc0) == 64;
    let is_ietf_protocol = octets[0] == 192 && octets[1] == 0 && octets[2] == 0;
    let is_benchmarking = octets[0] == 198 && (octets[1] & 0xfe) == 18;
    let is_reserved = octets[0] >= 240;
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || is_cgnat
        || is_ietf_protocol
        || is_benchmarking
        || is_reserved)
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let first = segments[0];
    let unique_local = first & 0xfe00 == 0xfc00;
    let link_local = first & 0xffc0 == 0xfe80;
    let is_6to4 = first == 0x2002;
    let is_nat64 = first == 0x0064 && segments[1] == 0xff9b;
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || unique_local
        || link_local
        || is_6to4
        || is_nat64)
        && ip.to_ipv4_mapped().is_none_or(ipv4_is_public)
}

fn domain_is_public(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() {
        return false;
    }
    for suffix in ["localhost", "local", "internal", "home.arpa", "lan"] {
        if name == suffix || name.ends_with(&format!(".{suffix}")) {
            return false;
        }
    }
    true
}

/// Accept a relay address for sync only if it is `wss://`, carries no
/// credentials, and its host is not this machine or a private network.
///
/// The check is on the literal host. A public name that resolves to a
/// private address is not caught here.
pub fn validate_sync_relay(raw: &str) -> Result<RelayUrl, RelayAddressError> {
    let url = RelayUrl::parse(raw.trim()).map_err(|_| RelayAddressError::Unparseable)?;
    let text = url.as_str();
    let authority = text
        .split_once("://")
        .map_or(text, |(_scheme, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return Err(RelayAddressError::HasCredentials);
    }
    if url.scheme() != RelayUrlScheme::Wss {
        return Err(RelayAddressError::NotWss);
    }
    let public = match url.host() {
        Some(Host::Domain(name)) => domain_is_public(name),
        Some(Host::Ipv4(ip)) => ipv4_is_public(ip),
        Some(Host::Ipv6(ip)) => ipv6_is_public(ip),
        None => false,
    };
    if !public {
        return Err(RelayAddressError::NotPublic);
    }
    Ok(url)
}

impl Config {
    /// Parse the three settings. Pure, so every rule is tested without
    /// touching the process environment.
    pub fn from_values(
        port: Option<&str>,
        authors: Option<&str>,
        sync_relays: Option<&str>,
    ) -> Self {
        let mut config = Config::default();

        if let Some(raw) = port.map(str::trim).filter(|raw| !raw.is_empty()) {
            match raw.parse::<u16>() {
                Ok(port) => config.port = port,
                Err(_) => {
                    let safe = redact_sensitive_input(raw);
                    config.warnings.push(format!(
                        "port {safe:?} is not a number from 0 to 65535; a free port was picked instead"
                    ));
                }
            }
        }

        for entry in entries(authors) {
            if is_secret_key_like(entry) {
                config.warnings.push(
                    "author \"<redacted nsec>\" is a secret key, not a public npub or hex public key; it was not added to the allowlist"
                        .to_string(),
                );
                continue;
            }
            match PublicKey::parse(entry) {
                Ok(author) => {
                    config.authors.insert(author);
                }
                Err(_) => {
                    let safe = redact_sensitive_input(entry);
                    config.warnings.push(format!(
                        "author {safe:?} is not an npub or a hex public key; it was not added to the allowlist"
                    ));
                }
            }
        }

        for entry in entries(sync_relays) {
            match validate_sync_relay(entry) {
                Ok(url) => {
                    if !config.sync_relays.contains(&url) {
                        config.sync_relays.push(url);
                    }
                }
                Err(reason) => {
                    let safe = redact_sensitive_input(entry);
                    config
                        .warnings
                        .push(format!("sync relay {safe:?} was not used: {reason}"));
                }
            }
        }

        config
    }

    /// Read the settings the editor passes to this process.
    pub fn from_env() -> Self {
        let port = std::env::var("HKASK_NOSTR_RELAY_PORT").ok();
        let authors = std::env::var("HKASK_NOSTR_RELAY_AUTHORS").ok();
        let sync_relays = std::env::var("HKASK_NOSTR_RELAY_SYNC_RELAYS").ok();
        Self::from_values(port.as_deref(), authors.as_deref(), sync_relays.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NIP-19's published example pair.
    const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";
    const HEX: &str = "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";

    #[test]
    fn nothing_configured_means_an_empty_allowlist_and_an_os_picked_port() {
        let config = Config::from_values(None, None, None);
        assert_eq!(config.port, 0);
        assert!(config.authors.is_empty());
        assert!(config.sync_relays.is_empty());
        assert!(config.warnings.is_empty());
    }

    /// The editor emits all three variables for every settings state. With
    /// nothing configured that is port "0" and two empty strings, which must
    /// read as "not set" and raise no warning.
    #[test]
    fn the_form_unset_settings_arrive_in_reads_as_nothing_configured() {
        let config = Config::from_values(Some("0"), Some(""), Some(""));
        assert_eq!(config.port, 0);
        assert!(config.authors.is_empty());
        assert!(config.sync_relays.is_empty());
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    }

    #[test]
    fn an_npub_and_its_hex_form_are_one_author() {
        let config = Config::from_values(None, Some(&format!("{NPUB}, {HEX}")), None);
        assert_eq!(config.authors.len(), 1);
        assert_eq!(
            config
                .authors
                .iter()
                .next()
                .map(PublicKey::to_hex)
                .as_deref(),
            Some(HEX)
        );
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn an_unparseable_author_is_left_out_and_named() {
        let config = Config::from_values(None, Some(&format!("{NPUB} not-a-key")), None);
        assert_eq!(config.authors.len(), 1);
        assert_eq!(config.warnings.len(), 1);
        assert!(config.warnings[0].contains("not-a-key"));
    }

    #[test]
    fn an_unparseable_port_falls_back_to_an_os_picked_port_and_is_named() {
        let config = Config::from_values(Some("70000"), None, None);
        assert_eq!(config.port, 0);
        assert!(config.warnings[0].contains("70000"));
        assert_eq!(Config::from_values(Some(" 7447 "), None, None).port, 7447);
    }

    #[test]
    fn a_public_wss_relay_is_accepted_once() {
        let config = Config::from_values(None, None, Some("wss://nos.lol wss://nos.lol/"));
        assert_eq!(config.sync_relays.len(), 1);
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn local_private_and_plaintext_relays_are_refused() {
        for relay in [
            "ws://relay.example.com",
            "wss://localhost",
            "wss://app.localhost:8443",
            "wss://127.0.0.1",
            "wss://10.1.2.3",
            "wss://192.168.1.10:7777",
            "wss://172.16.0.1",
            "wss://169.254.1.1",
            "wss://0.0.0.0",
            "wss://[::1]",
            "wss://[fe80::1]",
            "wss://[fd00::1]",
            "wss://[::ffff:127.0.0.1]",
            "https://relay.example.com",
            "not a url",
        ] {
            assert!(
                validate_sync_relay(relay).is_err(),
                "{relay} must be refused"
            );
        }
        let config = Config::from_values(None, None, Some("wss://127.0.0.1 wss://nos.lol"));
        assert_eq!(config.sync_relays.len(), 1);
        assert_eq!(config.warnings.len(), 1);
    }

    #[test]
    fn a_public_ip_literal_is_accepted() {
        assert!(validate_sync_relay("wss://93.184.216.34").is_ok());
        assert!(validate_sync_relay("wss://[2606:2800:220:1::1]").is_ok());
    }
}
