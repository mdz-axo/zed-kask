#![forbid(unsafe_code)]
//! Public Nostr operations: nothing in this crate reads, derives or uses a
//! secret key.
//!
//! The Nostr MCP server parses untrusted relay text, so it may depend on this
//! crate and never on `hkask-nostr-signer`. Keeping the two apart at the crate
//! boundary means a package that does not depend on the signer cannot link
//! signing code under any build.

use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;
use k256::schnorr::{Signature, VerifyingKey};
use thiserror::Error;

/// Why a signature was not accepted.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerifyError {
    /// The 32 bytes are not the x coordinate of a point on secp256k1.
    #[error("public key is not a valid x-only secp256k1 key")]
    InvalidPublicKey,
    /// The signature is not a well-formed BIP-340 signature, or it does not
    /// verify for this key and message.
    #[error("signature does not verify")]
    BadSignature,
}

/// Verify a BIP-340 Schnorr signature over `message` for an x-only public key.
pub fn verify_schnorr(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), VerifyError> {
    let key = VerifyingKey::from_slice(public_key).map_err(|_| VerifyError::InvalidPublicKey)?;
    let signature = Signature::from_bytes(signature).map_err(|_| VerifyError::BadSignature)?;
    key.verify_raw(message, &signature)
        .map_err(|_| VerifyError::BadSignature)
}

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
    let parsed = CheckedHrpstring::new::<Bech32>(text).map_err(|_| IdentifierError::NotBech32)?;
    let found = parsed.hrp().to_lowercase();
    // Checked before the payload is decoded, so a secret key pasted here is
    // never turned into bytes.
    if found != "npub" {
        return Err(IdentifierError::WrongPrefix {
            expected: "npub",
            found,
        });
    }
    let payload: Vec<u8> = parsed.byte_iter().collect();
    <[u8; 32]>::try_from(payload).map_err(|payload| IdentifierError::WrongLength {
        found: payload.len(),
    })
}
