//! `nostr_account` probes the seed relays: the tool is called through its
//! public `Parameters<T>` seam against relays started inside the test.
//!
//! Compiled only with the `test-fixture` feature, the one build that will
//! connect to loopback. Nothing here leaves the machine, and no identity is
//! written into this file.

#![cfg(feature = "test-fixture")]

use std::sync::Arc;

use hkask_mcp_nostr_client::config::Config;
use hkask_mcp_nostr_client::relays::MAX_SEED_RELAYS;
use hkask_mcp_nostr_client::server::{AccountRequest, NostrClientServer};
use hkask_mcp_nostr_client::test_support::{HandshakeFailingRelay, TestRelay, stopped_relay_url};
use hkask_mcp_test_support::{Parameters, invoke_tool, parse_envelope};
use hkask_types::WebID;
use serde_json::Value;

fn server_with_seed_relays(seed_relays: &[String]) -> NostrClientServer {
    let joined = seed_relays.join(",");
    NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(None, Some(&joined))),
    )
}

/// The report for one address, found by the address the report states.
fn report_for<'a>(account: &'a Value, address: &str) -> &'a Value {
    account["seed_relays"]
        .as_array()
        .expect("seed_relays is a list")
        .iter()
        .find(|relay| relay["address"] == address)
        .unwrap_or_else(|| panic!("no report for {address}: {account}"))
}

/// expect: one call reports each seed relay's outcome by name.
/// pre: three seed relays: one healthy, one stopped, one that accepts the
/// connection and fails the websocket handshake.
/// post: the call succeeds; the healthy relay is `connected`, the stopped one
/// `unreachable`, the third `handshake_failed`; the two failures carry a
/// reason, and the reports are in the order configured.
#[tokio::test]
async fn relay_probe_names_connected_unreachable_and_handshake_failure() {
    let healthy = TestRelay::start().await.expect("the test relay starts");
    let failing = HandshakeFailingRelay::start()
        .await
        .expect("the listener starts");
    let stopped = stopped_relay_url().await.expect("a free port is found");

    let addresses = [
        healthy.url().to_string(),
        stopped.to_string(),
        failing.url().to_string(),
    ];
    let server = server_with_seed_relays(&addresses);
    let account = invoke_tool!(server, nostr_account, AccountRequest {});

    let reports = account["seed_relays"]
        .as_array()
        .expect("seed_relays is a list");
    let reported: Vec<&str> = reports
        .iter()
        .map(|relay| relay["address"].as_str().expect("an address"))
        .collect();
    assert_eq!(reported, addresses, "one report per relay, in order");

    assert_eq!(
        report_for(&account, &addresses[0])["state"],
        "connected",
        "{account}"
    );
    let unreachable = report_for(&account, &addresses[1]);
    assert_eq!(unreachable["state"], "unreachable", "{account}");
    assert!(
        unreachable["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
    let handshake = report_for(&account, &addresses[2]);
    assert_eq!(handshake["state"], "handshake_failed", "{account}");
    assert!(
        handshake["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert!(report_for(&account, &addresses[0]).get("reason").is_none());
}

/// expect: an entry the server will not connect to is reported as refused,
/// is never contacted, and is never repeated; the usable entries are still
/// probed.
/// pre: one healthy relay, one entry that is not an address, one entry
/// carrying credentials.
/// post: three reports; the healthy relay `connected`; the other two
/// `refused` with a reason and no address; the response contains neither
/// refused entry's text.
#[tokio::test]
async fn relay_probe_reports_a_refused_entry_without_repeating_it() {
    let healthy = TestRelay::start().await.expect("the test relay starts");
    let not_an_address = "s3cr3tmarker-not-an-address".to_string();
    let with_credentials = "ws://operator:s3cr3tmarker@127.0.0.1:9".to_string();
    let server = server_with_seed_relays(&[
        not_an_address.clone(),
        healthy.url().to_string(),
        with_credentials.clone(),
    ]);

    let response = server
        .nostr_account(Parameters(AccountRequest {}))
        .await
        .expect("the tool reports the outcomes; it does not fail");
    assert!(
        !response.contains("s3cr3tmarker"),
        "a refused entry was repeated"
    );
    let account = parse_envelope(&response);
    let reports = account["seed_relays"]
        .as_array()
        .expect("seed_relays is a list");
    assert_eq!(reports.len(), 3);
    assert_eq!(reports[0]["state"], "refused");
    assert_eq!(reports[0]["position"], 1);
    assert!(reports[0].get("address").is_none());
    assert_eq!(reports[1]["state"], "connected");
    assert_eq!(reports[2]["state"], "refused");
    assert_eq!(reports[2]["position"], 3);
    assert!(
        reports[2]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("user name or password"))
    );
}

/// expect: the tool states which addresses this build will connect to, so a
/// reader can tell a fixture build from an installed one.
/// pre: a fixture build with no seed relays.
/// post: the policy is the fixture's; the relay list is empty.
#[tokio::test]
async fn relay_probe_states_the_address_policy_of_the_build() {
    let account = invoke_tool!(
        server_with_seed_relays(&[]),
        nostr_account,
        AccountRequest {}
    );
    assert_eq!(account["relay_address_policy"], "loopback_allowed");
    assert_eq!(account["seed_relays"], serde_json::json!([]));
}

/// expect: one call contacts a bounded number of relays and says what it
/// left out.
/// pre: more seed relays than the bound, every one a stopped address.
/// post: exactly the bound's worth of reports, and the count not probed.
#[tokio::test]
async fn relay_probe_is_bounded_and_says_what_it_skipped() {
    let mut addresses = Vec::new();
    for _ in 0..MAX_SEED_RELAYS + 2 {
        addresses.push(
            stopped_relay_url()
                .await
                .expect("a free port is found")
                .to_string(),
        );
    }
    let account = invoke_tool!(
        server_with_seed_relays(&addresses),
        nostr_account,
        AccountRequest {}
    );
    assert_eq!(
        account["seed_relays"]
            .as_array()
            .expect("seed_relays is a list")
            .len(),
        MAX_SEED_RELAYS
    );
    assert_eq!(account["seed_relays_not_probed"], 2);
}
