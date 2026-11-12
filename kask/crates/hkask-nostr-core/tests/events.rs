//! NIP-01 events and NIP-19 identifiers at the crate's public interface.
//! Expected values come from published documents or from standard tools, never
//! from this crate.

use hkask_nostr_core::{
    Event, EventError, IdentifierError, decode_note, decode_npub, encode_note, encode_npub,
    event_id,
};

/// The first signed example in NIP-17 (a kind 1059 gift wrap), copied
/// unmodified from lines 191-201 of
/// `https://github.com/nostr-protocol/nips/blob/master/17.md`
/// (sha256 of this file `7197816af5198ca789aa44254c0da5de6428cb4a72c31d9f888842394a4ee615`).
/// It was signed by another implementation, which is the point of using it.
const PUBLISHED_EVENT: &str = include_str!("fixtures/nip17-published-gift-wrap.json");
const PUBLISHED_EVENT_ID: &str = "2886780f7349afc1344047524540ee716f7bdc1b64191699855662330bf235d8";
const PUBLISHED_EVENT_PUBKEY: &str =
    "8f8a7ec43b77d25799281207e1a47f7a654755055788f7482653f9c9661c6d51";

fn key(hex_text: &str) -> [u8; 32] {
    hex::decode(hex_text)
        .expect("test value is hex")
        .try_into()
        .expect("test value is 32 bytes")
}

fn refusal(text: &str) -> EventError {
    match Event::from_json(text) {
        Ok(_) => panic!("must not verify"),
        Err(error) => error,
    }
}

/// expect: an event signed by another implementation verifies, so text a relay
/// serves can be attributed to its author.
/// pre: the published NIP-17 example.
/// post: it parses and verifies; id, author, kind, time and the single tag
/// equal what the document states.
#[test]
fn a_published_signed_event_verifies() {
    let event = Event::from_json(PUBLISHED_EVENT)
        .unwrap_or_else(|error| panic!("published event must verify: {error}"));
    assert_eq!(hex::encode(event.id()), PUBLISHED_EVENT_ID);
    assert_eq!(hex::encode(event.pubkey()), PUBLISHED_EVENT_PUBKEY);
    assert_eq!(event.kind(), 1059);
    assert_eq!(event.created_at(), 1_703_128_320);
    assert_eq!(
        event.tags(),
        [vec![
            "p".to_string(),
            "918e2da906df4ccd12c8ac672d8335add131a4cf9d27ce42b3bb3625755f0788".to_string()
        ]]
    );
    assert!(event.content().starts_with("AsqzdlMsG304G8h08bE67dhAR1gF"));
}

/// expect: a relay cannot change an event's text and keep its author.
/// pre: the published event with one byte of its content changed.
/// post: `IdMismatch`: the id no longer matches the content.
#[test]
fn a_one_byte_change_to_the_content_is_refused() {
    let tampered = PUBLISHED_EVENT.replacen("AsqzdlMsG304", "AsqzdlMsG305", 1);
    assert_ne!(tampered, PUBLISHED_EVENT, "the edit must apply");
    assert_eq!(refusal(&tampered), EventError::IdMismatch);
}

/// expect: an event carrying another event's signature is refused.
/// pre: the published event with its id recomputed for changed content, so the
/// id check passes and only the signature is wrong.
/// post: `BadSignature`.
#[test]
fn a_recomputed_id_without_a_new_signature_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(PUBLISHED_EVENT).expect("fixture is JSON");
    let content = "changed";
    let pubkey = key(PUBLISHED_EVENT_PUBKEY);
    let tags = vec![vec![
        "p".to_string(),
        "918e2da906df4ccd12c8ac672d8335add131a4cf9d27ce42b3bb3625755f0788".to_string(),
    ]];
    let new_id = event_id(&pubkey, 1_703_128_320, 1059, &tags, content);
    value["content"] = content.into();
    value["id"] = hex::encode(new_id).into();
    assert_eq!(refusal(&value.to_string()), EventError::BadSignature);
}

/// expect: text that is not a NIP-01 event is refused as malformed, and the
/// refusal repeats none of it.
/// pre: not JSON; a JSON object with no `sig`; an id of the wrong length; an
/// id in upper-case hex (NIP-01 requires lower case); a kind above 65535.
/// post: `Malformed` for each.
#[test]
fn text_that_is_not_an_event_is_malformed() {
    let no_sig = {
        let mut value: serde_json::Value =
            serde_json::from_str(PUBLISHED_EVENT).expect("fixture is JSON");
        value.as_object_mut().expect("object").remove("sig");
        value.to_string()
    };
    let short_id = PUBLISHED_EVENT.replacen(PUBLISHED_EVENT_ID, &PUBLISHED_EVENT_ID[..62], 1);
    let upper_id =
        PUBLISHED_EVENT.replacen(PUBLISHED_EVENT_ID, &PUBLISHED_EVENT_ID.to_uppercase(), 1);
    let big_kind = PUBLISHED_EVENT.replacen("\"kind\":1059", "\"kind\":65536", 1);
    for candidate in [
        "not json at all".to_string(),
        no_sig,
        short_id,
        upper_id,
        big_kind,
    ] {
        assert_ne!(candidate, PUBLISHED_EVENT, "the edit must apply");
        let error = refusal(&candidate);
        assert_eq!(error, EventError::Malformed);
        assert!(!format!("{error} {error:?}").contains("AsqzdlMsG304"));
    }
}

/// expect: the event id follows NIP-01's serialization rules exactly, so ids
/// computed here equal ids computed by any other client.
/// pre: a tag containing a double quote, and content containing a line break,
/// tab, backslash, double quote, carriage return, backspace, form feed and a
/// non-ASCII character.
/// post: the id equals the SHA-256 (coreutils `sha256sum`) of this
/// hand-written serialization:
/// `[0,"3bf0…459d",1700000000,1,[["t","a\"b"]],"line1\nline2\ttab \\ \"q\" \r\b\f é"]`
#[test]
fn event_id_follows_nip01_serialization_and_escaping() {
    let pubkey = key("3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d");
    let tags = vec![vec!["t".to_string(), "a\"b".to_string()]];
    let content = "line1\nline2\ttab \\ \"q\" \r\u{8}\u{c} é";
    assert_eq!(
        hex::encode(event_id(&pubkey, 1_700_000_000, 1, &tags, content)),
        "32ffac92a50cdc7be84108de891a68051f3a3089dcd2918f488e62373322399c"
    );
}

/// expect: a verified event survives being written out and read back, so what
/// the editor signs is what the server sends.
/// pre: the published event.
/// post: `to_json` output parses to an equal event; it contains no line break.
#[test]
fn a_verified_event_round_trips_through_json() {
    let event = Event::from_json(PUBLISHED_EVENT).expect("published event verifies");
    let written = event.to_json();
    assert!(!written.contains('\n'));
    let read_back = Event::from_json(&written)
        .unwrap_or_else(|error| panic!("round trip must verify: {error}"));
    assert_eq!(read_back, event);
}

/// expect: an event assembled from its parts is verified like one that was
/// parsed.
/// pre: the published event's fields, then the same fields with one content
/// byte changed.
/// post: the first equals the parsed event; the second is `BadSignature` (the
/// id is recomputed from the parts, so the old signature no longer fits).
#[test]
fn from_parts_verifies_the_signature_over_the_computed_id() {
    let parsed = Event::from_json(PUBLISHED_EVENT).expect("published event verifies");
    let built = Event::from_parts(
        *parsed.pubkey(),
        parsed.created_at(),
        parsed.kind(),
        parsed.tags().to_vec(),
        parsed.content().to_string(),
        *parsed.sig(),
    )
    .unwrap_or_else(|error| panic!("the same parts must verify: {error}"));
    assert_eq!(built, parsed);

    let error = Event::from_parts(
        *parsed.pubkey(),
        parsed.created_at(),
        parsed.kind(),
        parsed.tags().to_vec(),
        format!("{}x", parsed.content()),
        *parsed.sig(),
    )
    .expect_err("changed content must not verify");
    assert_eq!(error, EventError::BadSignature);
}

/// expect: NIP-19's published `npub` examples encode from the published keys.
/// pre: the two `npub` examples in NIP-19.
/// post: each key encodes to the `npub` the NIP states, and decodes back.
#[test]
fn nip19_published_keys_encode_to_published_npubs() {
    for (npub, key_hex) in [
        (
            "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6",
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        ),
        (
            "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg",
            "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e",
        ),
    ] {
        assert_eq!(encode_npub(&key(key_hex)), npub);
        assert_eq!(hex::encode(decode_npub(npub).expect("decodes")), key_hex);
    }
}

/// expect: a `note` identifier decodes to its event id and encodes back.
/// pre: the published event's id. NIP-19 publishes no `note` example, so the
/// string below was first printed by this crate and then decoded by the
/// Amethyst command-line client (`amy decode`, 2026-11-12) to this id.
/// post: encode and decode agree with that pair.
#[test]
fn a_note_identifier_encodes_and_decodes_an_event_id() {
    let id = key(PUBLISHED_EVENT_ID);
    let note = "note19zr8srmnfxhuzdzqgafy2s8ww9hhhhqmvsv3dxv92e3rxzljxhvq6423ne";
    assert_eq!(encode_note(&id), note);
    assert_eq!(decode_note(note).expect("decodes"), id);
}

/// expect: the two identifier kinds cannot be confused.
/// pre: an `npub` given to `decode_note`.
/// post: `WrongPrefix` naming both prefixes.
#[test]
fn decode_note_refuses_an_npub() {
    let npub = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";
    assert_eq!(
        decode_note(npub).expect_err("an npub is not a note"),
        IdentifierError::WrongPrefix {
            expected: "note",
            found: "npub".to_string(),
        }
    );
}
