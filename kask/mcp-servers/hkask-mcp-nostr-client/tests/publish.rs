//! The publish path's send half, against relays started inside the test.
//!
//! Compiled only with the `test-fixture` feature, the one build that will
//! connect to a relay on loopback. Nothing here leaves the machine, every
//! key is generated inside the test, and the editor is not there: the sign
//! step is the editor's (pinned in `kask_bridge`), so the session's send
//! and read-back are driven with a key the test itself generates.

use std::sync::Arc;

use hkask_mcp_nostr_client::config::Config;
use hkask_mcp_nostr_client::read::Session;
use hkask_mcp_nostr_client::relays::BUILD_ADDRESS_POLICY;
use hkask_mcp_nostr_client::test_support::TestRelay;
use nostr_sdk::prelude::*;

fn note(keys: &Keys, at: u64, text: &str) -> Event {
    EventBuilder::new(Kind::from_u16(1), text)
        .custom_created_at(Timestamp::from_secs(at))
        .finalize(keys)
        .expect("a throwaway key signs")
}

fn config(seed: &TestRelay) -> Arc<Config> {
    let url = seed.url().to_string();
    Arc::new(Config::from_values(None, Some(&url)))
}

/// expect: the session publishes a signed event to the relay it is
/// connected to, reports the relay's own answer, and the read-back finds
/// the event by id with the relay that holds it. The event is signed by a
/// key generated inside the test.
#[tokio::test]
async fn a_session_publishes_reports_each_relay_and_reads_the_event_back() {
    let keys = Keys::generate();
    let published = note(&keys, 100, "published by the test");
    let seed = TestRelay::start().await.expect("the relay starts");

    let session = Session::open(&config(&seed), BUILD_ADDRESS_POLICY).await;
    assert!(session.has_targets());
    let reports = session.send(&published).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["state"], "accepted", "{reports:?}");
    assert_eq!(reports[0]["role"], "seed", "{reports:?}");

    let reading = session.read(&[Filter::new().id(published.id)], 1).await;
    assert_eq!(reading.events.len(), 1, "the read-back finds it by id");
    assert_eq!(reading.events[0].0.id, published.id);
    assert_eq!(reading.events[0].1, [seed.url().to_string()]);
}

/// expect: a relay that refuses the event is reported in the relay's own
/// words, bounded, and the read-back finds nothing.
#[tokio::test]
async fn a_refusing_relay_is_reported_in_its_own_words() {
    let keys = Keys::generate();
    let published = note(&keys, 100, "refused by the relay");
    let seed = TestRelay::start_rejecting("blocked: this relay takes nothing")
        .await
        .expect("the relay starts");

    let session = Session::open(&config(&seed), BUILD_ADDRESS_POLICY).await;
    let reports = session.send(&published).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["state"], "refused", "{reports:?}");
    assert!(
        reports[0]["reason"]
            .as_str()
            .expect("the relay's own words")
            .contains("blocked"),
        "{reports:?}"
    );

    let reading = session.read(&[Filter::new().id(published.id)], 1).await;
    assert_eq!(reading.events.len(), 0, "nothing was stored");
}

/// Start a minimal loopback Blossom (`BUD-02` + `BUD-11`) HTTP server that
/// checks `PUT /upload`, decodes and verifies `Authorization: Nostr <base64url>`
/// (`kind: 24242`), verifies `X-SHA-256` against the body SHA-256, and returns
/// `201 Created` with a `BlobDescriptor` (or a tampered hash when `tamper_hash`
/// is true).
async fn start_mock_blossom_server(expected_pubkey_hex: String, tamper_hash: bool) -> String {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds loopback Blossom server");
    let addr = listener.local_addr().expect("addr");
    let base_url = format!("http://{addr}");
    let expected_domain = addr.to_string();
    let returned_base = base_url.clone();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut header_end = None;
            let mut chunk = [0u8; 4096];
            while header_end.is_none() {
                let n = stream.read(&mut chunk).await.expect("reads request");
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
                header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
            }
            let body_offset = header_end.expect("headers complete");
            let header_text = String::from_utf8_lossy(&buf[..body_offset]).into_owned();
            assert!(
                header_text.starts_with("PUT /upload "),
                "expected PUT /upload, got: {header_text}"
            );

            let mut content_len = 0usize;
            let mut auth_token = None;
            let mut x_sha256 = None;
            for line in header_text.lines().skip(1) {
                if let Some((name, val)) = line.split_once(':') {
                    let val = val.trim();
                    if name.eq_ignore_ascii_case("content-length") {
                        content_len = val.parse().expect("content-length");
                    } else if name.eq_ignore_ascii_case("authorization") {
                        auth_token = val.strip_prefix("Nostr ").map(str::to_string);
                    } else if name.eq_ignore_ascii_case("x-sha-256") {
                        x_sha256 = Some(val.to_string());
                    }
                }
            }
            while buf.len() < body_offset + content_len {
                let n = stream.read(&mut chunk).await.expect("reads body");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let body = &buf[body_offset..body_offset + content_len];
            let actual_sha = hex::encode(Sha256::digest(body));
            assert_eq!(x_sha256.as_deref(), Some(actual_sha.as_str()));

            let token_b64 = auth_token.expect("Authorization: Nostr header required");
            let token_json = String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(token_b64)
                    .expect("standard padded base64 auth"),
            )
            .expect("utf8 auth");
            let verified =
                hkask_nostr_core::Event::from_json(&token_json).expect("BUD-11 auth verifies");
            assert_eq!(verified.kind(), 24_242);
            assert_eq!(hex::encode(verified.pubkey()), expected_pubkey_hex);
            assert!(
                verified
                    .tags()
                    .iter()
                    .any(|t| t == &["t".to_string(), "upload".to_string()])
            );
            assert!(
                verified
                    .tags()
                    .iter()
                    .any(|t| t == &["server".to_string(), expected_domain.clone()])
            );
            assert!(
                verified
                    .tags()
                    .iter()
                    .any(|t| t == &["x".to_string(), actual_sha.clone()])
            );

            let reported_sha = if tamper_hash {
                "0".repeat(64)
            } else {
                actual_sha.clone()
            };
            let descriptor = serde_json::json!({
                "url": format!("{returned_base}/{actual_sha}.mp4"),
                "sha256": reported_sha,
                "size": body.len(),
                "type": "video/mp4",
                "uploaded": 1_700_000_000u64,
            })
            .to_string();
            let http = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{descriptor}",
                descriptor.len()
            );
            stream.write_all(http.as_bytes()).await.expect("writes 201");
        }
    });

    base_url
}

/// expect: local MP4 media is hashed, authorized with a verified `BUD-11`
/// `kind: 24242` token, uploaded via `BUD-02` `PUT /upload` (rejecting a
/// server that returns a mismatched SHA-256 and keeping the honest server's
/// URL), composed into a `NIP-92` `imeta` kind 1 note, published to the relay,
/// and read back intact by event id.
#[tokio::test]
async fn blossom_upload_verifies_bud02_descriptor_and_publishes_nip92_imeta_note() {
    use hkask_mcp_nostr_client::blossom::{
        compose_note_content, encode_blossom_auth_header, prepare_local_media,
        resolve_blossom_targets, upload_prepared_media,
    };

    let keys = Keys::generate();
    let pubkey_hex = keys.public_key().to_hex();
    let tampered_server = start_mock_blossom_server(pubkey_hex.clone(), true).await;
    let honest_server = start_mock_blossom_server(pubkey_hex, false).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mp4_path = dir.path().join("Knife_Fight.mp4");
    std::fs::write(
        &mp4_path,
        b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isomfake-video-payload",
    )
    .expect("writes mp4");

    let prepared = prepare_local_media(&[mp4_path.display().to_string()])
        .await
        .expect("prepares mp4");
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].mime_type, "video/mp4");

    let targets = resolve_blossom_targets(
        &[tampered_server.clone(), honest_server.clone()],
        BUILD_ADDRESS_POLICY,
    )
    .expect("resolves loopback Blossom targets under test-fixture");
    assert_eq!(targets.len(), 2);

    // Build a throwaway BUD-11 `kind: 24242` event matching what the editor signs.
    let mut auth_tags = vec![
        Tag::parse(["t", "upload"]).expect("t tag"),
        Tag::parse(["expiration", "1900000300"]).expect("exp tag"),
        Tag::parse(["x", &prepared[0].sha256]).expect("x tag"),
    ];
    for target in &targets {
        auth_tags.push(Tag::parse(["server", &target.server_domain]).expect("server tag"));
    }
    let auth_event = EventBuilder::new(Kind::from_u16(24_242), "Upload media blob")
        .tags(auth_tags)
        .custom_created_at(Timestamp::from_secs(1_700_000_000))
        .finalize(&keys)
        .expect("signs BUD-11 auth");
    let auth_json: serde_json::Value =
        serde_json::from_str(&auth_event.as_json()).expect("serializes auth");
    let auth_header = encode_blossom_auth_header(&auth_json).expect("encodes Nostr base64url");

    let (attachments, reports) =
        upload_prepared_media(&prepared, &targets, &auth_header, BUILD_ADDRESS_POLICY)
            .await
            .expect("honest Blossom server succeeds after tampered server is rejected");
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        attachments[0].url,
        format!("{honest_server}/{}.mp4", prepared[0].sha256)
    );
    assert_eq!(reports[0]["servers"][0]["state"], "rejected_response");
    assert_eq!(reports[0]["servers"][1]["state"], "uploaded");

    let content = compose_note_content("Knife fight test post", &attachments);
    assert!(content.contains(&attachments[0].url));

    let imeta_tag = Tag::parse([
        "imeta".to_string(),
        format!("url {}", attachments[0].url),
        format!("m {}", attachments[0].mime_type),
        format!("x {}", attachments[0].sha256),
        format!("size {}", attachments[0].size),
    ])
    .expect("imeta tag");
    let published = EventBuilder::new(Kind::from_u16(1), &content)
        .tag(imeta_tag)
        .custom_created_at(Timestamp::from_secs(1_700_000_010))
        .finalize(&keys)
        .expect("signs kind 1 note with imeta");

    let seed = TestRelay::start().await.expect("relay starts");
    let session = Session::open(&config(&seed), BUILD_ADDRESS_POLICY).await;
    let relay_reports = session.send(&published).await;
    assert_eq!(relay_reports[0]["state"], "accepted");

    let reading = session.read(&[Filter::new().id(published.id)], 1).await;
    assert_eq!(reading.events.len(), 1);
    let stored = &reading.events[0].0;
    assert_eq!(stored.content, content);
    assert!(
        stored
            .tags
            .iter()
            .any(|t| t.as_slice().first().map(String::as_str) == Some("imeta")),
        "stored event carries the NIP-92 imeta tag"
    );
}

/// A7 (`exp_a2be8e6e7445464585ad131f4519eed4`) task 1: `acn_ask_publish_loopback`.
///
/// expect: a signed ACN Ask commitment event carrying conditions_of_satisfaction,
/// disqualifier, deadline, and price_sats tags round-trips cleanly over the local
/// loopback TestRelay with exact content and tag fidelity.
#[tokio::test]
async fn acn_ask_publish_loopback() {
    let requester_keys = Keys::generate();
    let ask_payload = serde_json::json!({
        "protocol": "acn/v0",
        "phase": "ask",
        "commitment_id": "a7-loopback-ask-001",
        "conditions_of_satisfaction": [
            "all deterministic boundary invariants pass",
            "spend crosses ApprovalPort::request_spend_approval"
        ],
        "disqualifier": "unattended settlement or automated goal closure",
        "deadline": "2026-12-01T00:00:00Z",
        "price_sats": 0
    })
    .to_string();

    let ask_event = EventBuilder::new(Kind::from_u16(1), &ask_payload)
        .tags([
            Tag::parse(["t", "acn-ask"]).expect("acn-ask tag"),
            Tag::parse(["d", "a7-loopback-ask-001"]).expect("d tag"),
            Tag::parse(["disqualifier", "unattended-spend-or-closure"]).expect("disqualifier tag"),
        ])
        .custom_created_at(Timestamp::from_secs(1_700_000_100))
        .finalize(&requester_keys)
        .expect("throwaway requester key signs ACN Ask");

    let seed = TestRelay::start().await.expect("loopback relay starts");
    let session = Session::open(&config(&seed), BUILD_ADDRESS_POLICY).await;
    let reports = session.send(&ask_event).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["state"], "accepted", "{reports:?}");

    let reading = session.read(&[Filter::new().id(ask_event.id)], 1).await;
    assert_eq!(reading.events.len(), 1, "loopback read-back finds ACN Ask");
    let stored = &reading.events[0].0;
    assert_eq!(stored.id, ask_event.id);
    assert_eq!(stored.content, ask_payload);
    assert!(
        stored
            .tags
            .iter()
            .any(|t| t.as_slice() == ["disqualifier", "unattended-spend-or-closure"]),
        "stored ACN Ask preserves disqualifier tag"
    );
}
