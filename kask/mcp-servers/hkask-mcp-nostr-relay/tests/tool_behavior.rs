//! Tool-behavior contract tests: every tool is called through its public
//! `Parameters<T>` seam, against a real relay engine, a real LMDB store in a
//! temp directory, and the real accept layer on a loopback port.
//!
//! Nothing here leaves the machine. Where a test needs a "public relay" to
//! sync from, it starts a second in-process relay on loopback and hands its
//! address to `sync::run` directly; the configured-relay check that refuses
//! loopback addresses is tested separately in `config.rs`.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use hkask_mcp_nostr_relay::admission::{PATH_SYNC, PATH_WEBSOCKET, Refusal};
use hkask_mcp_nostr_relay::config::Config;
use hkask_mcp_nostr_relay::server::{
    ACCESS_LIMITS, ListenerState, LogRequest, MAX_CONTENT_CHARS, MAX_QUERY_EVENTS,
    NostrRelayServer, QueryRequest, RelayState, StatusRequest, SyncRequest, open_relay,
    start_listener,
};
use hkask_mcp_nostr_relay::sync;
use hkask_mcp_test_support::{invoke_tool, parse_envelope};
use hkask_types::{McpErrorKind, WebID};
use nostr_sdk::prelude::*;
use rmcp::handler::server::wrapper::Parameters;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Fixture {
    server: NostrRelayServer,
    state: Arc<RelayState>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }
}

async fn fixture_with(authors: &[&Keys], sync_relays: Vec<RelayUrl>) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = Config {
        port: 0,
        authors: authors
            .iter()
            .map(|keys| keys.public_key())
            .collect::<BTreeSet<_>>(),
        sync_relays,
        warnings: Vec::new(),
    };
    let mut state = open_relay(config, &dir.path().join("events"))
        .await
        .expect("relay opens");
    start_listener(&mut state).await;
    let port = match &state.listener {
        ListenerState::Listening(address) => address.port(),
        ListenerState::Down(reason) => panic!("listener must be up in tests: {reason}"),
    };
    let state = Arc::new(state);
    Fixture {
        server: NostrRelayServer::new(WebID::new(), state.clone()),
        state,
        port,
        _dir: dir,
    }
}

async fn fixture(authors: &[&Keys]) -> Fixture {
    fixture_with(authors, Vec::new()).await
}

fn note(keys: &Keys, kind: Kind, text: &str) -> Event {
    EventBuilder::new(kind, text)
        .finalize(keys)
        .expect("event signs")
}

/// Publish one event over the websocket with the SDK's own client and
/// return `(relays that accepted, failure messages)`.
async fn publish(url: &str, event: &Event) -> (usize, Vec<String>) {
    let client = Client::default();
    client.add_relay(url).await.expect("relay added");
    client.connect().await;
    let result = tokio::time::timeout(Duration::from_secs(10), client.send_event(event)).await;
    client.shutdown().await;
    match result {
        Ok(Ok(output)) => (
            output.success.len(),
            output.failed.values().map(ToString::to_string).collect(),
        ),
        Ok(Err(error)) => (0, vec![error.to_string()]),
        Err(_) => (0, vec!["timed out".to_string()]),
    }
}

/// Send one raw HTTP request and return the response's status line and body.
async fn raw_request(port: u16, request: String) -> (String, String) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .expect("connects");
    stream.write_all(request.as_bytes()).await.expect("writes");
    let mut buffer = vec![0u8; 4096];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
        .await
        .expect("answers within 5s")
        .expect("reads");
    let text = String::from_utf8_lossy(&buffer[..read]).to_string();
    let status = text.lines().next().unwrap_or_default().to_string();
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(
        !text
            .to_ascii_lowercase()
            .contains("access-control-allow-origin"),
        "no response may carry Access-Control-Allow-Origin: {text}"
    );
    (status, body)
}

fn upgrade_request(host: &str, origin: Option<&str>) -> String {
    let origin = origin
        .map(|origin| format!("Origin: {origin}\r\n"))
        .unwrap_or_default();
    format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{origin}\r\n"
    )
}

// ── nostr_relay_status ──────────────────────────────────────────────

#[tokio::test]
async fn status_reports_the_listener_the_policy_and_the_access_limits() {
    let author = Keys::generate();
    let fx = fixture(&[&author]).await;
    let status = invoke_tool!(fx.server, nostr_relay_status, StatusRequest {});

    assert_eq!(status["listener"]["state"], "listening");
    assert_eq!(status["listener"]["address"], fx.url());
    assert_eq!(status["archive"]["stored_events"], 0);
    assert_eq!(status["archive"]["encrypted_at_rest"], false);
    assert_eq!(
        status["policy"]["allowlisted_authors"],
        serde_json::json!([author.public_key().to_hex()])
    );
    let refused: Vec<u64> = status["policy"]["refused_kinds"]
        .as_array()
        .expect("refused kinds")
        .iter()
        .filter_map(|kind| kind.as_u64())
        .collect();
    assert!(
        refused.contains(&1059),
        "gift wraps are refused: {refused:?}"
    );
    assert_eq!(
        status["access_limits"].as_array().expect("limits").len(),
        ACCESS_LIMITS.len()
    );
    assert_eq!(status["ontology"]["dc_type"], "bibo:Report");
}

#[tokio::test]
async fn status_reports_a_port_that_is_already_taken_and_the_tools_still_work() {
    let first = fixture(&[]).await;
    let dir = tempfile::tempdir().expect("temp dir");
    let config = Config {
        port: first.port,
        ..Config::default()
    };
    let mut state = open_relay(config, &dir.path().join("events"))
        .await
        .expect("relay opens");
    start_listener(&mut state).await;
    let server = NostrRelayServer::new(WebID::new(), Arc::new(state));

    let status = invoke_tool!(server, nostr_relay_status, StatusRequest {});
    assert_eq!(status["listener"]["state"], "down");
    let reason = status["listener"]["reason"].as_str().expect("reason");
    assert!(reason.contains(&first.port.to_string()), "{reason}");
    let listing = invoke_tool!(server, nostr_relay_query, QueryRequest::default());
    assert_eq!(listing["returned"], 0);
}

// ── The accept layer, over a real socket ────────────────────────────

#[tokio::test]
async fn the_listener_refuses_web_page_origins_and_foreign_hosts() {
    let fx = fixture(&[]).await;
    let host = format!("127.0.0.1:{}", fx.port);

    let (status, _) = raw_request(fx.port, upgrade_request(&host, None)).await;
    assert_eq!(status, "HTTP/1.1 101 Switching Protocols");

    for origin in ["https://evil.example", "null", &format!("http://{host}")] {
        let (status, body) = raw_request(fx.port, upgrade_request(&host, Some(origin))).await;
        assert_eq!(status, "HTTP/1.1 403 Forbidden", "origin {origin}");
        assert_eq!(body, "origin not allowed");
    }

    let (status, _) = raw_request(
        fx.port,
        upgrade_request(&format!("attacker.example:{}", fx.port), None),
    )
    .await;
    assert_eq!(status, "HTTP/1.1 421 Misdirected Request");
}

#[tokio::test]
async fn the_listener_serves_a_relay_info_document_only_without_an_origin() {
    let fx = fixture(&[]).await;
    let get = |origin: &str| {
        format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: application/nostr+json\r\n{origin}Connection: close\r\n\r\n",
            fx.port
        )
    };
    let (status, body) = raw_request(fx.port, get("")).await;
    assert_eq!(status, "HTTP/1.1 200 OK");
    let document: serde_json::Value = serde_json::from_str(&body).expect("NIP-11 JSON");
    assert_eq!(document["limitation"]["restricted_writes"], true);

    let (status, _) = raw_request(fx.port, get("Origin: https://evil.example\r\n")).await;
    assert_eq!(status, "HTTP/1.1 403 Forbidden");
}

// ── The websocket path into the archive ─────────────────────────────

#[tokio::test]
async fn a_published_event_is_stored_only_for_an_allowlisted_author() {
    let author = Keys::generate();
    let stranger = Keys::generate();
    let fx = fixture(&[&author]).await;

    let accepted = note(&author, Kind::TextNote, "from the allowlist");
    assert_eq!(publish(&fx.url(), &accepted).await.0, 1);

    let (ok, failures) = publish(&fx.url(), &note(&stranger, Kind::TextNote, "not listed")).await;
    assert_eq!(ok, 0);
    assert!(
        failures.iter().any(|f| f.contains("allowlist")),
        "the client is told why: {failures:?}"
    );

    let listing = invoke_tool!(fx.server, nostr_relay_query, QueryRequest::default());
    assert_eq!(listing["returned"], 1);
    assert_eq!(listing["events"][0]["id"], accepted.id.to_hex());
    assert_eq!(listing["events"][0]["author"], author.public_key().to_hex());
}

#[tokio::test]
async fn a_gift_wrap_is_refused_on_the_websocket_even_from_an_allowlisted_author() {
    let author = Keys::generate();
    let fx = fixture(&[&author]).await;
    let (ok, failures) = publish(&fx.url(), &note(&author, Kind::GiftWrap, "opaque")).await;
    assert_eq!(ok, 0, "{failures:?}");
    let status = invoke_tool!(fx.server, nostr_relay_status, StatusRequest {});
    assert_eq!(status["archive"]["stored_events"], 0);
}

// ── nostr_relay_query ───────────────────────────────────────────────

#[tokio::test]
async fn query_filters_by_author_and_kind_and_marks_content_as_untrusted() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let fx = fixture(&[&alice, &bob]).await;
    for event in [
        note(&alice, Kind::TextNote, "alice note"),
        note(&alice, Kind::Comment, "alice comment"),
        note(&bob, Kind::TextNote, "bob note"),
    ] {
        fx.state.relay.add_event(event).await.expect("stored");
    }

    let by_alice = invoke_tool!(
        fx.server,
        nostr_relay_query,
        QueryRequest {
            authors: Some(vec![alice.public_key().to_hex()]),
            ..QueryRequest::default()
        }
    );
    assert_eq!(by_alice["returned"], 2);

    let comments = invoke_tool!(
        fx.server,
        nostr_relay_query,
        QueryRequest {
            kinds: Some(vec![1111]),
            ..QueryRequest::default()
        }
    );
    assert_eq!(comments["returned"], 1);
    assert_eq!(comments["events"][0]["content"], "alice comment");
    assert!(
        comments["content_trust"]
            .as_str()
            .expect("trust note")
            .contains("untrusted")
    );
    assert_eq!(comments["ontology"]["dc_type"], "dcmitype:Dataset");
}

#[tokio::test]
async fn query_truncates_long_content_and_caps_the_limit() {
    let author = Keys::generate();
    let fx = fixture(&[&author]).await;
    let long = "é".repeat(MAX_CONTENT_CHARS + 500);
    fx.state
        .relay
        .add_event(note(&author, Kind::TextNote, &long))
        .await
        .expect("stored");

    let listing = invoke_tool!(
        fx.server,
        nostr_relay_query,
        QueryRequest {
            limit: Some(10_000),
            ..QueryRequest::default()
        }
    );
    assert_eq!(listing["limit"], MAX_QUERY_EVENTS);
    let event = &listing["events"][0];
    assert_eq!(event["content_truncated"], true);
    assert_eq!(
        event["content"].as_str().expect("content").chars().count(),
        MAX_CONTENT_CHARS
    );
}

#[tokio::test]
async fn query_refuses_bad_input_with_invalid_argument() {
    let fx = fixture(&[]).await;
    for request in [
        QueryRequest {
            authors: Some(vec!["not-a-key".to_string()]),
            ..QueryRequest::default()
        },
        QueryRequest {
            limit: Some(0),
            ..QueryRequest::default()
        },
        QueryRequest {
            since: Some(200),
            until: Some(100),
            ..QueryRequest::default()
        },
    ] {
        let error = fx
            .server
            .nostr_relay_query(Parameters(request))
            .await
            .expect_err("bad input refused");
        assert_eq!(error.kind, McpErrorKind::InvalidArgument, "{error}");
    }
}

// ── nostr_relay_sync ────────────────────────────────────────────────

#[tokio::test]
async fn sync_stores_only_what_the_admission_rule_accepts() {
    let author = Keys::generate();
    let stranger = Keys::generate();

    // A stand-in for a public relay: open, in memory, on loopback. It holds
    // an allowlisted note, a stranger's note, and an allowlisted gift wrap.
    let source = LocalRelay::builder().build();
    source.run().await.expect("source relay runs");
    let wanted = note(&author, Kind::TextNote, "keep me");
    for event in [
        wanted.clone(),
        note(&stranger, Kind::TextNote, "not mine"),
        note(&author, Kind::GiftWrap, "private"),
    ] {
        source.add_event(event).await.expect("source stores");
    }
    let source_url = source.url().await;

    let fx = fixture(&[&author]).await;
    let report = sync::run(&fx.state.relay, &fx.state.gate, &[source_url])
        .await
        .expect("sync runs");

    assert_eq!(report.stored, 1, "{report:?}");
    assert_eq!(report.refused, report.fetched - report.admitted);
    assert!(report.relays.values().all(|state| state == "connected"));

    let listing = invoke_tool!(fx.server, nostr_relay_query, QueryRequest::default());
    assert_eq!(listing["returned"], 1);
    assert_eq!(listing["events"][0]["id"], wanted.id.to_hex());

    // A second sync finds nothing new.
    let source_url = source.url().await;
    let again = sync::run(&fx.state.relay, &fx.state.gate, &[source_url])
        .await
        .expect("second sync runs");
    assert_eq!(again.stored, 0, "{again:?}");
    source.shutdown();
}

#[tokio::test]
async fn sync_admits_nothing_a_relay_returns_outside_the_allowlist() {
    // The gate, not the fetch filter, is the control: call it directly with
    // what a misbehaving relay could return.
    let author = Keys::generate();
    let stranger = Keys::generate();
    let fx = fixture(&[&author]).await;
    assert_eq!(
        fx.state
            .gate
            .admit(PATH_SYNC, &note(&stranger, Kind::TextNote, "injected")),
        Err(Refusal::AuthorNotAllowed)
    );
    assert_eq!(
        fx.state
            .gate
            .admit(PATH_SYNC, &note(&author, Kind::GiftWrap, "injected")),
        Err(Refusal::KindNotAccepted(1059))
    );
}

#[tokio::test]
async fn sync_without_authors_or_relays_is_a_failed_precondition() {
    let fx = fixture(&[]).await;
    let error = fx
        .server
        .nostr_relay_sync(Parameters(SyncRequest::default()))
        .await
        .expect_err("no authors");
    assert_eq!(error.kind, McpErrorKind::FailedPrecondition, "{error}");
    assert!(error.to_string().contains("allowlist"), "{error}");

    let author = Keys::generate();
    let fx = fixture(&[&author]).await;
    let error = fx
        .server
        .nostr_relay_sync(Parameters(SyncRequest::default()))
        .await
        .expect_err("no relays");
    assert_eq!(error.kind, McpErrorKind::FailedPrecondition, "{error}");
    assert!(error.to_string().contains("sync relay"), "{error}");
}

#[tokio::test]
async fn sync_refuses_a_relay_the_operator_did_not_configure() {
    let author = Keys::generate();
    let configured = RelayUrl::parse("wss://relay.example.com").expect("relay url");
    let fx = fixture_with(&[&author], vec![configured]).await;
    for requested in ["wss://other.example.com", "ws://127.0.0.1:1", "not a url"] {
        let error = fx
            .server
            .nostr_relay_sync(Parameters(SyncRequest {
                relays: Some(vec![requested.to_string()]),
            }))
            .await
            .expect_err("unconfigured relay refused");
        assert_eq!(
            error.kind,
            McpErrorKind::InvalidArgument,
            "{requested}: {error}"
        );
    }
}

// ── nostr_relay_log ─────────────────────────────────────────────────

#[tokio::test]
async fn the_log_records_admissions_and_refusals_newest_first() {
    let author = Keys::generate();
    let stranger = Keys::generate();
    let fx = fixture(&[&author]).await;

    let refused = note(&stranger, Kind::TextNote, "no");
    let admitted = note(&author, Kind::TextNote, "yes");
    publish(&fx.url(), &refused).await;
    publish(&fx.url(), &admitted).await;

    let log = invoke_tool!(fx.server, nostr_relay_log, LogRequest::default());
    assert_eq!(log["returned"], 2);
    assert_eq!(log["decisions"][0]["event_id"], admitted.id.to_hex());
    assert_eq!(log["decisions"][0]["outcome"], "admitted");
    assert_eq!(log["decisions"][0]["path"], PATH_WEBSOCKET);
    assert_eq!(log["decisions"][1]["event_id"], refused.id.to_hex());
    assert_eq!(
        log["decisions"][1]["outcome"],
        Refusal::AuthorNotAllowed.message()
    );
    assert_eq!(log["since_start"]["admitted"], 1);
    assert_eq!(log["since_start"]["refused"], 1);

    let error = fx
        .server
        .nostr_relay_log(Parameters(LogRequest { limit: Some(0) }))
        .await
        .expect_err("zero limit refused");
    assert_eq!(error.kind, McpErrorKind::InvalidArgument);
}

// ── The archive survives a restart ──────────────────────────────────

#[tokio::test]
async fn stored_events_are_still_there_when_the_store_is_reopened() {
    let author = Keys::generate();
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("events");
    let config = Config {
        authors: BTreeSet::from([author.public_key()]),
        ..Config::default()
    };
    let event = note(&author, Kind::TextNote, "durable");
    {
        let state = open_relay(config.clone(), &path).await.expect("opens");
        state.relay.add_event(event.clone()).await.expect("stored");
    }
    let state = open_relay(config, &path).await.expect("reopens");
    let server = NostrRelayServer::new(WebID::new(), Arc::new(state));
    let listing = invoke_tool!(server, nostr_relay_query, QueryRequest::default());
    assert_eq!(listing["returned"], 1);
    assert_eq!(listing["events"][0]["id"], event.id.to_hex());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("store dir")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "the archive directory is private");
    }
}

/// `parse_envelope` is the seam the macro uses; keep the import honest.
#[test]
fn the_envelope_helper_unwraps_content() {
    let body = parse_envelope(r#"{"content":{"returned":0}}"#);
    assert_eq!(body["returned"], 0);
}
