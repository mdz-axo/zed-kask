//! Tool-behavior contract tests: `nostr_account` is called through its public
//! `Parameters<T>` seam.
//!
//! Nothing here touches the network, a keychain or the process environment.
//! The identity values are NIP-19's published examples, not anyone's account.

use std::sync::Arc;

use hkask_mcp_nostr_client::config::{Config, Identity};
use hkask_mcp_nostr_client::server::{AccountRequest, NostrClientServer};
use hkask_mcp_test_support::invoke_tool;
use hkask_types::WebID;

/// NIP-19's published `npub` example and the hex public key it states.
const NIP19_NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";
const NIP19_PUBKEY_HEX: &str = "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";
/// NIP-19's published `nsec` example: a secret key in the wrong field.
const NIP19_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn server(npub: Option<&str>, seed_relays: Option<&str>) -> NostrClientServer {
    NostrClientServer::new(
        WebID::new(),
        Arc::new(Config::from_values(npub, seed_relays)),
    )
}

/// expect: the tool reports the identity set in settings in both its forms.
/// pre: NIP-19's example `npub`, and two seed relays.
/// post: state `configured`, the `npub`, the hex key NIP-19 states for it, and
/// the seed relays in the order given.
#[tokio::test]
async fn nostr_account_reports_the_hex_key_for_a_configured_npub() {
    let server = server(
        Some(NIP19_NPUB),
        Some("wss://relay.example.com,wss://other.example.com"),
    );
    let account = invoke_tool!(server, nostr_account, AccountRequest {});

    assert_eq!(account["server"], "hkask-mcp-nostr-client");
    assert_eq!(account["identity"]["state"], "configured");
    assert_eq!(account["identity"]["npub"], NIP19_NPUB);
    assert_eq!(account["identity"]["public_key_hex"], NIP19_PUBKEY_HEX);
    assert_eq!(
        account["seed_relays"],
        serde_json::json!(["wss://relay.example.com", "wss://other.example.com"])
    );
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

/// expect: a value that is not an `npub` is reported as invalid, and a secret
/// key pasted into the public field is never repeated in tool output.
/// pre: NIP-19's example `nsec` in the `npub` setting.
/// post: state `invalid` with a reason; the whole response contains neither
/// the `nsec` text nor a key field.
#[tokio::test]
async fn nostr_account_reports_an_invalid_identity_without_repeating_it() {
    let server = server(Some(NIP19_NSEC), None);
    let response = server
        .nostr_account(hkask_mcp_test_support::Parameters(AccountRequest {}))
        .await
        .expect("the tool reports the state; it does not fail");
    assert!(
        !response.contains(NIP19_NSEC),
        "the response repeats the misplaced secret key"
    );
    let account = hkask_mcp_test_support::parse_envelope(&response);
    assert_eq!(account["identity"]["state"], "invalid");
    assert!(
        account["identity"]["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert!(account["identity"].get("public_key_hex").is_none());
}

/// expect: the tool states that this server cannot sign, and carries the
/// dual-axis stamp every tool response carries.
/// pre: any configuration.
/// post: `can_sign` is false; `ontology` holds a process and a state concept.
#[tokio::test]
async fn nostr_account_states_custody_and_carries_the_ontology_stamp() {
    let account = invoke_tool!(server(None, None), nostr_account, AccountRequest {});
    assert_eq!(account["can_sign"], false);
    assert_eq!(account["holds_key"], false);
    assert!(account["ontology"]["pko_procedure"].is_string());
    assert!(account["ontology"]["dc_type"].is_string());
}

/// expect: the configuration the tests build is the configuration the tool
/// reads, so the cases above exercise the parsing rules.
/// pre: NIP-19's example `npub`.
/// post: `Identity::Configured` holding the canonical `npub`.
#[test]
fn config_holds_the_identity_the_tool_reports() {
    let config = Config::from_values(Some(NIP19_NPUB), None);
    assert!(matches!(
        config.identity,
        Identity::Configured { ref npub, .. } if npub == NIP19_NPUB
    ));
}

/// expect: the seed-relay list is read as the operator wrote it, cleaned of
/// blanks and repeats, and an upper-case `npub` is reported in the canonical
/// lower-case form.
/// pre: relays separated by commas, spaces and a newline, one repeated; the
/// NIP-19 example `npub` in upper case (bech32 allows either case).
/// post: two relays in first-seen order; the lower-case `npub`.
#[test]
fn config_cleans_the_relay_list_and_canonicalizes_the_npub() {
    let config = Config::from_values(
        Some(&format!("  {}  ", NIP19_NPUB.to_uppercase())),
        Some("wss://a.example.com, wss://b.example.com\nwss://a.example.com ,,"),
    );
    assert_eq!(
        config.seed_relays,
        ["wss://a.example.com", "wss://b.example.com"]
    );
    assert!(matches!(
        config.identity,
        Identity::Configured { ref npub, ref public_key }
            if npub == NIP19_NPUB && public_key[0] == 0x7e
    ));
}
