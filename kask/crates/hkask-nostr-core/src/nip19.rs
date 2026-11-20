//! NIP-19 bare identifiers: `npub` (public keys) and `note` (event ids).

use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32, ByteIterExt, Fe32IterExt, Hrp};
use thiserror::Error;

/// Why a NIP-19 identifier was not accepted. No variant carries the payload:
/// the text may be a secret key pasted into the wrong field.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    /// The text is not a valid bech32 string.
    #[error("not a bech32 string")]
    NotBech32,
    /// The text is bech32 but carries a different prefix.
    #[error("expected the `{expected}` prefix, found `{found}`")]
    WrongPrefix {
        expected: &'static str,
        found: String,
    },
    /// The payload is not 32 bytes.
    #[error("payload is {found} bytes, expected 32")]
    WrongLength { found: usize },
}

/// Decode a NIP-19 `npub` into the 32-byte x-only public key.
pub fn decode_npub(text: &str) -> Result<[u8; 32], IdentifierError> {
    decode_bare("npub", text)
}

/// Encode a 32-byte x-only public key as a NIP-19 `npub`.
pub fn encode_npub(public_key: &[u8; 32]) -> String {
    encode_bare("npub", public_key)
}

/// Decode a NIP-19 `note` into the 32-byte event id.
pub fn decode_note(text: &str) -> Result<[u8; 32], IdentifierError> {
    decode_bare("note", text)
}

/// Encode a 32-byte event id as a NIP-19 `note`.
pub fn encode_note(event_id: &[u8; 32]) -> String {
    encode_bare("note", event_id)
}

/// Bech32 (not bech32m) with no witness version, as NIP-19 specifies. The
/// iterator form cannot fail, so there is no error to hide.
fn encode_bare(prefix: &'static str, payload: &[u8; 32]) -> String {
    let hrp = Hrp::parse_unchecked(prefix);
    payload
        .iter()
        .copied()
        .bytes_to_fes()
        .with_checksum::<Bech32>(&hrp)
        .chars()
        .collect()
}

fn decode_bare(expected: &'static str, text: &str) -> Result<[u8; 32], IdentifierError> {
    let parsed = CheckedHrpstring::new::<Bech32>(text).map_err(|_| IdentifierError::NotBech32)?;
    let found = parsed.hrp().to_lowercase();
    // Checked before the payload is decoded, so a secret key pasted here is
    // never turned into bytes.
    if found != expected {
        return Err(IdentifierError::WrongPrefix { expected, found });
    }
    let payload: Vec<u8> = parsed.byte_iter().collect();
    <[u8; 32]>::try_from(payload).map_err(|payload| IdentifierError::WrongLength {
        found: payload.len(),
    })
}
