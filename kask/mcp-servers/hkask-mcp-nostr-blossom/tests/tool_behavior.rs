//! Tool-behavior contract tests: every Blossom MCP tool (`nostr_blossom_status`,
//! `nostr_blossom_list`, `nostr_blossom_log`) is invoked through its public
//! `Parameters<T>` seam against a real `BlobStore` in a temp directory and, for
//! listener assertions, a real loopback HTTP listener.

use std::collections::BTreeSet;
use std::sync::Arc;

use hkask_ipc_wire::nostr_wire::MAX_BLOSSOM_UPLOAD_BYTES;
use hkask_mcp_nostr_blossom::config::{Config, RefusedEntry};
use hkask_mcp_nostr_blossom::server::{
    ACCESS_LIMITS, BlossomState, DECISION_LOG_CAPACITY, DEFAULT_LIST_BLOBS, DEFAULT_LOG_DECISIONS,
    ListRequest, ListenerState, LogRequest, MAX_LIST_BLOBS, NostrBlossomServer, StatusRequest,
};
use hkask_mcp_nostr_blossom::start_with;
use hkask_mcp_nostr_blossom::storage::BlobStore;
use hkask_mcp_test_support::{Parameters, parse_envelope};
use hkask_nostr_core::encode_npub;
use hkask_types::{McpErrorKind, WebID};
use sha2::{Digest, Sha256};

struct Fixture {
    server: NostrBlossomServer,
    state: Arc<BlossomState>,
    _dir: tempfile::TempDir,
}

fn pubkey_bytes(seed: u8) -> [u8; 32] {
    let mut key = [0u8; 32];
    for (idx, byte) in key.iter_mut().enumerate() {
        *byte = seed.wrapping_add(idx as u8);
    }
    key
}

fn fixture_with_config(config: Config, listener: ListenerState) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_dir = dir.path().join("blossom");
    let store = Arc::new(BlobStore::open(store_dir.clone()).expect("store opens"));
    let state = Arc::new(BlossomState::new(store, config, store_dir, listener));
    let server = NostrBlossomServer::new(WebID::new(), Arc::clone(&state));
    Fixture {
        server,
        state,
        _dir: dir,
    }
}

fn store_payload(
    state: &BlossomState,
    payload: &[u8],
    mime_type: &str,
    owner: &[u8; 32],
) -> String {
    let sha = hex::encode(Sha256::digest(payload));
    let staged = state.store.temp_path(state.next_nonce());
    std::fs::write(&staged, payload).expect("writes staged blob");
    state
        .store
        .commit_blob(
            &staged,
            &sha,
            mime_type,
            payload.len() as u64,
            &hex::encode(owner),
            state.config.max_bytes,
        )
        .expect("commits blob");
    sha
}

#[tokio::test]
async fn nostr_blossom_status_reports_live_listener_quota_authors_and_limits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_dir = tmp.path().join("blossom");
    let ann_path = store_dir.join("listener.json");
    let author = pubkey_bytes(10);

    let config = Config {
        port: 0,
        max_bytes: 4 * 1024 * 1024,
        public_base_url: Some("https://media.example.com".to_string()),
        public_host: Some("media.example.com".to_string()),
        allowed_authors: BTreeSet::from([author]),
        refused: Vec::new(),
    };

    let running = start_with(config, store_dir.clone(), ann_path)
        .await
        .expect("sidecar starts");
    let payload = b"sample-audio-bytes";
    let _sha = store_payload(&running.state, payload, "audio/wav", &author);
    running.state.record_decision(
        "upload",
        Some("abcd".repeat(16)),
        Some(hex::encode(author)),
        true,
        "stored".to_string(),
    );

    let server = NostrBlossomServer::new(WebID::new(), Arc::clone(&running.state));
    let raw = server
        .nostr_blossom_status(Parameters(StatusRequest {}))
        .await
        .expect("status succeeds");
    let body = parse_envelope(&raw);

    assert_eq!(body["server"], "hkask-mcp-nostr-blossom");
    assert_eq!(body["listener"]["state"], "listening");
    assert!(
        body["listener"]["address"]
            .as_str()
            .is_some_and(|addr| addr.starts_with("http://127.0.0.1:")),
        "listener address must be loopback http URL: {}",
        body["listener"]
    );
    assert_eq!(body["blob_count"], 1);
    assert_eq!(body["total_bytes"], payload.len() as u64);
    assert_eq!(body["max_bytes"], 4 * 1024 * 1024_u64);
    assert_eq!(body["max_upload_bytes"], MAX_BLOSSOM_UPLOAD_BYTES);
    assert_eq!(body["public_base_url"], "https://media.example.com");
    assert_eq!(body["allowed_author_count"], 1);
    assert_eq!(
        body["allowed_authors"],
        serde_json::json!([hex::encode(author)])
    );
    assert_eq!(body["decisions_admitted"], 1);
    assert_eq!(body["decisions_refused"], 0);
    assert_eq!(body["refused_config"], serde_json::json!([]));
    assert_eq!(body["access_limits"], serde_json::json!(ACCESS_LIMITS));
    assert_eq!(body["holds_key"], false);

    drop(running.announcement);
    if let Some(task) = running.task {
        task.abort();
    }
}

#[tokio::test]
async fn nostr_blossom_status_surfaces_down_listener_and_refused_config_without_raw_values() {
    let config = Config {
        port: 0,
        max_bytes: 1024,
        public_base_url: None,
        public_host: None,
        allowed_authors: BTreeSet::new(),
        refused: vec![
            RefusedEntry {
                field: "public_base_url",
                position: 0,
                reason: "public_base_url must use https://".to_string(),
            },
            RefusedEntry {
                field: "allowed_authors",
                position: 1,
                reason: "not an npub or 64-character hex public key".to_string(),
            },
        ],
    };
    let fx = fixture_with_config(
        config,
        ListenerState::Down("address already in use".to_string()),
    );

    let raw = fx
        .server
        .nostr_blossom_status(Parameters(StatusRequest {}))
        .await
        .expect("status succeeds even when listener is down");
    let body = parse_envelope(&raw);

    assert_eq!(body["listener"]["state"], "down");
    assert_eq!(body["listener"]["reason"], "address already in use");
    let refused = body["refused_config"].as_array().expect("refused array");
    assert_eq!(refused.len(), 2);
    assert_eq!(refused[0]["field"], "public_base_url");
    assert_eq!(refused[1]["field"], "allowed_authors");
    assert_eq!(refused[1]["position"], 1);
}

#[tokio::test]
async fn nostr_blossom_list_filters_by_hex_or_npub_and_honors_limits() {
    let author_a = pubkey_bytes(21);
    let author_b = pubkey_bytes(42);
    let fx = fixture_with_config(
        Config::default(),
        ListenerState::Down("not bound in unit fixture".to_string()),
    );

    // Empty store defaults cleanly.
    let empty_raw = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest::default()))
        .await
        .expect("empty list succeeds");
    let empty_body = parse_envelope(&empty_raw);
    assert_eq!(empty_body["returned"], 0);
    assert_eq!(empty_body["limit"], DEFAULT_LIST_BLOBS);
    assert_eq!(empty_body["blobs"], serde_json::json!([]));

    let sha_a = store_payload(&fx.state, b"alpha-image", "image/png", &author_a);
    let sha_b = store_payload(&fx.state, b"bravo-video", "video/mp4", &author_b);

    // Unfiltered returns both blobs.
    let all_raw = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: None,
            limit: None,
        }))
        .await
        .expect("unfiltered list succeeds");
    let all_body = parse_envelope(&all_raw);
    assert_eq!(all_body["returned"], 2);

    // Hex author filter isolates author A.
    let hex_raw = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: Some(hex::encode(author_a)),
            limit: Some(MAX_LIST_BLOBS as u32),
        }))
        .await
        .expect("hex author list succeeds");
    let hex_body = parse_envelope(&hex_raw);
    assert_eq!(hex_body["returned"], 1);
    assert_eq!(hex_body["limit"], MAX_LIST_BLOBS);
    assert_eq!(hex_body["blobs"][0]["sha256"], sha_a);

    // NIP-19 npub author filter resolves to the exact same owner.
    let npub_b = encode_npub(&author_b);
    let npub_raw = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: Some(format!("  {npub_b}  ")),
            limit: Some(1),
        }))
        .await
        .expect("npub author list succeeds");
    let npub_body = parse_envelope(&npub_raw);
    assert_eq!(npub_body["returned"], 1);
    assert_eq!(npub_body["limit"], 1);
    assert_eq!(npub_body["blobs"][0]["sha256"], sha_b);
}

#[tokio::test]
async fn nostr_blossom_list_rejects_invalid_limits_and_never_echoes_secret_author_input() {
    let fx = fixture_with_config(
        Config::default(),
        ListenerState::Down("not bound".to_string()),
    );

    let zero_err = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: None,
            limit: Some(0),
        }))
        .await
        .expect_err("limit 0 must fail");
    assert_eq!(zero_err.kind, McpErrorKind::InvalidArgument);
    assert!(zero_err.to_string().contains("at least 1"));

    let over_err = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: None,
            limit: Some(MAX_LIST_BLOBS as u32 + 1),
        }))
        .await
        .expect_err("over-cap limit must fail");
    assert_eq!(over_err.kind, McpErrorKind::InvalidArgument);
    assert!(over_err.to_string().contains(&MAX_LIST_BLOBS.to_string()));

    let secret_like = "nsec1s3cr3tmarker000000000000000000000000000000000000000000";
    let author_err = fx
        .server
        .nostr_blossom_list(Parameters(ListRequest {
            author: Some(secret_like.to_string()),
            limit: None,
        }))
        .await
        .expect_err("nsec or malformed author must fail");
    assert_eq!(author_err.kind, McpErrorKind::InvalidArgument);
    let msg = author_err.to_string();
    assert!(
        !msg.contains("s3cr3tmarker") && !msg.contains("nsec1"),
        "error message must never echo pasted secret material: {msg}"
    );
}

#[tokio::test]
async fn nostr_blossom_log_orders_newest_first_caps_ring_buffer_and_validates_limits() {
    let fx = fixture_with_config(
        Config::default(),
        ListenerState::Down("not bound".to_string()),
    );

    let empty_raw = fx
        .server
        .nostr_blossom_log(Parameters(LogRequest::default()))
        .await
        .expect("empty log succeeds");
    let empty_body = parse_envelope(&empty_raw);
    assert_eq!(empty_body["returned"], 0);
    assert_eq!(empty_body["limit"], DEFAULT_LOG_DECISIONS);
    assert_eq!(empty_body["total_admitted"], 0);
    assert_eq!(empty_body["total_refused"], 0);

    // Push DECISION_LOG_CAPACITY + 5 entries to exercise ring-buffer eviction
    // while keeping monotonic lifetime totals.
    for idx in 0..(DECISION_LOG_CAPACITY + 5) {
        let ok = idx % 2 == 0;
        fx.state.record_decision(
            if ok { "upload" } else { "head_upload" },
            Some(format!("{idx:064x}")),
            None,
            ok,
            format!("decision-{idx}"),
        );
    }

    let capped_raw = fx
        .server
        .nostr_blossom_log(Parameters(LogRequest {
            limit: Some(DECISION_LOG_CAPACITY as u32),
        }))
        .await
        .expect("max-limit log succeeds");
    let capped_body = parse_envelope(&capped_raw);
    assert_eq!(capped_body["returned"], DECISION_LOG_CAPACITY);
    assert_eq!(capped_body["total_admitted"], 103);
    assert_eq!(capped_body["total_refused"], 102);

    let decisions = capped_body["decisions"]
        .as_array()
        .expect("decisions array");
    assert_eq!(
        decisions[0]["outcome"],
        format!("decision-{}", DECISION_LOG_CAPACITY + 4)
    );
    assert_eq!(
        decisions[DECISION_LOG_CAPACITY - 1]["outcome"],
        "decision-5"
    );

    let two_raw = fx
        .server
        .nostr_blossom_log(Parameters(LogRequest { limit: Some(2) }))
        .await
        .expect("limit 2 log succeeds");
    let two_body = parse_envelope(&two_raw);
    assert_eq!(two_body["returned"], 2);
    assert_eq!(two_body["limit"], 2);

    let zero_err = fx
        .server
        .nostr_blossom_log(Parameters(LogRequest { limit: Some(0) }))
        .await
        .expect_err("limit 0 must fail");
    assert_eq!(zero_err.kind, McpErrorKind::InvalidArgument);

    let over_err = fx
        .server
        .nostr_blossom_log(Parameters(LogRequest {
            limit: Some(DECISION_LOG_CAPACITY as u32 + 1),
        }))
        .await
        .expect_err("over-cap limit must fail");
    assert_eq!(over_err.kind, McpErrorKind::InvalidArgument);
}

fn sign_blossom_auth_with_tags(secret: [u8; 32], tags: Vec<Vec<String>>) -> String {
    use base64::Engine as _;
    use hkask_nostr_core::{Event, event_id};
    use k256::schnorr::SigningKey;
    use k256::schnorr::signature::hazmat::PrehashSigner;

    let signing_key = SigningKey::from_bytes(&secret.into()).expect("valid signing key");
    let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes().into();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix time")
        .as_secs();
    let created_at = now.saturating_sub(5);
    let id = event_id(&pubkey, created_at, 24_242, &tags, "Blossom auth");
    let sig = signing_key.sign_prehash(&id).expect("signs");
    let event = Event::from_parts(
        pubkey,
        created_at,
        24_242,
        tags,
        "Blossom auth".to_string(),
        sig.to_bytes(),
    )
    .expect("verified");
    let encoded = base64::engine::general_purpose::STANDARD.encode(event.to_json());
    format!("Nostr {encoded}")
}

#[tokio::test]
async fn get_and_head_enforce_csp_nosniff_and_attachment_for_active_or_unknown_mimes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_dir = tmp.path().join("blossom");
    let ann_path = store_dir.join("listener.json");
    let author_secret = [44u8; 32];
    let author_pk: [u8; 32] = k256::schnorr::SigningKey::from_bytes(&author_secret.into())
        .expect("signing key")
        .verifying_key()
        .to_bytes()
        .into();

    let config = Config {
        port: 0,
        max_bytes: 4 * 1024 * 1024,
        public_base_url: None,
        public_host: None,
        allowed_authors: BTreeSet::from([author_pk]),
        refused: Vec::new(),
    };
    let running = start_with(config, store_dir, ann_path)
        .await
        .expect("sidecar starts");
    let addr = match running.state.listener {
        ListenerState::Listening(a) => a,
        ListenerState::Down(ref r) => panic!("listener down: {r}"),
    };
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    for (mime, payload) in [
        ("text/html", b"<script>alert(1)</script>".as_slice()),
        (
            "application/xhtml+xml",
            b"<html xmlns='http://www.w3.org/1999/xhtml'/>".as_slice(),
        ),
        ("image/svg+xml", b"<svg onload='alert(1)'/>".as_slice()),
        ("text/javascript", b"alert(1)".as_slice()),
        ("application/javascript", b"export default 1;".as_slice()),
        ("application/x-custom-unknown", b"opaque-bytes".as_slice()),
    ] {
        let sha = store_payload(&running.state, payload, mime, &author_pk);
        for is_head in [false, true] {
            let resp = if is_head {
                client
                    .head(format!("{base}/{sha}"))
                    .send()
                    .await
                    .expect("HEAD succeeds")
            } else {
                client
                    .get(format!("{base}/{sha}"))
                    .send()
                    .await
                    .expect("GET succeeds")
            };
            assert_eq!(
                resp.status(),
                reqwest::StatusCode::OK,
                "mime={mime} head={is_head}"
            );
            let headers = resp.headers();
            assert_eq!(
                headers
                    .get("content-security-policy")
                    .and_then(|v| v.to_str().ok()),
                Some("sandbox; default-src 'none'"),
                "CSP missing or wrong for {mime} (head={is_head})"
            );
            assert_eq!(
                headers
                    .get("x-content-type-options")
                    .and_then(|v| v.to_str().ok()),
                Some("nosniff"),
                "nosniff missing for {mime} (head={is_head})"
            );
            assert_eq!(
                headers
                    .get("content-disposition")
                    .and_then(|v| v.to_str().ok()),
                Some("attachment"),
                "active/unknown {mime} must carry Content-Disposition: attachment (head={is_head})"
            );
        }
    }

    // Passive media also receives CSP + nosniff, while preserving inline playback MIME.
    let mp4_sha = store_payload(
        &running.state,
        b"passive-mp4-bytes",
        "video/mp4",
        &author_pk,
    );
    let mp4_resp = client
        .get(format!("{base}/{mp4_sha}.mp4"))
        .send()
        .await
        .expect("GET passive mp4");
    assert_eq!(mp4_resp.status(), reqwest::StatusCode::OK);
    assert_eq!(
        mp4_resp
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok()),
        Some("sandbox; default-src 'none'")
    );
    assert_eq!(
        mp4_resp
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        mp4_resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("video/mp4")
    );
    assert!(
        mp4_resp.headers().get("content-disposition").is_none(),
        "passive video/mp4 must remain playable inline"
    );

    drop(running.announcement);
    if let Some(task) = running.task {
        task.abort();
    }
}

#[tokio::test]
async fn kind_24242_server_tags_reject_foreign_server_replay_and_accept_matching_domain() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_dir = tmp.path().join("blossom");
    let ann_path = store_dir.join("listener.json");
    let author_secret = [55u8; 32];
    let author_pk: [u8; 32] = k256::schnorr::SigningKey::from_bytes(&author_secret.into())
        .expect("signing key")
        .verifying_key()
        .to_bytes()
        .into();

    let config = Config {
        port: 0,
        max_bytes: 4 * 1024 * 1024,
        public_base_url: Some("https://media.example.com".to_string()),
        public_host: Some("media.example.com".to_string()),
        allowed_authors: BTreeSet::from([author_pk]),
        refused: Vec::new(),
    };
    let running = start_with(config, store_dir, ann_path)
        .await
        .expect("sidecar starts");
    let addr = match running.state.listener {
        ListenerState::Listening(a) => a,
        ListenerState::Down(ref r) => panic!("listener down: {r}"),
    };
    let base = format!("http://{addr}");
    let local_domain = addr.to_string();
    let client = reqwest::Client::new();

    let payload = b"scoped-server-tag-payload";
    let sha = hex::encode(Sha256::digest(payload));
    let exp = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_secs()
        + 600)
        .to_string();

    // 1. Upload token scoped to a foreign Blossom server must be refused on HEAD and PUT.
    let foreign_upload_auth = sign_blossom_auth_with_tags(
        author_secret,
        vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["expiration".to_string(), exp.clone()],
            vec!["x".to_string(), sha.clone()],
            vec!["server".to_string(), "blossom.other.example".to_string()],
        ],
    );
    let head_foreign = client
        .head(format!("{base}/upload"))
        .header("Authorization", &foreign_upload_auth)
        .header("X-SHA-256", &sha)
        .header("X-Content-Length", payload.len().to_string())
        .send()
        .await
        .expect("HEAD request");
    assert!(
        head_foreign.status().is_client_error(),
        "foreign server tag must be rejected on HEAD /upload, got {}",
        head_foreign.status()
    );

    let put_foreign = client
        .put(format!("{base}/upload"))
        .header("Authorization", &foreign_upload_auth)
        .header("Content-Type", "image/png")
        .header("X-SHA-256", &sha)
        .body(payload.to_vec())
        .send()
        .await
        .expect("PUT request");
    assert!(
        put_foreign.status().is_client_error(),
        "foreign server tag must be rejected on PUT /upload, got {}",
        put_foreign.status()
    );

    // 2. Upload token carrying multiple server tags where one matches this server succeeds.
    let matching_upload_auth = sign_blossom_auth_with_tags(
        author_secret,
        vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["expiration".to_string(), exp.clone()],
            vec!["x".to_string(), sha.clone()],
            vec!["server".to_string(), "blossom.other.example".to_string()],
            vec!["server".to_string(), local_domain],
        ],
    );
    let put_ok = client
        .put(format!("{base}/upload"))
        .header("Authorization", &matching_upload_auth)
        .header("Content-Type", "image/png")
        .header("X-SHA-256", &sha)
        .body(payload.to_vec())
        .send()
        .await
        .expect("PUT matching server");
    assert_eq!(put_ok.status(), reqwest::StatusCode::OK);

    // 3. Delete token scoped to a foreign server must be refused and preserve the blob.
    let foreign_delete_auth = sign_blossom_auth_with_tags(
        author_secret,
        vec![
            vec!["t".to_string(), "delete".to_string()],
            vec!["expiration".to_string(), exp.clone()],
            vec!["x".to_string(), sha.clone()],
            vec!["server".to_string(), "blossom.other.example".to_string()],
        ],
    );
    let del_foreign = client
        .delete(format!("{base}/{sha}"))
        .header("Authorization", &foreign_delete_auth)
        .send()
        .await
        .expect("DELETE foreign");
    assert!(
        del_foreign.status().is_client_error(),
        "foreign server tag must be rejected on DELETE, got {}",
        del_foreign.status()
    );
    assert!(running.state.store.get_blob(&sha).expect("db").is_some());

    // 4. Delete token matching configured public_host succeeds.
    let public_host_delete_auth = sign_blossom_auth_with_tags(
        author_secret,
        vec![
            vec!["t".to_string(), "delete".to_string()],
            vec!["expiration".to_string(), exp],
            vec!["x".to_string(), sha.clone()],
            vec!["server".to_string(), "media.example.com".to_string()],
        ],
    );
    let del_ok = client
        .delete(format!("{base}/{sha}"))
        .header("Authorization", &public_host_delete_auth)
        .send()
        .await
        .expect("DELETE matching public_host");
    assert_eq!(del_ok.status(), reqwest::StatusCode::OK);
    assert!(running.state.store.get_blob(&sha).expect("db").is_none());

    // Refusals are surfaced through nostr_blossom_log.
    let mcp = NostrBlossomServer::new(WebID::new(), Arc::clone(&running.state));
    let log_raw = mcp
        .nostr_blossom_log(Parameters(LogRequest { limit: Some(10) }))
        .await
        .expect("log succeeds");
    let log_body = parse_envelope(&log_raw);
    assert!(log_body["total_refused"].as_u64().unwrap_or(0) >= 3);

    drop(running.announcement);
    if let Some(task) = running.task {
        task.abort();
    }
}
