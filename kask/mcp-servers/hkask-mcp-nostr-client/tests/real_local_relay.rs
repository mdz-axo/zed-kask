//! The client server against the real relay server.
//!
//! The other test files read relays that are the relay library's own
//! listener. The local relay is not that: it has its own accept layer (it
//! refuses a request that carries an `Origin` header or a foreign `Host`),
//! its own admission rule, and it tells the client where it listens by
//! writing a file. This file starts that server inside the test and reads it
//! through the path an installed client uses: the announcement, with no seed
//! relay.
//!
//! Runs in every build, the installed one included: the announcement is the
//! one way a loopback address reaches a build that refuses loopback seed
//! relays. Nothing here leaves the machine, and every key is generated
//! inside the test.

use std::collections::BTreeSet;
use std::sync::Arc;

use hkask_mcp_nostr_client::config::Config;
use hkask_mcp_nostr_client::server::{
    AccountRequest, FetchEventsRequest, NostrClientServer, ThreadRequest,
};
use hkask_mcp_nostr_relay::config::Config as RelayConfig;
use hkask_mcp_nostr_relay::server::{
    ListenerState, RelayState, announce_listener, open_relay, start_listener,
};
use hkask_mcp_test_support::invoke_tool;
use hkask_types::WebID;
use hkask_types::nostr_wire::WireEvent;
use nostr_sdk::prelude::*;

fn note(keys: &Keys, at: u64, text: &str, tags: &[&[&str]]) -> Event {
    let tags: Vec<Tag> = tags
        .iter()
        .map(|parts| Tag::parse(parts.iter().copied()).expect("a tag"))
        .collect();
    EventBuilder::new(Kind::from_u16(1), text)
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(at))
        .finalize(keys)
        .expect("a throwaway key signs")
}

fn npub(keys: &Keys) -> String {
    hkask_nostr_core::encode_npub(&keys.public_key().to_bytes())
}

/// The relay server and a client server that finds it the way an installed
/// client does.
struct Pair {
    relay: RelayState,
    client: NostrClientServer,
    address: String,
    _dir: tempfile::TempDir,
}

/// Start the relay server with `authors` allowlisted, let it announce its
/// listener, and point a client at the announcement and at no seed relay.
async fn pair(authors: &[&Keys]) -> Pair {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = RelayConfig {
        port: 0,
        authors: authors
            .iter()
            .map(|keys| keys.public_key())
            .collect::<BTreeSet<_>>(),
        sync_relays: Vec::new(),
        warnings: Vec::new(),
    };
    let mut relay = open_relay(config, &dir.path().join("events"))
        .await
        .expect("the relay opens");
    start_listener(&mut relay).await;
    let address = match &relay.listener {
        ListenerState::Listening(address) => format!("ws://{address}"),
        ListenerState::Down(reason) => panic!("the listener must be up: {reason}"),
    };
    let announcement = dir.path().join("listener.json");
    announce_listener(&relay.listener, &announcement).expect("the relay announces its listener");

    let client = NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(None, None).with_local_relay_announcement(announcement)),
    );
    Pair {
        relay,
        client,
        address,
        _dir: dir,
    }
}

/// Put an event in the archive as a sync would: through the relay engine,
/// which verifies it.
async fn archive(pair: &Pair, event: &Event) {
    let status = pair
        .relay
        .relay
        .add_event(event.clone())
        .await
        .expect("the archive accepts a valid event");
    assert!(status.is_success(), "{status:?}");
}

/// expect: an installed client reads the local relay's archive through the
/// relay's own accept layer, having been told the address by the relay
/// itself and by nothing else.
/// pre: the relay server running with one allowlisted author and two of
/// that author's notes archived; a client with no seed relay, pointed only
/// at the file the relay wrote.
/// post: `nostr_account` reports the local relay `connected` at the address
/// the relay bound; `nostr_fetch_events` returns both notes, newest first,
/// each served by that address, with the local relay's read `complete`.
#[tokio::test]
async fn the_client_reads_the_real_relay_through_its_announcement() {
    let author = Keys::generate();
    let fx = pair(&[&author]).await;
    archive(&fx, &note(&author, 100, "older", &[])).await;
    archive(&fx, &note(&author, 200, "newer", &[])).await;

    let account = invoke_tool!(fx.client, nostr_account, AccountRequest {});
    assert_eq!(account["local_relay"]["state"], "connected", "{account}");
    assert_eq!(
        account["local_relay"]["address"]
            .as_str()
            .map(|address| address.trim_end_matches('/')),
        Some(fx.address.as_str())
    );
    assert_eq!(account["seed_relays"], serde_json::json!([]));

    let response = invoke_tool!(
        fx.client,
        nostr_fetch_events,
        FetchEventsRequest {
            authors: Some(vec![npub(&author)]),
            ..FetchEventsRequest::default()
        }
    );
    let events: Vec<WireEvent> =
        serde_json::from_value(response["events"].clone()).expect("events in the shared shape");
    assert_eq!(
        events
            .iter()
            .map(|event| event.content.as_str())
            .collect::<Vec<_>>(),
        ["newer", "older"]
    );
    for event in &events {
        assert_eq!(event.served_by.len(), 1);
        assert_eq!(event.served_by[0].trim_end_matches('/'), fx.address);
    }
    let relays = response["relays"].as_array().expect("a list");
    assert_eq!(relays.len(), 1, "the local relay and no other: {response}");
    assert_eq!(relays[0]["role"], "local");
    assert_eq!(relays[0]["state"], "complete");
    assert_eq!(relays[0]["events"], 2);
    assert_eq!(response["every_relay_finished"], true);
    assert_eq!(response["local_relay"], "announced");
}

/// expect: a conversation held in the local archive is read in order from
/// the real relay, which answers the client's two questions (the note, then
/// the root and everything that tags it).
/// pre: a root note and a reply to it, both by the allowlisted author, in
/// the archive.
/// post: root at depth 0, reply at depth 1 and marked as the note asked
/// for; nothing missing.
#[tokio::test]
async fn a_conversation_is_read_from_the_real_relay() {
    let author = Keys::generate();
    let fx = pair(&[&author]).await;
    let root = note(&author, 100, "root", &[]);
    let reply = note(
        &author,
        200,
        "reply",
        &[&["e", &root.id.to_hex(), "", "root"]],
    );
    archive(&fx, &root).await;
    archive(&fx, &reply).await;

    let response = invoke_tool!(
        fx.client,
        nostr_thread,
        ThreadRequest {
            event: reply.id.to_hex(),
        }
    );
    assert_eq!(response["root"], root.id.to_hex());
    assert_eq!(response["missing_count"], 0);
    let notes = response["notes"].as_array().expect("a list");
    let shape: Vec<(&str, u64)> = notes
        .iter()
        .map(|note| {
            (
                note["content"].as_str().expect("content"),
                note["depth"].as_u64().expect("depth"),
            )
        })
        .collect();
    assert_eq!(shape, [("root", 0), ("reply", 1)]);
    assert_eq!(notes[1]["is_focus"], true);
}

/// expect: when the relay server's listener is down, it withdraws its
/// announcement, and the client then has no relay to read rather than a
/// stale address.
/// pre: a running pair; then the relay reports its listener down and
/// re-announces, as it does at startup.
/// post: the announcement file is gone, `nostr_account` reports
/// `not_announced`, and a read is a failed precondition.
#[tokio::test]
async fn a_withdrawn_announcement_leaves_the_client_with_no_local_relay() {
    let author = Keys::generate();
    let fx = pair(&[&author]).await;
    let announcement = fx._dir.path().join("listener.json");
    assert!(announcement.exists(), "the control: it was announced");

    announce_listener(
        &ListenerState::Down("the port is taken".to_string()),
        &announcement,
    )
    .expect("withdrawn");
    assert!(!announcement.exists());

    let account = invoke_tool!(fx.client, nostr_account, AccountRequest {});
    assert_eq!(account["local_relay"]["state"], "not_announced");
    let error = fx
        .client
        .nostr_fetch_events(hkask_mcp_test_support::Parameters(FetchEventsRequest {
            authors: Some(vec![npub(&author)]),
            ..FetchEventsRequest::default()
        }))
        .await
        .expect_err("there is no relay to read");
    assert_eq!(
        error.kind,
        hkask_types::McpErrorKind::FailedPrecondition,
        "{error}"
    );
}
