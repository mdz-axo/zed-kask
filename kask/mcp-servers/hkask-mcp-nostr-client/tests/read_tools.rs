//! The read tools, called through their public `Parameters<T>` seam against
//! relays started inside the test.
//!
//! Compiled only with the `test-fixture` feature, the one build that will
//! connect to a seed relay on loopback. Nothing here leaves the machine, and
//! every key is generated inside the test.

#![cfg(feature = "test-fixture")]

use std::sync::Arc;

use hkask_mcp_nostr_client::config::Config;
use hkask_mcp_nostr_client::server::{
    AuthorRelaysRequest, DecodeRequest, FetchEventsRequest, NostrClientServer,
    NotificationsRequest, ProfileRequest, ThreadRequest,
};
use hkask_mcp_nostr_client::test_support::{TestRelay, stopped_relay_url};
use hkask_mcp_test_support::{Parameters, invoke_tool};
use hkask_types::nostr_wire::{MAX_CONTENT_CHARS, WireEvent};
use hkask_types::{McpErrorKind, WebID};
use nostr_sdk::prelude::*;
use serde_json::Value;

fn event_at(keys: &Keys, kind: u16, at: u64, text: &str, tags: &[&[&str]]) -> Event {
    let tags: Vec<Tag> = tags
        .iter()
        .map(|parts| Tag::parse(parts.iter().copied()).expect("a tag"))
        .collect();
    EventBuilder::new(Kind::from_u16(kind), text)
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(at))
        .finalize(keys)
        .expect("a throwaway key signs")
}

fn note(keys: &Keys, at: u64, text: &str, tags: &[&[&str]]) -> Event {
    event_at(keys, 1, at, text, tags)
}

fn npub(keys: &Keys) -> String {
    hkask_nostr_core::encode_npub(&keys.public_key().to_bytes())
}

/// A server reading the given seed relays, and optionally a local relay
/// announced the way the relay server announces it.
struct Reader {
    server: NostrClientServer,
    _dir: tempfile::TempDir,
}

fn reader(identity: Option<&Keys>, seeds: &[String], local: Option<&TestRelay>) -> Reader {
    let dir = tempfile::tempdir().expect("temp dir");
    let npub = identity.map(npub);
    let mut config = Config::from_values(npub.as_deref(), Some(&seeds.join(",")));
    if let Some(local) = local {
        let path = dir.path().join("listener.json");
        let address = local.url().to_string();
        let announcement = serde_json::json!({ "address": address.trim_end_matches('/') });
        std::fs::write(&path, announcement.to_string()).expect("written");
        config = config.with_local_relay_announcement(path);
    }
    Reader {
        server: NostrClientServer::new(WebID::new(), Arc::new(config)),
        _dir: dir,
    }
}

fn relay_report<'a>(response: &'a Value, relay: &TestRelay) -> &'a Value {
    let address = relay.url().to_string();
    response["relays"]
        .as_array()
        .expect("relays is a list")
        .iter()
        .find(|report| report["address"] == address)
        .unwrap_or_else(|| panic!("no report for {address}: {response}"))
}

fn events(response: &Value, key: &str) -> Vec<WireEvent> {
    serde_json::from_value(response[key].clone())
        .unwrap_or_else(|error| panic!("{key} is a list of events in the shared shape: {error}"))
}

// ── nostr_fetch_events ──────────────────────────────────────────────

/// expect: the local relay is the redundant copy. An event both relays hold
/// is one event with two sources; an event only the local relay holds is
/// still read when the public relay is down.
/// pre: a local relay holding notes A and B; a seed relay holding A only;
/// then the same local relay with a seed relay that is not running.
/// post: first read returns A (served by both) and B (served by the local
/// relay), newest first, and says every relay finished. Second read still
/// returns both, names the seed relay `unreachable`, and says not every
/// relay finished.
#[tokio::test]
async fn an_event_names_every_relay_that_returned_it_and_the_local_relay_covers_an_outage() {
    let author = Keys::generate();
    let a = note(&author, 100, "held by both", &[]);
    let b = note(&author, 200, "held locally only", &[]);
    let local = TestRelay::start().await.expect("the local relay starts");
    let seed = TestRelay::start().await.expect("the seed relay starts");
    local.store(a.clone()).await.expect("stored");
    local.store(b.clone()).await.expect("stored");
    seed.store(a.clone()).await.expect("stored");

    let request = || FetchEventsRequest {
        authors: Some(vec![npub(&author)]),
        ..FetchEventsRequest::default()
    };
    let both = reader(None, &[seed.url().to_string()], Some(&local));
    let response = invoke_tool!(both.server, nostr_fetch_events, request());

    let found = events(&response, "events");
    assert_eq!(
        found
            .iter()
            .map(|event| event.content.as_str())
            .collect::<Vec<_>>(),
        ["held locally only", "held by both"],
        "newest first"
    );
    assert_eq!(found[0].id, b.id.to_hex());
    assert_eq!(found[0].served_by, [local.url().to_string()]);
    let mut sources = found[1].served_by.clone();
    sources.sort();
    let mut expected = [local.url().to_string(), seed.url().to_string()];
    expected.sort();
    assert_eq!(sources, expected, "one event, two sources");

    assert_eq!(response["every_relay_finished"], true);
    assert_eq!(response["local_relay"], "announced");
    assert_eq!(relay_report(&response, &local)["role"], "local");
    assert_eq!(relay_report(&response, &local)["state"], "complete");
    assert_eq!(relay_report(&response, &seed)["role"], "seed");
    assert_eq!(relay_report(&response, &seed)["events"], 1);
    assert!(
        response["content_trust"]
            .as_str()
            .is_some_and(|text| text.contains("untrusted"))
    );

    let stopped = stopped_relay_url().await.expect("a free port");
    let outage = reader(None, &[stopped.to_string()], Some(&local));
    let response = invoke_tool!(outage.server, nostr_fetch_events, request());
    assert_eq!(events(&response, "events").len(), 2);
    assert_eq!(response["every_relay_finished"], false);
    let down = response["relays"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|report| report["address"] == stopped.to_string())
        .expect("the stopped relay is reported");
    assert_eq!(down["state"], "unreachable");
    assert_eq!(down["events"], 0);
}

/// expect: when no relay answers, the call fails. An empty list would say
/// "nothing exists", which was not found out.
/// pre: one seed relay that is not running; no local relay.
/// post: `unavailable`, naming the relay and its state.
#[tokio::test]
async fn a_read_no_relay_answered_fails_instead_of_returning_nothing() {
    let author = Keys::generate();
    let stopped = stopped_relay_url().await.expect("a free port");
    let fx = reader(None, &[stopped.to_string()], None);
    let error = fx
        .server
        .nostr_fetch_events(Parameters(FetchEventsRequest {
            authors: Some(vec![npub(&author)]),
            ..FetchEventsRequest::default()
        }))
        .await
        .expect_err("no relay answered");
    assert_eq!(error.kind, McpErrorKind::Unavailable, "{error}");
    assert!(error.message.contains("unreachable"), "{error}");
    assert!(error.message.contains("not an empty result"), "{error}");
}

/// expect: with no relay to ask at all, the call says what to configure.
/// pre: no seed relay and no local relay.
/// post: `failed_precondition`.
#[tokio::test]
async fn a_read_with_no_relay_configured_is_a_failed_precondition() {
    let author = Keys::generate();
    let fx = reader(None, &[], None);
    let error = fx
        .server
        .nostr_fetch_events(Parameters(FetchEventsRequest {
            authors: Some(vec![npub(&author)]),
            ..FetchEventsRequest::default()
        }))
        .await
        .expect_err("nothing to ask");
    assert_eq!(error.kind, McpErrorKind::FailedPrecondition, "{error}");
}

/// expect: a relay that answers and holds nothing is an empty result, not a
/// failure: that is a finding.
/// pre: a running, empty seed relay.
/// post: zero events, the relay `complete`, `more_may_exist` false.
#[tokio::test]
async fn a_relay_that_answers_with_nothing_is_an_empty_result() {
    let author = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let fx = reader(None, &[seed.url().to_string()], None);
    let response = invoke_tool!(
        fx.server,
        nostr_fetch_events,
        FetchEventsRequest {
            authors: Some(vec![npub(&author)]),
            ..FetchEventsRequest::default()
        }
    );
    assert_eq!(response["returned"], 0);
    assert_eq!(response["more_may_exist"], false);
    assert_eq!(response["local_relay"], "not_announced");
    assert_eq!(relay_report(&response, &seed)["state"], "complete");
}

/// expect: the limit is honoured newest first and the result says more may
/// exist; long content is cut and marked.
/// pre: three notes by one author, the oldest longer than the content bound;
/// limit 2, then limit 3.
/// post: limit 2 returns the two newest and `more_may_exist`; limit 3
/// returns the long note cut to the bound and marked.
#[tokio::test]
async fn the_limit_keeps_the_newest_and_long_content_is_cut_and_marked() {
    let author = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let long = "é".repeat(MAX_CONTENT_CHARS + 50);
    for (at, text) in [(100, long.as_str()), (200, "middle"), (300, "newest")] {
        seed.store(note(&author, at, text, &[]))
            .await
            .expect("stored");
    }
    let fx = reader(None, &[seed.url().to_string()], None);
    let request = |limit| FetchEventsRequest {
        authors: Some(vec![npub(&author)]),
        limit: Some(limit),
        ..FetchEventsRequest::default()
    };

    let response = invoke_tool!(fx.server, nostr_fetch_events, request(2));
    let found = events(&response, "events");
    assert_eq!(
        found
            .iter()
            .map(|event| event.content.as_str())
            .collect::<Vec<_>>(),
        ["newest", "middle"]
    );
    assert_eq!(response["more_may_exist"], true);

    let response = invoke_tool!(fx.server, nostr_fetch_events, request(3));
    let found = events(&response, "events");
    assert_eq!(found.len(), 3);
    assert_eq!(found[2].content.chars().count(), MAX_CONTENT_CHARS);
    assert!(found[2].content_truncated);
    assert!(!found[0].content_truncated);
}

/// expect: bad input is refused before any relay is contacted, and a secret
/// key given as an author is never repeated.
/// pre: no author or id; limit 0; since after until; a direct-message kind;
/// text that is not a key; an nsec-shaped value. The server has no relay, so
/// a request that got past validation would fail differently.
/// post: `invalid_argument` for each; the nsec-shaped text is not in the
/// error.
#[tokio::test]
async fn fetch_refuses_bad_input_and_never_repeats_a_secret_key() {
    let author = Keys::generate();
    let fx = reader(None, &[], None);
    let by_author = || Some(vec![npub(&author)]);
    let secret = {
        let hrp = bech32::Hrp::parse("nsec").expect("valid prefix");
        bech32::encode::<bech32::Bech32>(hrp, &[7u8; 32]).expect("encodes")
    };
    for request in [
        FetchEventsRequest::default(),
        FetchEventsRequest {
            authors: by_author(),
            limit: Some(0),
            ..FetchEventsRequest::default()
        },
        FetchEventsRequest {
            authors: by_author(),
            since: Some(200),
            until: Some(100),
            ..FetchEventsRequest::default()
        },
        FetchEventsRequest {
            authors: by_author(),
            kinds: Some(vec![1, 1059]),
            ..FetchEventsRequest::default()
        },
        FetchEventsRequest {
            authors: Some(vec!["not-a-key".to_string()]),
            ..FetchEventsRequest::default()
        },
        FetchEventsRequest {
            authors: Some(vec![secret.clone()]),
            ..FetchEventsRequest::default()
        },
    ] {
        let error = fx
            .server
            .nostr_fetch_events(Parameters(request))
            .await
            .expect_err("bad input refused");
        assert_eq!(error.kind, McpErrorKind::InvalidArgument, "{error}");
        assert!(
            !error.message.contains(&secret),
            "a secret key was repeated"
        );
    }
}

// ── nostr_thread ────────────────────────────────────────────────────

/// expect: a conversation is read in order from any note in it, and a note
/// no relay returned is counted as missing rather than passed over.
/// pre: root R; reply A to R; reply B to A; a reply C to a note D that is on
/// no relay. The reader asks for the conversation of B.
/// post: R, A, B in reading order at depths 0, 1, 2; C shown at depth 0 with
/// its parent marked missing; one missing note, D; B is marked as the focus.
#[tokio::test]
async fn a_conversation_is_read_in_order_and_a_missing_note_is_counted() {
    let author = Keys::generate();
    let other = Keys::generate();
    let root = note(&author, 100, "root", &[]);
    let root_hex = root.id.to_hex();
    let a = note(&other, 200, "a", &[&["e", &root_hex, "", "root"]]);
    let b = note(
        &author,
        300,
        "b",
        &[
            &["e", &root_hex, "", "root"],
            &["e", &a.id.to_hex(), "", "reply"],
        ],
    );
    let absent = note(
        &other,
        250,
        "never stored",
        &[&["e", &root_hex, "", "root"]],
    );
    let c = note(
        &author,
        400,
        "c",
        &[
            &["e", &root_hex, "", "root"],
            &["e", &absent.id.to_hex(), "", "reply"],
        ],
    );
    let seed = TestRelay::start().await.expect("the relay starts");
    for event in [&root, &a, &b, &c] {
        seed.store(event.clone()).await.expect("stored");
    }
    let fx = reader(None, &[seed.url().to_string()], None);

    let response = invoke_tool!(
        fx.server,
        nostr_thread,
        ThreadRequest {
            event: b.id.to_hex(),
        }
    );
    assert_eq!(response["root"], root_hex);
    let notes = response["notes"].as_array().expect("a list");
    let shape: Vec<(&str, u64, bool)> = notes
        .iter()
        .map(|note| {
            (
                note["content"].as_str().expect("content"),
                note["depth"].as_u64().expect("depth"),
                note["parent_missing"].as_bool().expect("flag"),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("root", 0, false),
            ("a", 1, false),
            ("b", 2, false),
            ("c", 0, true)
        ]
    );
    assert_eq!(response["missing_count"], 1);
    assert_eq!(response["missing"][0], absent.id.to_hex());
    assert_eq!(notes[2]["is_focus"], true);
    assert_eq!(notes[2]["parent"], a.id.to_hex());
    assert_eq!(notes[0]["is_focus"], false);
    assert_eq!(notes[0]["served_by"][0], seed.url().to_string());
}

/// expect: asking for a note no relay holds is `not_found`, and says which
/// relays were not asked.
#[tokio::test]
async fn a_thread_for_a_note_no_relay_holds_is_not_found() {
    let author = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let fx = reader(None, &[seed.url().to_string()], None);
    let error = fx
        .server
        .nostr_thread(Parameters(ThreadRequest {
            event: note(&author, 1, "never stored", &[]).id.to_hex(),
        }))
        .await
        .expect_err("the note is on no relay");
    assert_eq!(error.kind, McpErrorKind::NotFound, "{error}");
    assert!(error.message.contains("not contacted"), "{error}");
}

// ── nostr_profile ───────────────────────────────────────────────────

/// expect: an author's current profile is reported with its fields as text
/// and its NIP-05 address as a claim; an author with no profile is
/// `found: false`; a profile that is not a JSON object is reported as such.
/// pre: one author with an older and a newer kind 0; one author with none;
/// one whose kind 0 content is not JSON.
/// post: the newer profile's name; the claimed address marked unchecked;
/// `found` false for the second; a named error and no fields for the third.
#[tokio::test]
async fn a_profile_is_the_authors_newest_and_its_nip05_is_reported_as_a_claim() {
    let named = Keys::generate();
    let silent = Keys::generate();
    let broken = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let older = r#"{"name":"old name"}"#;
    let newer = r#"{"name":"new name","about":"an about line","nip05":"someone@example.com","unknown_field":"left out","picture":7}"#;
    seed.store(event_at(&named, 0, 100, older, &[]))
        .await
        .expect("stored");
    seed.store(event_at(&named, 0, 200, newer, &[]))
        .await
        .expect("stored");
    seed.store(event_at(&broken, 0, 100, "not json", &[]))
        .await
        .expect("stored");
    let fx = reader(None, &[seed.url().to_string()], None);

    let response = invoke_tool!(
        fx.server,
        nostr_profile,
        ProfileRequest {
            authors: vec![npub(&named), npub(&silent), broken.public_key().to_hex()],
        }
    );
    assert_eq!(response["asked"], 3);
    assert_eq!(response["found"], 2);
    let profiles = response["profiles"].as_array().expect("a list");

    assert_eq!(profiles[0]["npub"], npub(&named));
    assert_eq!(profiles[0]["fields"]["name"], "new name");
    assert_eq!(profiles[0]["fields"]["about"], "an about line");
    assert!(profiles[0]["fields"].get("unknown_field").is_none());
    assert!(
        profiles[0]["fields"].get("picture").is_none(),
        "a field that is not text is left out"
    );
    assert_eq!(profiles[0]["nip05"]["claimed"], "someone@example.com");
    assert_eq!(profiles[0]["nip05"]["checked"], false);
    assert_eq!(profiles[0]["created_at"], 200);

    assert_eq!(profiles[1]["found"], false);
    assert!(profiles[1].get("fields").is_none());

    assert_eq!(profiles[2]["found"], true);
    assert!(profiles[2]["fields"].is_null());
    assert!(profiles[2]["fields_error"].is_string());
}

// ── nostr_author_relays ────────────────────────────────────────────────

/// expect: an author's relay list is read with its read and write marks,
/// and an address this server would never contact is reported by position
/// and reason, not repeated, and nothing listed is contacted.
/// pre: a kind 10002 listing a relay with no mark, one marked write, one
/// marked read, and a loopback address.
/// post: three usable entries with the marks NIP-65 gives them; one refused
/// entry at position 4; the loopback text nowhere in the response.
#[tokio::test]
async fn a_relay_list_is_reported_with_its_marks_and_hostile_addresses_are_refused() {
    let author = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let list = event_at(
        &author,
        10002,
        100,
        "",
        &[
            &["r", "wss://both.example.com"],
            &["r", "wss://outbox.example.com", "write"],
            &["r", "wss://inbox.example.com", "read"],
            &["r", "ws://127.0.0.1:9999"],
            &["t", "not-a-relay-tag"],
        ],
    );
    seed.store(list).await.expect("stored");
    let fx = reader(None, &[seed.url().to_string()], None);

    let response = fx
        .server
        .nostr_author_relays(Parameters(AuthorRelaysRequest {
            author: Some(npub(&author)),
        }))
        .await
        .expect("the list is read");
    assert!(!response.contains("9999"), "a refused address was repeated");
    let response = hkask_mcp_test_support::parse_envelope(&response);

    assert_eq!(response["found"], true);
    assert_eq!(response["contacted"], false);
    let listed = response["listed"].as_array().expect("a list");
    let marks: Vec<(bool, bool)> = listed
        .iter()
        .map(|relay| {
            (
                relay["read"].as_bool().expect("read"),
                relay["write"].as_bool().expect("write"),
            )
        })
        .collect();
    assert_eq!(marks, [(true, true), (false, true), (true, false)]);
    assert!(
        listed[0]["address"]
            .as_str()
            .is_some_and(|address| address.contains("both.example.com"))
    );
    let refused = response["refused"].as_array().expect("a list");
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["position"], 4);
}

/// expect: with no author named and no identity configured, the tool says
/// what is missing; an author with no list is `found: false`.
#[tokio::test]
async fn a_relay_list_needs_an_author_or_a_configured_identity() {
    let author = Keys::generate();
    let seed = TestRelay::start().await.expect("the relay starts");
    let fx = reader(None, &[seed.url().to_string()], None);

    let error = fx
        .server
        .nostr_author_relays(Parameters(AuthorRelaysRequest { author: None }))
        .await
        .expect_err("no identity to default to");
    assert_eq!(error.kind, McpErrorKind::FailedPrecondition, "{error}");

    let response = invoke_tool!(
        fx.server,
        nostr_author_relays,
        AuthorRelaysRequest {
            author: Some(npub(&author)),
        }
    );
    assert_eq!(response["found"], false);
    assert!(response.get("listed").is_none());
}

// ── nostr_notifications ─────────────────────────────────────────────

/// expect: what others published that tags the configured identity is read
/// and typed; the identity's own events and events that do not tag it are
/// left out.
/// pre: the identity's own note N; from someone else a reply to N, a bare
/// mention, a reaction and a repost, each tagging the identity; a note by
/// the identity tagging itself; a note by someone else tagging no one.
/// post: four notifications, newest first: repost, reaction, mention,
/// reply.
#[tokio::test]
async fn notifications_are_typed_and_leave_out_the_identitys_own_events() {
    let me = Keys::generate();
    let other = Keys::generate();
    let me_hex = me.public_key().to_hex();
    let mine = note(&me, 100, "my note", &[]);
    let mine_hex = mine.id.to_hex();
    let seed = TestRelay::start().await.expect("the relay starts");
    for event in [
        mine.clone(),
        note(
            &other,
            200,
            "a reply",
            &[&["e", &mine_hex, "", "root"], &["p", &me_hex]],
        ),
        note(&other, 300, "a mention", &[&["p", &me_hex]]),
        event_at(&other, 7, 400, "+", &[&["e", &mine_hex], &["p", &me_hex]]),
        event_at(&other, 6, 500, "", &[&["e", &mine_hex], &["p", &me_hex]]),
        note(&me, 600, "tagging myself", &[&["p", &me_hex]]),
        note(&other, 700, "tags no one", &[]),
    ] {
        seed.store(event).await.expect("stored");
    }
    let fx = reader(Some(&me), &[seed.url().to_string()], None);

    let response = invoke_tool!(
        fx.server,
        nostr_notifications,
        NotificationsRequest::default()
    );
    assert_eq!(response["for_npub"], npub(&me));
    let types: Vec<&str> = response["notifications"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|item| item["type"].as_str().expect("a type"))
        .collect();
    assert_eq!(types, ["repost", "reaction", "mention", "reply"]);
    assert!(
        events(&response, "notifications")
            .iter()
            .all(|event| event.author == other.public_key().to_hex())
    );
}

/// expect: notifications need an identity; without one the tool says so.
#[tokio::test]
async fn notifications_without_an_identity_are_a_failed_precondition() {
    let seed = TestRelay::start().await.expect("the relay starts");
    let fx = reader(None, &[seed.url().to_string()], None);
    let error = fx
        .server
        .nostr_notifications(Parameters(NotificationsRequest::default()))
        .await
        .expect_err("no identity configured");
    assert_eq!(error.kind, McpErrorKind::FailedPrecondition, "{error}");
    assert!(error.message.contains("kask.nostr.npub"), "{error}");
}

// ── nostr_decode ────────────────────────────────────────────────────

/// expect: the decode tool reads an identifier with no relay configured,
/// and refuses a secret key without repeating it.
/// pre: an `npub`; an nsec-shaped value. The server has no relay at all.
/// post: the key in hex; then `invalid_argument` without the value.
#[tokio::test]
async fn decode_reads_an_identifier_offline_and_refuses_a_secret_key() {
    let keys = Keys::generate();
    let fx = reader(None, &[], None);
    let report = invoke_tool!(
        fx.server,
        nostr_decode,
        DecodeRequest {
            identifier: format!("nostr:{}", npub(&keys)),
        }
    );
    assert_eq!(report["type"], "npub");
    assert_eq!(report["public_key_hex"], keys.public_key().to_hex());

    let secret = {
        let hrp = bech32::Hrp::parse("nsec").expect("valid prefix");
        bech32::encode::<bech32::Bech32>(hrp, &[7u8; 32]).expect("encodes")
    };
    let error = fx
        .server
        .nostr_decode(Parameters(DecodeRequest {
            identifier: secret.clone(),
        }))
        .await
        .expect_err("a secret key is refused");
    assert_eq!(error.kind, McpErrorKind::InvalidArgument, "{error}");
    assert!(!error.message.contains(&secret));
    assert!(error.message.contains("secret key"), "{error}");
}
