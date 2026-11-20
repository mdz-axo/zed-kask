//! The local relay's announcement: the one way an installed build is given a
//! loopback address, and what it admits.
//!
//! Runs in every build. Nothing here leaves the machine: the only
//! connection attempted is to a loopback port nothing listens on.

use std::sync::Arc;

use hkask_mcp_nostr_client::config::Config;
use hkask_mcp_nostr_client::relays::{
    LocalRelay, LocalRelayError, parse_local_relay_announcement, read_local_relay,
};
use hkask_mcp_nostr_client::server::{AccountRequest, NostrClientServer};
use hkask_mcp_test_support::{Parameters, invoke_tool, parse_envelope};
use hkask_types::WebID;

fn announcement(address: &str) -> String {
    serde_json::json!({ "address": address }).to_string()
}

/// A loopback port the operating system just handed out and that nothing
/// listens on any more.
fn stopped_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("an address").port()
}

/// expect: the address the relay server writes is accepted as it is.
/// pre: `ws://127.0.0.1:<port>` in the shared announcement shape.
/// post: that address, and nothing added to it.
#[test]
fn the_relay_servers_announcement_is_accepted() {
    let url = parse_local_relay_announcement(&announcement("ws://127.0.0.1:43455"))
        .expect("the relay server's own form");
    assert_eq!(
        url.to_string().trim_end_matches('/'),
        "ws://127.0.0.1:43455"
    );
}

/// expect: the announcement admits one shape only. It is the single way a
/// loopback address reaches an installed build, so every near miss is
/// refused: another host, another scheme, a path, credentials, a host that
/// only starts with the loopback literal.
/// pre: each address below, in a well-formed announcement.
/// post: `NotLoopback` for each.
#[test]
fn anything_but_the_loopback_literal_and_a_port_is_refused() {
    for address in [
        "ws://localhost:43455",
        "wss://127.0.0.1:43455",
        "http://127.0.0.1:43455",
        "ws://127.0.0.2:43455",
        "ws://10.0.0.5:43455",
        "ws://[::1]:43455",
        "ws://127.0.0.1",
        "ws://127.0.0.1:",
        "ws://127.0.0.1:0",
        "ws://127.0.0.1:65536",
        "ws://127.0.0.1:043455",
        "ws://127.0.0.1:43455/",
        "ws://127.0.0.1:43455/path",
        "ws://127.0.0.1:43455?x=1",
        "ws://127.0.0.1:43455@evil.example.com",
        "ws://127.0.0.1:80.evil.example.com",
        "ws://127.0.0.1:evil.example.com",
        "ws://user:pw@127.0.0.1:43455",
        "wss://relay.example.com",
        " ws://127.0.0.1:43455",
        "ws://127.0.0.1:43455 ",
        "",
    ] {
        assert_eq!(
            parse_local_relay_announcement(&announcement(address)),
            Err(LocalRelayError::NotLoopback),
            "{address:?} must be refused"
        );
    }
}

/// expect: a file that is not the relay server's announcement is refused as
/// such.
#[test]
fn text_that_is_not_an_announcement_is_refused() {
    for text in [
        "",
        "not json",
        "[]",
        "\"ws://127.0.0.1:43455\"",
        r#"{"addr":"ws://127.0.0.1:43455"}"#,
        r#"{"address":43455}"#,
    ] {
        assert_eq!(
            parse_local_relay_announcement(text),
            Err(LocalRelayError::NotAnAnnouncement),
            "{text:?}"
        );
    }
}

/// expect: no announcement means no local relay, which is not an error; a
/// file that is present and wrong is reported, not ignored.
/// pre: no path; a path with no file; a file of other text; a directory.
/// post: `NotAnnounced` twice, then `Unusable` with the reason for each.
#[test]
fn the_announcement_file_is_read_as_absent_usable_or_unusable() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(read_local_relay(None), LocalRelay::NotAnnounced);
    assert_eq!(
        read_local_relay(Some(&dir.path().join("listener.json"))),
        LocalRelay::NotAnnounced
    );

    let garbage = dir.path().join("garbage.json");
    std::fs::write(&garbage, "not json").expect("written");
    assert_eq!(
        read_local_relay(Some(&garbage)),
        LocalRelay::Unusable(LocalRelayError::NotAnAnnouncement)
    );
    assert_eq!(
        read_local_relay(Some(dir.path())),
        LocalRelay::Unusable(LocalRelayError::Unreadable)
    );

    let good = dir.path().join("good.json");
    std::fs::write(&good, announcement("ws://127.0.0.1:43455")).expect("written");
    assert!(matches!(
        read_local_relay(Some(&good)),
        LocalRelay::Announced(_)
    ));
}

/// expect: `nostr_account` says in words when no local relay is announced,
/// and reports an unusable announcement without repeating what it holds.
/// pre: no announcement file; then a file naming another host.
/// post: `not_announced`; then `unusable` with a reason and without the
/// file's text.
#[tokio::test]
async fn nostr_account_names_an_absent_and_an_unusable_local_relay() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("listener.json");
    let server = NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(None, None).with_local_relay_announcement(path.clone())),
    );

    let account = invoke_tool!(server, nostr_account, AccountRequest {});
    assert_eq!(account["local_relay"]["state"], "not_announced");

    std::fs::write(&path, announcement("ws://s3cr3tmarker.example.com:7777")).expect("written");
    let response = server
        .nostr_account(Parameters(AccountRequest {}))
        .await
        .expect("the tool reports the state; it does not fail");
    assert!(
        !response.contains("s3cr3tmarker"),
        "the announced text was repeated"
    );
    let account = parse_envelope(&response);
    assert_eq!(account["local_relay"]["state"], "unusable");
    assert!(
        account["local_relay"]["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
}

/// expect: an installed build tries the announced local relay, and still
/// refuses the same loopback address written as a seed relay.
/// pre: an announcement naming a loopback port nothing listens on, and that
/// same address in the seed-relay setting.
/// post: the local relay is reported by address as `unreachable` (it was
/// tried); the seed relay is `refused` and was not.
#[cfg(not(feature = "test-fixture"))]
#[tokio::test]
async fn an_installed_build_admits_the_announced_local_relay_and_no_other_loopback_address() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("listener.json");
    let address = format!("ws://127.0.0.1:{}", stopped_port());
    std::fs::write(&path, announcement(&address)).expect("written");
    let server = NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(None, Some(&address)).with_local_relay_announcement(path)),
    );

    let account = invoke_tool!(server, nostr_account, AccountRequest {});
    assert_eq!(account["relay_address_policy"], "public_wss_only");
    assert_eq!(account["local_relay"]["state"], "unreachable");
    assert_eq!(
        account["local_relay"]["address"]
            .as_str()
            .map(|reported| reported.trim_end_matches('/')),
        Some(address.as_str())
    );
    assert_eq!(account["seed_relays"][0]["state"], "refused");
    assert!(account["seed_relays"][0].get("address").is_none());
}

/// The control for the test above in a fixture build, where it is compiled
/// out: the helper it relies on yields a usable port.
#[test]
fn a_stopped_port_is_a_real_port() {
    assert_ne!(stopped_port(), 0);
}
