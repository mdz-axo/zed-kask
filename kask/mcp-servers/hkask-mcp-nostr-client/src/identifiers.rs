//! Reading the identifiers people paste: NIP-19 `npub`, `nprofile`, `note`,
//! `nevent` and `naddr`, with or without a `nostr:` prefix, and bare
//! 64-character hex.
//!
//! Nothing here contacts a relay. A secret key (`nsec`, `ncryptsec`) is
//! recognized by its prefix and refused before it is decoded: this process
//! never holds a key, and no message repeats what was given.

use nostr_sdk::prelude::*;
use serde_json::{Value, json};

/// The most relay hints reported from one identifier.
pub const MAX_RELAY_HINTS: usize = 16;

/// Why an identifier was not read. No variant carries the text given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierRefusal {
    /// The text is a secret key.
    SecretKey,
    /// The text is not an identifier this server reads.
    NotAnIdentifier,
}

impl IdentifierRefusal {
    /// What to tell the caller.
    pub fn message(self) -> &'static str {
        match self {
            Self::SecretKey => {
                "this is a secret key. It was not decoded and is not repeated here. A secret key \
                 belongs only in the Nostr settings; one that has been typed into a conversation \
                 has been sent to the model provider"
            }
            Self::NotAnIdentifier => {
                "not an npub, nprofile, note, nevent or naddr identifier, and not 64 characters \
                 of hex"
            }
        }
    }
}

/// Whether `text` is a secret key in either of its bech32 forms.
pub fn is_secret_key(text: &str) -> bool {
    let text = strip_uri(text).to_ascii_lowercase();
    text.starts_with("nsec1") || text.starts_with("ncryptsec1")
}

fn strip_uri(text: &str) -> &str {
    let text = text.trim();
    match text.get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case("nostr:") => &text[6..],
        _ => text,
    }
}

fn relay_hints(relays: &[RelayUrl]) -> Value {
    json!({
        "addresses": relays.iter().take(MAX_RELAY_HINTS).map(ToString::to_string).collect::<Vec<_>>(),
        "truncated": relays.len() > MAX_RELAY_HINTS,
        "contacted": false,
        "note": "written by whoever made the identifier; this server reads only the local relay and the seed relays",
    })
}

fn public_key_fields(public_key: &PublicKey) -> (String, String) {
    let bytes = public_key.to_bytes();
    (public_key.to_hex(), hkask_nostr_core::encode_npub(&bytes))
}

fn event_id_fields(id: &EventId) -> (String, String) {
    (id.to_hex(), hkask_nostr_core::encode_note(id.as_bytes()))
}

/// Read one identifier and say what it names.
pub fn decode(text: &str) -> Result<Value, IdentifierRefusal> {
    if is_secret_key(text) {
        return Err(IdentifierRefusal::SecretKey);
    }
    let text = strip_uri(text);

    // Bare hex names a public key or an event id; the text cannot say which.
    if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let hex = text.to_ascii_lowercase();
        let as_event = EventId::from_hex(&hex).map_err(|_| IdentifierRefusal::NotAnIdentifier)?;
        let mut report = json!({
            "type": "hex",
            "hex": hex,
            "meaning": "64 characters of hex name either a public key or an event id; the text does not say which",
            "as_event_id": { "note": event_id_fields(&as_event).1 },
        });
        // Not every 32 bytes are a public key.
        report["as_public_key"] = match PublicKey::from_hex(&hex) {
            Ok(public_key) => json!({ "npub": public_key_fields(&public_key).1 }),
            Err(_) => json!({ "valid": false }),
        };
        return Ok(report);
    }

    // The library reads bech32 in one case only; NIP-19 allows either.
    let lowered = text.to_ascii_lowercase();
    let decoded = Nip19::from_bech32(&lowered).map_err(|_| IdentifierRefusal::NotAnIdentifier)?;
    Ok(match decoded {
        Nip19::Pubkey(public_key) => {
            let (hex, npub) = public_key_fields(&public_key);
            json!({ "type": "npub", "public_key_hex": hex, "npub": npub })
        }
        Nip19::Profile(profile) => {
            let (hex, npub) = public_key_fields(&profile.public_key);
            json!({
                "type": "nprofile",
                "public_key_hex": hex,
                "npub": npub,
                "relay_hints": relay_hints(&profile.relays),
            })
        }
        Nip19::EventId(id) => {
            let (hex, note) = event_id_fields(&id);
            json!({ "type": "note", "event_id_hex": hex, "note": note })
        }
        Nip19::Event(event) => {
            let (hex, note) = event_id_fields(&event.event_id);
            json!({
                "type": "nevent",
                "event_id_hex": hex,
                "note": note,
                "author_hex": event.author.map(|author| author.to_hex()),
                "kind": event.kind.map(|kind| kind.as_u16()),
                "relay_hints": relay_hints(&event.relays),
            })
        }
        Nip19::Coordinate(coordinate) => {
            let (hex, npub) = public_key_fields(&coordinate.public_key);
            json!({
                "type": "naddr",
                "kind": coordinate.kind.as_u16(),
                "public_key_hex": hex,
                "npub": npub,
                "identifier": coordinate.identifier,
                "relay_hints": relay_hints(&coordinate.relays),
            })
        }
        // A secret key never gets this far (see `is_secret_key`). The
        // wildcard also covers the encrypted form, which the library has
        // only under a feature this build does not enable: any key form is
        // refused, not printed.
        _ => return Err(IdentifierRefusal::SecretKey),
    })
}

/// Read a public key given as an `npub`, an `nprofile` or 64-character hex.
pub fn parse_public_key(text: &str) -> Result<PublicKey, IdentifierRefusal> {
    if is_secret_key(text) {
        return Err(IdentifierRefusal::SecretKey);
    }
    PublicKey::parse(text.trim()).map_err(|_| IdentifierRefusal::NotAnIdentifier)
}

/// Read an event id given as a `note`, an `nevent` or 64-character hex.
pub fn parse_event_id(text: &str) -> Result<EventId, IdentifierRefusal> {
    if is_secret_key(text) {
        return Err(IdentifierRefusal::SecretKey);
    }
    EventId::parse(text.trim()).map_err(|_| IdentifierRefusal::NotAnIdentifier)
}

/// No identifier is written into this file. Each test makes a throwaway key
/// and encodes it at run time, as the rest of this crate's tests do. That
/// the `npub` and `note` encodings are NIP-19's is pinned on NIP-19's
/// published examples in `hkask-nostr-core`; the `nprofile`, `nevent` and
/// `naddr` codecs are the `nostr` library's and are not checked against
/// published values here.
#[cfg(test)]
mod tests {
    use super::*;

    fn throwaway_key() -> PublicKey {
        Keys::generate().public_key()
    }

    fn npub_of(public_key: &PublicKey) -> String {
        hkask_nostr_core::encode_npub(&public_key.to_bytes())
    }

    /// Bech32 text with the given prefix over 32 arbitrary bytes: the shape
    /// of a secret key, which is all the refusal looks at.
    fn bech32_with_prefix(prefix: &str) -> String {
        let hrp = bech32::Hrp::parse(prefix).expect("valid prefix");
        bech32::encode::<bech32::Bech32>(hrp, &[7u8; 32]).expect("encodes")
    }

    /// expect: an `npub` reads as its key, in either case, with a `nostr:`
    /// prefix, and with space around it.
    #[test]
    fn an_npub_decodes_to_its_key_however_it_is_written() {
        let public_key = throwaway_key();
        let npub = npub_of(&public_key);
        for text in [
            npub.clone(),
            npub.to_uppercase(),
            format!("nostr:{npub}"),
            format!("  {npub}\n"),
        ] {
            let report = decode(&text).expect("an npub");
            assert_eq!(report["type"], "npub");
            assert_eq!(report["public_key_hex"], public_key.to_hex());
            assert_eq!(report["npub"], npub);
            assert_eq!(parse_public_key(&npub), Ok(public_key));
        }
    }

    /// expect: an `nprofile` reads as its key and its relay hints, and says
    /// the hints were not contacted.
    #[test]
    fn an_nprofile_decodes_to_its_key_and_uncontacted_relay_hints() {
        let public_key = throwaway_key();
        let profile = Nip19Profile {
            public_key,
            relays: vec![
                RelayUrl::parse("wss://one.example.com").expect("an address"),
                RelayUrl::parse("wss://two.example.com").expect("an address"),
            ],
        };
        let text = profile.to_bech32().expect("encodes");
        let report = decode(&text).expect("an nprofile");
        assert_eq!(report["type"], "nprofile");
        assert_eq!(report["public_key_hex"], public_key.to_hex());
        let hints = report["relay_hints"]["addresses"]
            .as_array()
            .expect("a list");
        assert_eq!(hints.len(), 2);
        assert!(
            hints[0]
                .as_str()
                .is_some_and(|hint| hint.contains("one.example.com"))
        );
        assert_eq!(report["relay_hints"]["contacted"], false);
        assert_eq!(report["relay_hints"]["truncated"], false);
    }

    /// expect: an `nevent` reads as its event id, author and relay hints.
    #[test]
    fn an_nevent_decodes_to_its_event_id_and_author() {
        let author = throwaway_key();
        let id = EventId::from_slice(&[9u8; 32]).expect("32 bytes");
        let mut pointer = Nip19Event::new(id);
        pointer.author = Some(author);
        pointer.relays = vec![RelayUrl::parse("wss://one.example.com").expect("an address")];
        let text = pointer.to_bech32().expect("encodes");

        let report = decode(&text).expect("an nevent");
        assert_eq!(report["type"], "nevent");
        assert_eq!(report["event_id_hex"], id.to_hex());
        assert_eq!(report["author_hex"], author.to_hex());
        assert_eq!(parse_event_id(&text), Ok(id));
    }

    /// expect: a secret key is refused by name and never repeated, in every
    /// spelling and through every entry point.
    #[test]
    fn a_secret_key_is_refused_without_being_decoded_or_repeated() {
        for prefix in ["nsec", "ncryptsec"] {
            let secret = bech32_with_prefix(prefix);
            for text in [
                secret.clone(),
                secret.to_uppercase(),
                format!("nostr:{secret}"),
                format!(" {secret} "),
            ] {
                assert_eq!(decode(&text), Err(IdentifierRefusal::SecretKey));
                assert_eq!(parse_public_key(&text), Err(IdentifierRefusal::SecretKey));
                assert_eq!(parse_event_id(&text), Err(IdentifierRefusal::SecretKey));
            }
            assert!(!IdentifierRefusal::SecretKey.message().contains(&secret));
        }
    }

    /// expect: bare hex is reported as both things it could name.
    #[test]
    fn bare_hex_is_reported_as_a_key_and_as_an_event_id() {
        let public_key = throwaway_key();
        let hex = public_key.to_hex();
        let report = decode(&hex.to_uppercase()).expect("hex");
        assert_eq!(report["type"], "hex");
        assert_eq!(report["hex"], hex);
        assert_eq!(report["as_public_key"]["npub"], npub_of(&public_key));
        assert!(
            report["as_event_id"]["note"]
                .as_str()
                .is_some_and(|note| note.starts_with("note1"))
        );
    }

    /// expect: a `note` round-trips through the event-id reader, and text
    /// that is no identifier is refused.
    #[test]
    fn a_note_reads_as_an_event_id_and_other_text_is_refused() {
        let id = EventId::from_slice(&[9u8; 32]).expect("32 bytes");
        let note = hkask_nostr_core::encode_note(id.as_bytes());
        let report = decode(&note).expect("a note");
        assert_eq!(report["type"], "note");
        assert_eq!(report["event_id_hex"], id.to_hex());
        assert_eq!(parse_event_id(&note), Ok(id));
        assert_eq!(parse_event_id(&id.to_hex()), Ok(id));

        let short_hex = &id.to_hex()[..63];
        for text in ["", "hello", "npub1notbech32", short_hex] {
            assert_eq!(
                decode(text),
                Err(IdentifierRefusal::NotAnIdentifier),
                "{text:?}"
            );
        }
        assert_eq!(
            parse_public_key("not-a-key"),
            Err(IdentifierRefusal::NotAnIdentifier)
        );
    }
}
