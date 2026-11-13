//! Relay addresses this server will connect to, and what one connection
//! attempt found.
//!
//! Two rules hold for every relay this server contacts:
//!
//! - the address is admitted before any connection: a seed relay by
//!   [`validate_relay_address`], the local relay by
//!   [`parse_local_relay_announcement`];
//! - the attempt ends in exactly one named [`ProbeOutcome`].
//!
//! Addresses found in events (relay lists, hints in identifiers) are
//! reported and never contacted.

use std::error::Error as StdError;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::Duration;

use hkask_types::nostr_wire::LocalRelayAnnouncement;

use nostr_sdk::prelude::*;
use serde_json::{Value, json};

/// The most seed relays one `nostr_account` call contacts.
pub const MAX_SEED_RELAYS: usize = 8;

/// How long one relay has to accept a connection.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Which relay addresses a build connects to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressPolicy {
    /// `wss://` with a public host only. Every installed build.
    PublicOnly,
    /// Also `ws://` and loopback or private hosts, for the local test
    /// relays. Only a build with the `test-fixture` feature uses it.
    LoopbackAllowed,
}

impl AddressPolicy {
    /// The name `nostr_account` reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::PublicOnly => "public_wss_only",
            Self::LoopbackAllowed => "loopback_allowed",
        }
    }
}

/// The policy this build uses, fixed at compile time. A build without the
/// `test-fixture` feature refuses `ws://` and loopback addresses wherever
/// they come from, settings included.
#[cfg(not(feature = "test-fixture"))]
pub const BUILD_ADDRESS_POLICY: AddressPolicy = AddressPolicy::PublicOnly;
/// The policy this build uses: a fixture build, which may connect to the
/// local test relays.
#[cfg(feature = "test-fixture")]
pub const BUILD_ADDRESS_POLICY: AddressPolicy = AddressPolicy::LoopbackAllowed;

/// Why a relay address is not used. No variant carries the address, and no
/// message repeats it: a relay setting can hold anything that was pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayAddressError {
    /// Not a websocket relay address.
    Unparseable,
    /// The scheme is not `wss://`.
    NotWss,
    /// The host is this machine or a private network.
    NotPublic,
    /// The address carries a user name or password.
    HasCredentials,
}

impl std::fmt::Display for RelayAddressError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unparseable => "not a websocket relay address",
            Self::NotWss => "only wss:// relays are contacted",
            Self::NotPublic => "the host is this machine or a private network",
            Self::HasCredentials => "the address carries a user name or password",
        })
    }
}

impl StdError for RelayAddressError {}

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast())
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    let unique_local = first & 0xfe00 == 0xfc00;
    let link_local = first & 0xffc0 == 0xfe80;
    !(ip.is_loopback() || ip.is_unspecified() || unique_local || link_local)
        && ip.to_ipv4_mapped().is_none_or(ipv4_is_public)
}

/// Whether the literal host is outside this machine and private networks.
fn host_is_public(host: Host<&str>) -> bool {
    match host {
        Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !name.is_empty() && name != "localhost" && !name.ends_with(".localhost")
        }
        Host::Ipv4(ip) => ipv4_is_public(ip),
        Host::Ipv6(ip) => ipv6_is_public(ip),
    }
}

/// Accept a relay address only if `policy` admits it.
///
/// Under [`AddressPolicy::PublicOnly`]: `wss://`, and a host that is neither
/// `localhost` nor an IP literal in a loopback, private, link-local,
/// unspecified or broadcast range. An address with a user name or password
/// is refused under either policy.
///
/// The check is on the literal host. A public name that resolves to a
/// private address is not caught here.
pub fn validate_relay_address(
    address: &str,
    policy: AddressPolicy,
) -> Result<RelayUrl, RelayAddressError> {
    let url = RelayUrl::parse(address.trim()).map_err(|_| RelayAddressError::Unparseable)?;
    let Some(host) = url.host() else {
        return Err(RelayAddressError::Unparseable);
    };
    // `RelayUrl` exposes no user-info accessor. In a URL the user info is
    // whatever precedes an `@` in the authority, which ends at the first
    // `/`, `?` or `#`.
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
    if policy == AddressPolicy::LoopbackAllowed {
        return Ok(url);
    }
    if url.scheme() != RelayUrlScheme::Wss {
        return Err(RelayAddressError::NotWss);
    }
    if !host_is_public(host) {
        return Err(RelayAddressError::NotPublic);
    }
    Ok(url)
}

/// What one connection attempt to one relay found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The websocket connection was established.
    Connected,
    /// No connection to the host: refused, unroutable or unresolvable.
    Unreachable { reason: String },
    /// The host accepted the connection and the TLS or websocket handshake
    /// failed.
    HandshakeFailed { reason: String },
    /// Nothing was established within [`PROBE_TIMEOUT`].
    TimedOut,
    /// The attempt failed in a way the two stages above do not name.
    Failed { reason: String },
}

impl ProbeOutcome {
    /// The state name a tool reports.
    pub fn state(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Unreachable { .. } => "unreachable",
            Self::HandshakeFailed { .. } => "handshake_failed",
            Self::TimedOut => "timed_out",
            Self::Failed { .. } => "failed",
        }
    }

    /// The transport's own words for a failure, if it gave any.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Connected | Self::TimedOut => None,
            Self::Unreachable { reason }
            | Self::HandshakeFailed { reason }
            | Self::Failed { reason } => Some(reason),
        }
    }
}

/// Name a failed connection attempt.
///
/// The SDK reports a timeout by kind. For everything else the stage is read
/// from the websocket transport's error, found on the SDK error's source
/// chain: it fails the TCP stage as `Io` and everything after it (TLS, the
/// websocket upgrade) as `Ws`.
pub(crate) fn classify_failure(error: &nostr_sdk::error::Error) -> ProbeOutcome {
    if error.kind() == nostr_sdk::error::ErrorKind::Timeout {
        return ProbeOutcome::TimedOut;
    }
    let reason = error.to_string();
    let mut source: Option<&(dyn StdError + 'static)> = error.source();
    while let Some(inner) = source {
        if let Some(transport) = inner.downcast_ref::<async_wsocket::Error>() {
            return match transport {
                async_wsocket::Error::Io(_) => ProbeOutcome::Unreachable { reason },
                async_wsocket::Error::Ws(_) => ProbeOutcome::HandshakeFailed { reason },
                async_wsocket::Error::Timeout => ProbeOutcome::TimedOut,
                _ => ProbeOutcome::Failed { reason },
            };
        }
        source = inner.source();
    }
    ProbeOutcome::Failed { reason }
}

/// Try one connection to one relay and name what happened. The connection
/// is closed again before this returns.
pub async fn probe_relay(url: &RelayUrl) -> ProbeOutcome {
    let relay = Relay::new(url.clone());
    let attempt = relay.try_connect().timeout(PROBE_TIMEOUT).await;
    relay.shutdown();
    match attempt {
        Ok(()) => ProbeOutcome::Connected,
        Err(error) => classify_failure(&error),
    }
}

/// Why the local relay's announcement is not used. No variant carries the
/// announced text: the file is written by another process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalRelayError {
    /// The file could not be read.
    Unreadable,
    /// The file is not the announcement the relay server writes.
    NotAnAnnouncement,
    /// The address is not `ws://127.0.0.1:<port>`.
    NotLoopback,
}

impl std::fmt::Display for LocalRelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unreadable => "the local relay's announcement could not be read",
            Self::NotAnAnnouncement => "the local relay's announcement is not in the expected form",
            Self::NotLoopback => "the announced address is not ws://127.0.0.1:<port>",
        })
    }
}

impl StdError for LocalRelayError {}

/// The local relay, as this server finds it at the moment it is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRelay {
    /// The relay server has announced no listener: it is not running, its
    /// listener is down, or this server was not told where to look.
    NotAnnounced,
    /// The relay server announced this address.
    Announced(RelayUrl),
    /// An announcement is present and is not used.
    Unusable(LocalRelayError),
}

/// Accept the relay server's announcement only if it names
/// `ws://127.0.0.1:<port>` and nothing more.
///
/// This is the one way a loopback address is admitted in an installed build,
/// and it admits exactly one shape: the loopback IPv4 literal, a port from 1
/// to 65535, no path, no credentials. A seed relay or an address found in an
/// event never reaches this function.
///
/// Known limit: the file is trusted to have been written by the relay
/// server. Another local process that can write the data directory can point
/// this server at a different loopback port. It cannot forge events: every
/// event read is verified.
pub fn parse_local_relay_announcement(text: &str) -> Result<RelayUrl, LocalRelayError> {
    let announcement: LocalRelayAnnouncement =
        serde_json::from_str(text).map_err(|_| LocalRelayError::NotAnAnnouncement)?;
    let port = announcement
        .address
        .strip_prefix("ws://127.0.0.1:")
        .ok_or(LocalRelayError::NotLoopback)?;
    let canonical = port.bytes().all(|byte| byte.is_ascii_digit()) && !port.starts_with('0');
    if !canonical || port.parse::<u16>().is_err() {
        return Err(LocalRelayError::NotLoopback);
    }
    RelayUrl::parse(&announcement.address).map_err(|_| LocalRelayError::NotLoopback)
}

/// Read the relay server's announcement at `path`. Read at each call, not
/// once: the relay picks a new port whenever it starts.
pub fn read_local_relay(path: Option<&Path>) -> LocalRelay {
    let Some(path) = path else {
        return LocalRelay::NotAnnounced;
    };
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_local_relay_announcement(&text) {
            Ok(url) => LocalRelay::Announced(url),
            Err(error) => LocalRelay::Unusable(error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocalRelay::NotAnnounced,
        Err(_) => LocalRelay::Unusable(LocalRelayError::Unreadable),
    }
}

/// Report the local relay for `nostr_account`: whether it is announced, and
/// if so whether it answers.
pub async fn probe_local_relay(local: &LocalRelay) -> Value {
    match local {
        LocalRelay::NotAnnounced => json!({
            "state": "not_announced",
            "meaning": "the local relay server has announced no listener; reads use the seed relays only",
        }),
        LocalRelay::Unusable(error) => json!({
            "state": "unusable",
            "reason": error.to_string(),
        }),
        LocalRelay::Announced(url) => {
            let outcome = probe_relay(url).await;
            let mut report = json!({
                "address": url.to_string(),
                "state": outcome.state(),
            });
            if let Some(reason) = outcome.reason() {
                report["reason"] = json!(reason);
            }
            report
        }
    }
}

/// One seed-relay entry after validation.
enum SeedRelay {
    Usable(RelayUrl),
    Refused(RelayAddressError),
}

/// Probe the first [`MAX_SEED_RELAYS`] seed relays at once and report each
/// one, in the order configured. Returns the reports and how many entries
/// beyond the bound were not looked at.
///
/// An entry the policy refuses is reported by its position and the reason,
/// never by its text, and is not contacted.
pub async fn probe_seed_relays(
    seed_relays: &[String],
    policy: AddressPolicy,
) -> (Vec<Value>, usize) {
    let considered = &seed_relays[..seed_relays.len().min(MAX_SEED_RELAYS)];
    let not_probed = seed_relays.len() - considered.len();

    let entries: Vec<SeedRelay> = considered
        .iter()
        .map(|entry| match validate_relay_address(entry, policy) {
            Ok(url) => SeedRelay::Usable(url),
            Err(error) => SeedRelay::Refused(error),
        })
        .collect();

    let reports =
        futures::future::join_all(entries.iter().enumerate().map(|(index, entry)| async move {
            match entry {
                SeedRelay::Refused(error) => json!({
                    "position": index + 1,
                    "state": "refused",
                    "reason": error.to_string(),
                }),
                SeedRelay::Usable(url) => {
                    let outcome = probe_relay(url).await;
                    let mut report = json!({
                        "address": url.to_string(),
                        "state": outcome.state(),
                    });
                    if let Some(reason) = outcome.reason() {
                        report["reason"] = json!(reason);
                    }
                    report
                }
            }
        }))
        .await;

    (reports, not_probed)
}
