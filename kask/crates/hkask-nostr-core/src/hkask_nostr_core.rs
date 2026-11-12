#![forbid(unsafe_code)]
//! Public Nostr operations: nothing in this crate reads, derives or uses a
//! secret key.
//!
//! The Nostr MCP server parses untrusted relay text, so it may depend on this
//! crate and never on `hkask-nostr-signer`. That keeps key decryption and the
//! code that holds a key out of the server.
//!
//! Known limit: the curve library (`k256`) compiles its Schnorr signing and
//! verifying code under one feature, so its `SigningKey` type is linked here
//! too. Nothing in this crate constructs one.

mod event;
mod nip19;

pub use event::{Event, EventError, event_id};
pub use nip19::{IdentifierError, decode_note, decode_npub, encode_note, encode_npub};

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
