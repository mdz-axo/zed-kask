//! The crate's public functions against published test data. Every expected
//! value here is copied from a specification, never computed by this crate.

use hkask_nostr_core::{IdentifierError, decode_npub, verify_schnorr};

/// BIP-340's test vectors, copied unmodified from
/// `https://github.com/bitcoin/bips/blob/master/bip-0340/test-vectors.csv`
/// (sha256 `34c9d1d9c3a88d524bc80778540dc43f8306ec249a7485293063c376db851c2d`).
const BIP340_VECTORS: &str = include_str!("fixtures/bip340-test-vectors.csv");

fn hex_array<const N: usize>(text: &str, what: &str, index: &str) -> [u8; N] {
    let bytes = hex::decode(text).unwrap_or_else(|error| panic!("vector {index}: {what}: {error}"));
    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("vector {index}: {what} is not {N} bytes"))
}

/// expect: every BIP-340 vector marked TRUE verifies and every vector marked
/// FALSE is refused, so a relay cannot attribute text to a key that did not
/// sign it.
/// pre: the 19 published vectors (9 valid, 10 invalid).
/// post: the verdict equals the published "verification result" column.
#[test]
fn bip340_published_vectors_verify_or_refuse_as_published() {
    let mut accepted = 0;
    let mut refused = 0;
    for line in BIP340_VECTORS.lines().skip(1) {
        let fields: Vec<&str> = line.splitn(8, ',').collect();
        assert_eq!(fields.len(), 8, "malformed vector line: {line}");
        let index = fields[0];
        let public_key = hex_array::<32>(fields[2], "public key", index);
        let message = hex::decode(fields[4])
            .unwrap_or_else(|error| panic!("vector {index}: message: {error}"));
        let signature = hex_array::<64>(fields[5], "signature", index);
        let published = match fields[6] {
            "TRUE" => true,
            "FALSE" => false,
            other => panic!("vector {index}: unexpected verification result {other}"),
        };

        let verdict = verify_schnorr(&public_key, &message, &signature);
        assert_eq!(
            verdict.is_ok(),
            published,
            "vector {index} ({}): got {verdict:?}",
            fields[7]
        );
        if published {
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    assert_eq!((accepted, refused), (9, 10), "the fixture holds 19 vectors");
}

/// expect: NIP-19's published `npub` examples decode to the published keys.
/// pre: the two `npub` examples in NIP-19.
/// post: each decodes to the hex key the NIP states.
#[test]
fn nip19_published_npub_examples_decode_to_published_keys() {
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
        let decoded = decode_npub(npub).unwrap_or_else(|error| panic!("{npub}: {error}"));
        assert_eq!(hex::encode(decoded), key_hex, "{npub}");
    }
}

/// expect: a secret key pasted where a public key belongs is refused, and the
/// refusal does not repeat the key.
/// pre: NIP-19's published `nsec` example (a throwaway key from the spec).
/// post: `WrongPrefix`, whose text contains neither the bech32 nor the hex key.
#[test]
fn decode_npub_refuses_an_nsec_without_echoing_it() {
    let nsec = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    let error = decode_npub(nsec).expect_err("an nsec is not an npub");
    assert_eq!(
        error,
        IdentifierError::WrongPrefix {
            expected: "npub",
            found: "nsec".to_string(),
        }
    );
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains("vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laq"));
    assert!(!shown.contains("67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa"));
}
