//! Which relay addresses this server will connect to.
//!
//! The policy is a value, so both policies are tested in every build. Which
//! one a build uses is fixed at compile time by the `test-fixture` feature,
//! and the last two tests here pin that.
//!
//! Nothing here opens a connection.

use hkask_mcp_nostr_client::relays::{
    AddressPolicy, BUILD_ADDRESS_POLICY, RelayAddressError, validate_relay_address,
};

fn refusal(policy: AddressPolicy, address: &str) -> RelayAddressError {
    match validate_relay_address(address, policy) {
        Ok(url) => panic!("{address} must be refused, was accepted as {url}"),
        Err(error) => error,
    }
}

/// expect: a `wss://` address with a public host is accepted.
/// pre: a domain name, a domain with a port and path, a public IPv4 literal
/// (RFC 5737 documentation range is not private), a public IPv6 literal.
/// post: each is accepted under the public-only policy.
#[test]
fn address_validation_accepts_public_wss_addresses() {
    for address in [
        "wss://relay.example.com",
        "wss://relay.example.com:8443/nostr",
        "wss://203.0.113.7",
        "wss://[2001:4860:4860::8888]",
    ] {
        let url = validate_relay_address(address, AddressPolicy::PublicOnly)
            .unwrap_or_else(|error| panic!("{address} must be accepted: {error}"));
        assert!(url.as_str().starts_with("wss://"), "{url}");
    }
}

/// expect: anything that is not `wss://` is refused before any connection.
/// pre: `ws://` to a public host, and two schemes that are not websockets.
/// post: `NotWss` for the first; the others are not relay addresses at all.
#[test]
fn address_validation_refuses_everything_that_is_not_wss() {
    assert_eq!(
        refusal(AddressPolicy::PublicOnly, "ws://relay.example.com"),
        RelayAddressError::NotWss
    );
    for address in [
        "https://relay.example.com",
        "relay.example.com",
        "",
        "wss://",
    ] {
        assert_eq!(
            refusal(AddressPolicy::PublicOnly, address),
            RelayAddressError::Unparseable,
            "{address:?}"
        );
    }
}

/// expect: a host that is this machine or a private network is refused, on
/// the literal host.
/// pre: `localhost` in three spellings; loopback, private, link-local and
/// unspecified IPv4 literals; loopback, unspecified, unique-local, link-local
/// and IPv4-mapped-private IPv6 literals. All `wss://`.
/// post: `NotPublic` for each.
#[test]
fn address_validation_refuses_local_and_private_hosts() {
    for address in [
        "wss://localhost",
        "wss://LOCALHOST:7447",
        "wss://relay.localhost",
        "wss://localhost.",
        "wss://127.0.0.1",
        "wss://127.8.9.10:7447",
        "wss://10.0.0.5",
        "wss://172.16.4.4",
        "wss://192.168.1.20",
        "wss://169.254.10.10",
        "wss://0.0.0.0",
        "wss://255.255.255.255",
        "wss://[::1]",
        "wss://[::]",
        "wss://[fc00::1]",
        "wss://[fd12:3456::1]",
        "wss://[fe80::1]",
        "wss://[::ffff:10.0.0.5]",
        "wss://[::ffff:127.0.0.1]",
    ] {
        assert_eq!(
            refusal(AddressPolicy::PublicOnly, address),
            RelayAddressError::NotPublic,
            "{address}"
        );
    }
}

/// expect: an address carrying a user name or password is refused, and the
/// refusal repeats none of it.
/// pre: a `wss://` address with credentials before the host.
/// post: `HasCredentials`, whose text contains neither part.
#[test]
fn address_validation_refuses_credentials_without_repeating_them() {
    let error = refusal(
        AddressPolicy::PublicOnly,
        "wss://operator:hunter2@relay.example.com",
    );
    assert_eq!(error, RelayAddressError::HasCredentials);
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
    assert!(!shown.contains("operator"), "{shown}");
}

/// expect: no refusal repeats the address it refused. A seed-relay setting
/// can hold anything the operator pasted.
/// pre: one address per refusal kind, each carrying a marker string.
/// post: no refusal's text contains the marker.
#[test]
fn address_validation_refusals_never_repeat_the_address() {
    const MARKER: &str = "s3cr3tmarker";
    for address in [
        format!("ws://{MARKER}.example.com"),
        format!("wss://{MARKER}.localhost"),
        format!("{MARKER} is not an address"),
        format!("wss://user:{MARKER}@relay.example.com"),
    ] {
        let error = refusal(AddressPolicy::PublicOnly, &address);
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(MARKER), "{shown}");
    }
}

/// expect: the fixture policy admits what the local test relays need, and
/// still refuses what is not a relay address.
/// pre: `ws://` on loopback with a port; `wss://` to a public host; a
/// non-websocket scheme; an address with credentials.
/// post: the first two are accepted; the last two are refused as under the
/// public-only policy.
#[test]
fn address_validation_fixture_policy_admits_loopback_ws() {
    for address in ["ws://127.0.0.1:40123", "wss://relay.example.com"] {
        validate_relay_address(address, AddressPolicy::LoopbackAllowed)
            .unwrap_or_else(|error| panic!("{address} must be accepted: {error}"));
    }
    assert_eq!(
        refusal(AddressPolicy::LoopbackAllowed, "https://relay.example.com"),
        RelayAddressError::Unparseable
    );
    assert_eq!(
        refusal(
            AddressPolicy::LoopbackAllowed,
            "ws://operator:hunter2@127.0.0.1:40123"
        ),
        RelayAddressError::HasCredentials
    );
}

/// expect: a build without the `test-fixture` feature uses the public-only
/// policy, so it refuses `ws://` and loopback addresses wherever they come
/// from, settings included.
/// pre: this test file compiled without the feature.
/// post: the build's policy is `PublicOnly`.
#[cfg(not(feature = "test-fixture"))]
#[test]
fn address_validation_a_build_without_the_fixture_is_public_only() {
    assert_eq!(BUILD_ADDRESS_POLICY, AddressPolicy::PublicOnly);
}

/// expect: only a build with the `test-fixture` feature admits loopback.
/// pre: this test file compiled with the feature.
/// post: the build's policy is `LoopbackAllowed`.
#[cfg(feature = "test-fixture")]
#[test]
fn address_validation_a_fixture_build_admits_loopback() {
    assert_eq!(BUILD_ADDRESS_POLICY, AddressPolicy::LoopbackAllowed);
}
