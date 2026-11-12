//! BIP-340 signing, restricted to untagged kind 1 notes.

use chacha20poly1305::aead::Generate;
use hkask_nostr_core::{Event, event_id};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::secret::SecretKey;

/// The only event kind this crate signs: a short text note (NIP-01).
pub const NOTE_KIND: u16 = 1;

/// Why nothing was signed. No variant carries key material.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignError {
    /// The kind is not [`NOTE_KIND`]. Refused before the key is used.
    #[error("kind {kind} is not signed here; only kind 1 notes are")]
    KindNotAllowed { kind: u16 },
    /// The operating system did not supply random bytes for the signature's
    /// auxiliary randomness.
    #[error("the operating system did not supply random bytes")]
    Randomness,
    /// The curve library produced no signature, or the signature it produced
    /// did not verify. Not reachable for a valid key; present so no path
    /// panics and nothing unverified is returned.
    #[error("signing failed")]
    Signature,
}

/// Sign one untagged event with `key` and return it verified.
///
/// Only `kind` 1 is accepted. The event carries no tags, so a reply, a
/// mention or any other tagged note cannot be produced here.
pub fn sign_event(
    key: &SecretKey,
    kind: u16,
    created_at: u64,
    content: &str,
) -> Result<Event, SignError> {
    if kind != NOTE_KIND {
        return Err(SignError::KindNotAllowed { kind });
    }
    let pubkey = key.x_only_public_key();
    let id = event_id(&pubkey, created_at, kind, &[], content);
    let aux_rand = Zeroizing::new(<[u8; 32]>::try_generate().map_err(|_| SignError::Randomness)?);
    let signature = sign_with_aux(key, &id, &aux_rand)?;
    // `Event::from_parts` recomputes the id and verifies the signature, so an
    // event that does not verify never leaves this function.
    Event::from_parts(
        pubkey,
        created_at,
        kind,
        Vec::new(),
        content.to_string(),
        signature,
    )
    .map_err(|_| SignError::Signature)
}

/// BIP-340 signature of `message` under `key` with the given auxiliary
/// randomness. [`sign_event`] supplies fresh randomness; tests supply the
/// published values.
fn sign_with_aux(
    key: &SecretKey,
    message: &[u8],
    aux_rand: &[u8; 32],
) -> Result<[u8; 64], SignError> {
    key.sign_raw(message, aux_rand).ok_or(SignError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BIP-340's test vectors: the same unmodified file the core crate's
    /// verification test reads.
    const BIP340_VECTORS: &str =
        include_str!("../../hkask-nostr-core/tests/fixtures/bip340-test-vectors.csv");

    /// NIP-19's published `nsec` example and the public key NIP-19 lists for
    /// it. A throwaway key from the specification.
    const NIP19_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const NIP19_PUBKEY_HEX: &str =
        "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";

    fn bytes<const N: usize>(text: &str, what: &str, index: &str) -> [u8; N] {
        hex::decode(text)
            .unwrap_or_else(|error| panic!("vector {index}: {what}: {error}"))
            .try_into()
            .unwrap_or_else(|_| panic!("vector {index}: {what} is not {N} bytes"))
    }

    /// expect: this crate signs exactly as BIP-340 specifies, and derives the
    /// public key BIP-340 states for each secret key.
    /// pre: every published vector that carries a secret key (0 to 3 and 15 to
    /// 18; message sizes 32, 0, 1, 17 and 100 bytes).
    /// post: the derived public key and the signature equal the published
    /// "public key" and "signature" columns.
    #[test]
    fn bip340_published_signing_vectors_produce_published_signatures() {
        let mut signed = 0;
        for line in BIP340_VECTORS.lines().skip(1) {
            let fields: Vec<&str> = line.splitn(8, ',').collect();
            assert_eq!(fields.len(), 8, "malformed vector line: {line}");
            let index = fields[0];
            if fields[1].is_empty() {
                continue;
            }
            let key = SecretKey::from_bytes(&bytes::<32>(fields[1], "secret key", index))
                .unwrap_or_else(|error| panic!("vector {index}: secret key: {error}"));
            assert_eq!(
                hex::encode_upper(key.x_only_public_key()),
                fields[2],
                "vector {index}: public key"
            );
            let aux_rand = bytes::<32>(fields[3], "aux_rand", index);
            let message = hex::decode(fields[4])
                .unwrap_or_else(|error| panic!("vector {index}: message: {error}"));
            let signature = sign_with_aux(&key, &message, &aux_rand)
                .unwrap_or_else(|error| panic!("vector {index}: {error}"));
            assert_eq!(
                hex::encode_upper(signature),
                fields[5],
                "vector {index}: signature"
            );
            signed += 1;
        }
        assert_eq!(signed, 8, "the fixture holds 8 vectors with a secret key");
    }

    /// expect: a kind 1 note is signed into an event any client can verify.
    /// pre: NIP-19's example key, time 1700000000, content `hello nostr`.
    /// post: the event's id is the SHA-256 (coreutils `sha256sum`) of
    /// `[0,"7e7e…df4e",1700000000,1,[],"hello nostr"]`; author, kind, time and
    /// content are as given; it has no tags; and its JSON form verifies when
    /// read back through the core crate.
    #[test]
    fn a_kind_1_note_is_signed_into_a_verifiable_event() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let event = sign_event(&key, 1, 1_700_000_000, "hello nostr")
            .unwrap_or_else(|error| panic!("a kind 1 note must sign: {error}"));
        assert_eq!(
            hex::encode(event.id()),
            "03b64435a898c62cf0cd7e19eb48af2b6ae2787457a1e1f475f8f0368907fade"
        );
        assert_eq!(hex::encode(event.pubkey()), NIP19_PUBKEY_HEX);
        assert_eq!(event.kind(), 1);
        assert_eq!(event.created_at(), 1_700_000_000);
        assert_eq!(event.content(), "hello nostr");
        assert!(event.tags().is_empty());
        let read_back = Event::from_json(&event.to_json())
            .unwrap_or_else(|error| panic!("the signed event must verify: {error}"));
        assert_eq!(read_back, event);
    }

    /// expect: nothing but a kind 1 note is signed.
    /// pre: metadata (0), contacts (3), an encrypted DM (4), a deletion (5), a
    /// repost (6), a reaction (7), a seal (13), a gift wrap (1059), a comment
    /// (1111), a relay list (10002), an article (30023), and the edges 2 and
    /// 65535.
    /// post: `KindNotAllowed` naming the kind, for each. The refusal is the
    /// whole result: there is no event and no signature in it.
    #[test]
    fn any_kind_other_than_1_is_refused_without_a_signature() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        for kind in [0u16, 2, 3, 4, 5, 6, 7, 13, 1059, 1111, 10002, 30023, 65535] {
            assert_eq!(
                sign_event(&key, kind, 1_700_000_000, "hello nostr"),
                Err(SignError::KindNotAllowed { kind })
            );
        }
    }

    /// expect: each signature uses fresh auxiliary randomness, as BIP-340
    /// recommends.
    /// pre: the same key, time and content signed twice.
    /// post: the two events have the same id and different signatures; both
    /// verified when they were built.
    #[test]
    fn signing_the_same_note_twice_gives_two_signatures() {
        let key = SecretKey::from_nsec(NIP19_NSEC).expect("published nsec parses");
        let first = sign_event(&key, 1, 1_700_000_000, "hello nostr").expect("signs");
        let second = sign_event(&key, 1, 1_700_000_000, "hello nostr").expect("signs again");
        assert_eq!(first.id(), second.id());
        assert_ne!(first.sig(), second.sig());
    }
}
