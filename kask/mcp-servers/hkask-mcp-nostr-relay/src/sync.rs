//! Fill the archive from public relays.
//!
//! `LocalRelay::sync_with` cannot do this: it fetches into a client that has
//! no handle on the relay's store, so nothing is saved. A client that does
//! share the store saves everything it receives and skips the admission
//! rule. So sync here is three explicit steps: fetch into a client with no
//! handle on the store, admit each event through the one [`Gate`], and store
//! the admitted ones with `LocalRelay::add_event`, which verifies the event's
//! id and signature first.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use nostr_sdk::prelude::*;
use serde::Serialize;

use crate::admission::{Gate, PATH_SYNC};

/// How long to wait for the relays to connect.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for the stored events.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// The most events one sync asks for.
pub const MAX_EVENTS_PER_SYNC: usize = 5_000;

/// Why a sync could not run at all.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the author allowlist is empty, so there is nothing to sync")]
    NoAuthors,
    #[error("no sync relay is configured")]
    NoRelays,
    #[error("no sync relay could be reached: {0}")]
    NoRelayReachable(String),
    #[error("fetching events failed: {0}")]
    Fetch(String),
}

/// What one sync did.
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    /// Each relay asked, and `connected` or the reason it was not used.
    pub relays: BTreeMap<String, String>,
    /// Events the relays returned.
    pub fetched: usize,
    /// Events the admission rule accepted.
    pub admitted: usize,
    /// Events the admission rule refused.
    pub refused: usize,
    /// Admitted events that were new to the archive.
    pub stored: usize,
    /// Admitted events the archive did not store (already present, replaced
    /// by a newer version, deleted, or expired).
    pub not_stored: usize,
    /// Events that failed id or signature verification.
    pub invalid: usize,
    /// True when the fetch hit [`MAX_EVENTS_PER_SYNC`]; older events may
    /// remain on the relays.
    pub truncated: bool,
}

/// How long one sync waits on the network.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// For the relays to connect.
    pub connect: Duration,
    /// For the connected relays to return their stored events.
    pub fetch: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            fetch: FETCH_TIMEOUT,
        }
    }
}

/// Fetch the allowlisted authors' events from `relays` and store the
/// admitted ones through `relay`.
pub async fn run(
    relay: &LocalRelay,
    gate: &Gate,
    relays: &[RelayUrl],
) -> Result<SyncReport, SyncError> {
    run_with(relay, gate, relays, Timeouts::default()).await
}

fn is_public_sync_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => hkask_types::net_guard::is_public_ipv4(v4),
        IpAddr::V6(v6) => {
            hkask_types::net_guard::is_public_ipv6(v6) && !hkask_types::net_guard::is_nat64_ipv6(v6)
        }
    }
}

/// Verify that a DNS lookup returned at least one address and that every
/// resolved address is publicly routable (closing the literal-host-only gap
/// in `config::validate_sync_relay` for static/cached private-IP DNS records).
pub(crate) fn check_resolved_ips(host: &str, ips: &[IpAddr]) -> Result<(), String> {
    if ips.is_empty() {
        return Err(format!("refused: DNS returned no addresses for {host}"));
    }
    for &ip in ips {
        if !is_public_sync_ip(ip) {
            return Err(format!(
                "refused: {host} resolved to non-public address {ip}"
            ));
        }
    }
    Ok(())
}

async fn check_public_dns_with<F, Fut>(
    url: &RelayUrl,
    timeout: Duration,
    resolve: F,
) -> Result<(), String>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: Future<Output = std::io::Result<Vec<IpAddr>>>,
{
    if url.scheme() != RelayUrlScheme::Wss {
        return Ok(());
    }
    let Some(host) = url.host() else {
        return Err("refused: relay URL has no host".to_string());
    };
    match host {
        url::Host::Ipv4(v4) => check_resolved_ips(&v4.to_string(), &[IpAddr::V4(v4)]),
        url::Host::Ipv6(v6) => check_resolved_ips(&v6.to_string(), &[IpAddr::V6(v6)]),
        url::Host::Domain(domain) => {
            let ips = tokio::time::timeout(timeout, resolve(domain.to_string(), 0))
                .await
                .map_err(|_| format!("refused: DNS lookup timed out for {domain}"))?
                .map_err(|error| format!("refused: DNS lookup failed for {domain}: {error}"))?;
            check_resolved_ips(domain, &ips)
        }
    }
}

async fn resolve_system_dns(host: String, port: u16) -> std::io::Result<Vec<IpAddr>> {
    let addrs = tokio::net::lookup_host((host.as_str(), port)).await?;
    Ok(addrs.map(|addr| addr.ip()).collect())
}

/// [`run`] with explicit timeouts.
pub async fn run_with(
    relay: &LocalRelay,
    gate: &Gate,
    relays: &[RelayUrl],
    timeouts: Timeouts,
) -> Result<SyncReport, SyncError> {
    if gate.authors().is_empty() {
        return Err(SyncError::NoAuthors);
    }
    if relays.is_empty() {
        return Err(SyncError::NoRelays);
    }

    let mut report = SyncReport::default();

    // No `.database(...)`: this client must not write to the archive.
    let client = Client::default();
    let mut added = 0usize;
    for url in relays {
        if let Err(reason) = check_public_dns_with(url, timeouts.connect, resolve_system_dns).await
        {
            report.relays.insert(url.to_string(), reason);
            continue;
        }
        match client.add_relay(url.clone()).await {
            Ok(_) => added += 1,
            Err(error) => {
                report
                    .relays
                    .insert(url.to_string(), format!("not added: {error}"));
            }
        }
    }
    if added == 0 {
        client.shutdown().await;
        let detail = report
            .relays
            .iter()
            .map(|(url, state)| format!("{url} {state}"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(SyncError::NoRelayReachable(detail));
    }
    let connected = client.try_connect().timeout(timeouts.connect).await;
    for url in connected.success.keys() {
        report
            .relays
            .insert(url.to_string(), "connected".to_string());
    }
    for (url, reason) in &connected.failed {
        report
            .relays
            .insert(url.to_string(), format!("unreachable: {reason}"));
    }
    if connected.success.is_empty() {
        client.shutdown().await;
        let detail = report
            .relays
            .iter()
            .map(|(url, state)| format!("{url} {state}"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(SyncError::NoRelayReachable(detail));
    }

    // The filter limits what is transferred. It is not the control: a relay
    // can answer with anything, so every event goes through the gate below.
    let filter = Filter::new()
        .authors(gate.authors().iter().copied())
        .limit(MAX_EVENTS_PER_SYNC);
    // Ask only the relays that connected. A fetch addressed to the whole
    // pool also waits on the ones that did not, for the full fetch limit
    // (observed live: one relay of three answered at once and the sync
    // still took 40 seconds).
    let targets: Vec<(RelayUrl, Vec<Filter>)> = connected
        .success
        .keys()
        .map(|url| (url.clone(), vec![filter.clone()]))
        .collect();
    let fetched = client.fetch_events(targets).timeout(timeouts.fetch).await;
    client.shutdown().await;
    let events = fetched.map_err(|error| SyncError::Fetch(error.to_string()))?;

    report.fetched = events.len();
    report.truncated = events.len() >= MAX_EVENTS_PER_SYNC;
    for event in events {
        if gate.admit(PATH_SYNC, &event).is_err() {
            report.refused += 1;
            continue;
        }
        report.admitted += 1;
        match relay.add_event(event).await {
            Ok(status) if status.is_success() => report.stored += 1,
            Ok(_) => report.not_stored += 1,
            Err(_) => report.invalid += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn resolved_public_ips_pass_and_any_non_public_ip_fails_closed() {
        let public_v4 = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        let public_v6 = IpAddr::V6("2606:2800:220:1::1".parse().expect("v6"));
        assert!(check_resolved_ips("relay.example", &[public_v4, public_v6]).is_ok());

        assert!(
            check_resolved_ips("relay.example", &[]).is_err(),
            "empty DNS answer must fail closed"
        );

        let non_public: [IpAddr; 7] = [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6("fe80::1".parse().expect("link-local v6")),
            IpAddr::V6("64:ff9b::5db8:d822".parse().expect("nat64 v6")),
        ];
        for bad in non_public {
            let err = check_resolved_ips("rebind.example", &[public_v4, bad])
                .expect_err("mixed public + non-public DNS answer must be refused");
            assert!(
                err.contains("non-public") && err.contains(&bad.to_string()),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn wss_domain_resolving_to_private_ip_is_refused_before_connect() {
        let url = RelayUrl::parse("wss://rebind.example").expect("valid wss url");
        let err = check_public_dns_with(&url, Duration::from_secs(1), |_host, _port| async {
            Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
        })
        .await
        .expect_err("private DNS resolution must be refused");
        assert!(
            err.contains("non-public") && err.contains("127.0.0.1"),
            "{err}"
        );

        let ok = check_public_dns_with(&url, Duration::from_secs(1), |_host, _port| async {
            Ok(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))])
        })
        .await;
        assert!(ok.is_ok(), "{ok:?}");
    }
}
