//! NIP-01 events: the event id, and verification of an event's id and
//! signature.

use k256::sha2::{Digest, Sha256};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::verify_schnorr;

/// Why text or parts did not yield a verified event. No variant carries the
/// event's content, which is untrusted third-party text.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventError {
    /// The text is not a NIP-01 event: not JSON, a field is missing or of the
    /// wrong type, or a hex field has the wrong length or case.
    #[error("not a NIP-01 event")]
    Malformed,
    /// The `id` is not the SHA-256 of the serialized event.
    #[error("the event id is not the hash of the event")]
    IdMismatch,
    /// The `sig` is not a valid signature of the id by `pubkey`.
    #[error("the event signature does not verify")]
    BadSignature,
}

/// A NIP-01 event whose id and signature have been checked. There is no way
/// to build one that has not been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    id: [u8; 32],
    pubkey: [u8; 32],
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
    sig: [u8; 64],
}

/// The wire form. Unknown fields are ignored; relays add some.
#[derive(Deserialize)]
struct RawEvent {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
    sig: String,
}

impl Event {
    /// Parse a NIP-01 event from JSON and verify its id and signature.
    pub fn from_json(text: &str) -> Result<Self, EventError> {
        let raw: RawEvent = serde_json::from_str(text).map_err(|_| EventError::Malformed)?;
        let claimed_id = lower_hex::<32>(&raw.id).ok_or(EventError::Malformed)?;
        let pubkey = lower_hex::<32>(&raw.pubkey).ok_or(EventError::Malformed)?;
        let sig = lower_hex::<64>(&raw.sig).ok_or(EventError::Malformed)?;

        let id = event_id(&pubkey, raw.created_at, raw.kind, &raw.tags, &raw.content);
        if id != claimed_id {
            return Err(EventError::IdMismatch);
        }
        Self::verified(
            id,
            pubkey,
            raw.created_at,
            raw.kind,
            raw.tags,
            raw.content,
            sig,
        )
    }

    /// Build an event from its parts, computing the id and verifying the
    /// signature over it.
    pub fn from_parts(
        pubkey: [u8; 32],
        created_at: u64,
        kind: u16,
        tags: Vec<Vec<String>>,
        content: String,
        sig: [u8; 64],
    ) -> Result<Self, EventError> {
        let id = event_id(&pubkey, created_at, kind, &tags, &content);
        Self::verified(id, pubkey, created_at, kind, tags, content, sig)
    }

    fn verified(
        id: [u8; 32],
        pubkey: [u8; 32],
        created_at: u64,
        kind: u16,
        tags: Vec<Vec<String>>,
        content: String,
        sig: [u8; 64],
    ) -> Result<Self, EventError> {
        verify_schnorr(&pubkey, &id, &sig).map_err(|_| EventError::BadSignature)?;
        Ok(Self {
            id,
            pubkey,
            created_at,
            kind,
            tags,
            content,
            sig,
        })
    }

    /// The event as NIP-01 JSON on one line, for sending to a relay.
    pub fn to_json(&self) -> String {
        let mut object = Map::new();
        object.insert("id".to_string(), Value::from(hex::encode(self.id)));
        object.insert("pubkey".to_string(), Value::from(hex::encode(self.pubkey)));
        object.insert("created_at".to_string(), Value::from(self.created_at));
        object.insert("kind".to_string(), Value::from(self.kind));
        object.insert("tags".to_string(), tags_value(&self.tags));
        object.insert("content".to_string(), Value::from(self.content.as_str()));
        object.insert("sig".to_string(), Value::from(hex::encode(self.sig)));
        Value::Object(object).to_string()
    }

    /// The event id: the SHA-256 of the serialized event.
    pub fn id(&self) -> &[u8; 32] {
        &self.id
    }

    /// The author's x-only public key.
    pub fn pubkey(&self) -> &[u8; 32] {
        &self.pubkey
    }

    /// The author's claimed creation time, in seconds since the Unix epoch.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// The event kind.
    pub fn kind(&self) -> u16 {
        self.kind
    }

    /// The event's tags.
    pub fn tags(&self) -> &[Vec<String>] {
        &self.tags
    }

    /// The event's content: untrusted text written by the author.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The BIP-340 signature of the id.
    pub fn sig(&self) -> &[u8; 64] {
        &self.sig
    }
}

/// The NIP-01 event id: SHA-256 of
/// `[0,<pubkey hex>,<created_at>,<kind>,<tags>,<content>]`, serialized with no
/// whitespace.
///
/// Escaping is `serde_json`'s: the seven characters NIP-01 names (`\n`, `\"`,
/// `\\`, `\r`, `\t`, `\b`, `\f`) as NIP-01 writes them, and every other
/// character verbatim, with one exception. NIP-01 says all other characters
/// are included verbatim, but a raw control character is not valid JSON, so
/// the remaining ones below U+0020 are written as `\u00XX`.
pub fn event_id(
    pubkey: &[u8; 32],
    created_at: u64,
    kind: u16,
    tags: &[Vec<String>],
    content: &str,
) -> [u8; 32] {
    let serialized = Value::Array(vec![
        Value::from(0u8),
        Value::from(hex::encode(pubkey)),
        Value::from(created_at),
        Value::from(kind),
        tags_value(tags),
        Value::from(content),
    ])
    .to_string();
    Sha256::digest(serialized.as_bytes()).into()
}

/// Maximum NIP-13 proof-of-work difficulty supported by `mine_nip13_nonce`.
/// Capped at 24 bits (~16M expected hashes) so signing stays interactive.
pub const MAX_NIP13_POW_BITS: u8 = 24;

/// Count the number of leading zero bits in a 32-byte NIP-01 event id (NIP-13).
pub fn leading_zero_bits(id: &[u8; 32]) -> u8 {
    let mut bits = 0u8;
    for &byte in id {
        if byte == 0 {
            bits = bits.saturating_add(8);
        } else {
            bits = bits.saturating_add(byte.leading_zeros() as u8);
            break;
        }
    }
    bits
}

/// Mine a NIP-13 `["nonce", "<counter>", "<target_bits>"]` tag so the resulting
/// NIP-01 event id has at least `target_bits` leading zero bits.
///
/// Returns `None` if `target_bits == 0`, `target_bits > MAX_NIP13_POW_BITS`, or
/// the bounded iteration cap is reached without finding a match.
pub fn mine_nip13_nonce(
    pubkey: &[u8; 32],
    created_at: u64,
    kind: u16,
    base_tags: &[Vec<String>],
    content: &str,
    target_bits: u8,
) -> Option<Vec<String>> {
    if target_bits == 0 || target_bits > MAX_NIP13_POW_BITS {
        return None;
    }
    let max_attempts = (1u64 << target_bits).saturating_mul(16).max(65_536);
    let target_str = target_bits.to_string();
    let mut tags: Vec<Vec<String>> = base_tags
        .iter()
        .filter(|t| t.first().map(String::as_str) != Some("nonce"))
        .cloned()
        .collect();
    let nonce_idx = tags.len();
    tags.push(vec!["nonce".to_string(), String::new(), target_str]);

    for nonce in 0..max_attempts {
        tags[nonce_idx][1] = nonce.to_string();
        let id = event_id(pubkey, created_at, kind, &tags, content);
        if leading_zero_bits(&id) >= target_bits {
            return Some(tags.pop().expect("nonce tag present"));
        }
    }
    None
}

fn tags_value(tags: &[Vec<String>]) -> Value {
    Value::Array(
        tags.iter()
            .map(|tag| Value::Array(tag.iter().map(|item| Value::from(item.as_str())).collect()))
            .collect(),
    )
}

/// Decode exactly `N` bytes of lower-case hex. NIP-01 requires lower case, and
/// two spellings of one id would otherwise name the same event.
fn lower_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let well_formed = text.len() == N * 2
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if !well_formed {
        return None;
    }
    hex::decode(text).ok()?.try_into().ok()
}

#[cfg(test)]
mod nip13_tests {
    use super::*;

    #[test]
    fn leading_zero_bits_counts_prefix_zeros() {
        let mut id = [0u8; 32];
        id[0] = 0x00;
        id[1] = 0x0f;
        assert_eq!(leading_zero_bits(&id), 12);
        id[0] = 0x80;
        assert_eq!(leading_zero_bits(&id), 0);
        assert_eq!(leading_zero_bits(&[0u8; 32]), 255);
    }

    #[test]
    fn mine_nip13_nonce_meets_target_and_rejects_out_of_range() {
        let pubkey = [7u8; 32];
        assert!(mine_nip13_nonce(&pubkey, 1_700_000_000, 1, &[], "hi", 0).is_none());
        assert!(
            mine_nip13_nonce(&pubkey, 1_700_000_000, 1, &[], "hi", MAX_NIP13_POW_BITS + 1)
                .is_none()
        );

        let tag = mine_nip13_nonce(&pubkey, 1_700_000_000, 1, &[], "hello nip-13", 10)
            .expect("mines 10-bit nonce");
        assert_eq!(tag.len(), 3);
        assert_eq!(tag[0], "nonce");
        assert_eq!(tag[2], "10");

        let id = event_id(&pubkey, 1_700_000_000, 1, &[tag], "hello nip-13");
        assert!(leading_zero_bits(&id) >= 10);
    }
}
