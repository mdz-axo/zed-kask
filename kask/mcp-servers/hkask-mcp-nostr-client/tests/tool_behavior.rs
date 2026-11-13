//! Tool-behavior contract tests: `nostr_account` is called through its public
//! `Parameters<T>` seam.
//!
//! Nothing here touches the network, a keychain or the process environment.
//!
//! No identity is written into this file. Each test builds a throwaway key and
//! encodes it at run time, the way the editor hands the server whatever `npub`
//! the operator set in settings. That the encoding is NIP-19's is pinned on
//! NIP-19's published examples in `hkask-nostr-core`'s own tests, not here.

use std::sync::Arc;

use hkask_mcp_nostr_client::config::{Config, Identity};
use hkask_mcp_nostr_client::server::{
    AccountRequest, AuthorRelaysRequest, DecodeRequest, FetchEventsRequest, NostrClientServer,
    NotificationsRequest, ProfileRequest, ThreadRequest,
};
use hkask_mcp_test_support::{Parameters, invoke_tool, parse_envelope};
use hkask_nostr_core::encode_npub;
use hkask_types::{McpErrorKind, WebID};

/// A throwaway 32-byte key: the bytes 1 to 32. Not anyone's account.
fn throwaway_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for (byte, value) in key.iter_mut().zip(1u8..) {
        *byte = value;
    }
    key
}

/// The `npub` text for the throwaway key, as an operator would paste it.
fn throwaway_npub() -> String {
    encode_npub(&throwaway_key())
}

fn lower_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Bech32 text with the given prefix over the throwaway key: the shape of an
/// `nsec`, an `ncryptsec` or any other identifier pasted into the wrong field.
fn bech32_with_prefix(prefix: &str) -> String {
    let hrp = bech32::Hrp::parse(prefix).expect("valid prefix");
    bech32::encode::<bech32::Bech32>(hrp, &throwaway_key()).expect("encodes")
}

fn server(npub: Option<&str>, seed_relays: Option<&str>) -> NostrClientServer {
    NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(npub, seed_relays)),
    )
}

/// expect: the tool reports the identity set in settings in both its forms.
/// pre: the `npub` of a throwaway key. No seed relay: the tool probes the
/// seed relays it is given, and no test in a default build hands it an
/// address it would connect to (the probe is tested in `relay_probe.rs`
/// against relays inside the test).
/// post: state `configured`, that `npub`, and the key's bytes in lower-case
/// hex.
#[tokio::test]
async fn nostr_account_reports_the_hex_key_for_a_configured_npub() {
    let npub = throwaway_npub();
    let server = server(Some(&npub), None);
    let account = invoke_tool!(server, nostr_account, AccountRequest {});

    assert_eq!(account["server"], "hkask-mcp-nostr-client");
    assert_eq!(account["identity"]["state"], "configured");
    assert_eq!(account["identity"]["npub"], npub);
    assert_eq!(
        account["identity"]["public_key_hex"],
        lower_hex(&throwaway_key())
    );
    assert_eq!(account["seed_relays"], serde_json::json!([]));
}

/// expect: a seed relay the build will not connect to is reported as refused
/// and is never contacted, whichever build this is.
/// pre: three entries no build connects to: a scheme that is not a
/// websocket, text that is not an address, and an address with credentials.
/// post: three `refused` reports in order, each with its position and a
/// reason and without the entry's text.
#[tokio::test]
async fn nostr_account_refuses_seed_relays_no_build_connects_to() {
    let server = server(
        None,
        Some(
            "https://s3cr3tmarker.example.com,s3cr3tmarker,wss://u:s3cr3tmarker@relay.example.com",
        ),
    );
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
    for (index, report) in reports.iter().enumerate() {
        assert_eq!(report["state"], "refused", "{report}");
        assert_eq!(report["position"], index + 1);
        assert!(report.get("address").is_none());
        assert!(
            report["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty())
        );
    }
}

/// expect: with no identity set, the tool says so by name and does not fail.
/// pre: nothing configured, and separately the empty values the editor emits
/// for an unset section.
/// post: state `not_configured`, no key fields, no seed relays.
#[tokio::test]
async fn nostr_account_names_the_not_configured_state() {
    for server in [server(None, None), server(Some(""), Some("  "))] {
        let account = invoke_tool!(server, nostr_account, AccountRequest {});
        assert_eq!(account["identity"]["state"], "not_configured");
        assert!(account["identity"].get("npub").is_none());
        assert!(account["identity"].get("public_key_hex").is_none());
        assert_eq!(account["seed_relays"], serde_json::json!([]));
    }
}

/// expect: a value that is not an `npub` is reported as invalid, and is never
/// repeated in tool output, whatever it is. A secret key pasted into the
/// public field is the case that matters most.
/// pre: the throwaway key encoded as an `nsec`, as an `ncryptsec`, as a
/// `note`, and under a prefix that is not a Nostr one; and text that is not
/// bech32.
/// post: state `invalid` with a reason; the whole response contains neither
/// the value nor a key field. The two secret forms are named as a secret key.
#[tokio::test]
async fn nostr_account_reports_an_invalid_identity_without_repeating_it() {
    for (value, is_secret) in [
        (bech32_with_prefix("nsec"), true),
        (bech32_with_prefix("ncryptsec"), true),
        (bech32_with_prefix("note"), false),
        (bech32_with_prefix("hunter"), false),
        ("not bech32 at all".to_string(), false),
    ] {
        let server = server(Some(&value), None);
        let response = server
            .nostr_account(Parameters(AccountRequest {}))
            .await
            .expect("the tool reports the state; it does not fail");
        assert!(
            !response.contains(&value),
            "the response repeats the configured value"
        );
        assert!(
            !response.contains("hunter"),
            "the response repeats a prefix the operator typed"
        );
        let account = parse_envelope(&response);
        assert_eq!(account["identity"]["state"], "invalid");
        let reason = account["identity"]["reason"].as_str().expect("a reason");
        assert_eq!(reason.contains("secret key"), is_secret, "{reason}");
        assert!(account["identity"].get("npub").is_none());
        assert!(account["identity"].get("public_key_hex").is_none());
    }
}

/// expect: the tool states that this server cannot sign, and carries the
/// dual-axis stamp every tool response carries.
/// pre: any configuration.
/// post: `can_sign` and `holds_key` are false; `ontology` holds a process and
/// a state concept.
#[tokio::test]
async fn nostr_account_states_custody_and_carries_the_ontology_stamp() {
    let account = invoke_tool!(server(None, None), nostr_account, AccountRequest {});
    assert_eq!(account["can_sign"], false);
    assert_eq!(account["holds_key"], false);
    assert!(account["ontology"]["pko_procedure"].is_string());
    assert!(account["ontology"]["dc_type"].is_string());
}

/// expect: the configuration holds the key the configured `npub` names.
/// pre: the `npub` of the throwaway key.
/// post: `Identity::Configured` with that `npub` and the key's 32 bytes.
#[test]
fn config_holds_the_identity_the_tool_reports() {
    let npub = throwaway_npub();
    let config = Config::from_values(Some(&npub), None);
    assert_eq!(
        config.identity,
        Identity::Configured {
            npub,
            public_key: throwaway_key(),
        }
    );
}

/// expect: the seed-relay list is read as the operator wrote it, cleaned of
/// blanks and repeats, and an upper-case `npub` is reported in the canonical
/// lower-case form.
/// pre: relays separated by commas, spaces and a newline, one repeated; the
/// throwaway `npub` in upper case with spaces around it (bech32 allows
/// either case).
/// post: two relays in first-seen order; the lower-case `npub` and its key.
#[test]
fn config_cleans_the_relay_list_and_canonicalizes_the_npub() {
    let config = Config::from_values(
        Some(&format!("  {}  ", throwaway_npub().to_uppercase())),
        Some("wss://a.example.com, wss://b.example.com\nwss://a.example.com ,,"),
    );
    assert_eq!(
        config.seed_relays,
        ["wss://a.example.com", "wss://b.example.com"]
    );
    assert_eq!(
        config.identity,
        Identity::Configured {
            npub: throwaway_npub(),
            public_key: throwaway_key(),
        }
    );
}

/// expect: a limit over the stated maximum is refused by name before any
/// relay is contacted, and never clamped in silence (mcp-tool-review NC-02).
/// pre: an identity, no relay (so a request that got past the limit check
/// would fail later with `failed_precondition`, not `invalid_argument`).
#[tokio::test]
async fn a_limit_over_the_maximum_is_refused_not_clamped() {
    let npub = throwaway_npub();
    let server = server(Some(&npub), None);
    let fetch = server
        .nostr_fetch_events(Parameters(FetchEventsRequest {
            authors: Some(vec![npub.clone()]),
            limit: Some(100_000),
            ..FetchEventsRequest::default()
        }))
        .await
        .expect_err("100000 is over the maximum");
    assert_eq!(fetch.kind, McpErrorKind::InvalidArgument, "{fetch}");
    assert!(
        fetch.to_string().contains("100000") && fetch.to_string().contains("100"),
        "{fetch}"
    );
    let notifications = server
        .nostr_notifications(Parameters(NotificationsRequest {
            limit: Some(101),
            ..NotificationsRequest::default()
        }))
        .await
        .expect_err("101 is over the maximum");
    assert_eq!(notifications.kind, McpErrorKind::InvalidArgument);
    // The maximum itself passes the limit check and fails only for want of a relay.
    let at_most = server
        .nostr_notifications(Parameters(NotificationsRequest {
            limit: Some(100),
            ..NotificationsRequest::default()
        }))
        .await
        .expect_err("no relay to read");
    assert_eq!(at_most.kind, McpErrorKind::FailedPrecondition, "{at_most}");
}

/// expect: an argument no tool declares is refused at decode, so `author`
/// for `authors` never reads as "no author named" (NC-01).
#[test]
fn undeclared_arguments_are_refused_at_decode() {
    fn refuses_unknown<T: serde::de::DeserializeOwned + std::fmt::Debug>(
        name: &str,
        mut valid: serde_json::Value,
    ) {
        serde_json::from_value::<T>(valid.clone())
            .unwrap_or_else(|error| panic!("{name}: the valid form must decode: {error}"));
        valid["zz_undeclared"] = serde_json::json!(1);
        let error = serde_json::from_value::<T>(valid).expect_err(name);
        assert!(
            error.to_string().contains("unknown field"),
            "{name}: {error}"
        );
    }
    refuses_unknown::<AccountRequest>("AccountRequest", serde_json::json!({}));
    refuses_unknown::<DecodeRequest>("DecodeRequest", serde_json::json!({"identifier": "x"}));
    refuses_unknown::<FetchEventsRequest>("FetchEventsRequest", serde_json::json!({"kinds": [1]}));
    refuses_unknown::<ThreadRequest>("ThreadRequest", serde_json::json!({"event": "x"}));
    refuses_unknown::<ProfileRequest>("ProfileRequest", serde_json::json!({"authors": []}));
    refuses_unknown::<AuthorRelaysRequest>("AuthorRelaysRequest", serde_json::json!({}));
    refuses_unknown::<NotificationsRequest>("NotificationsRequest", serde_json::json!({}));
    serde_json::from_value::<FetchEventsRequest>(serde_json::json!({"author": ["x"]}))
        .expect_err("a singular `author` is not the `authors` filter");
}
