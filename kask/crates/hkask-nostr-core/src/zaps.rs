//! NIP-57 Lightning Zap (`kind: 9734` / `9735`) and NIP-61 Nutzap
//! (`kind: 9321` / `10019`) parsing and verification (D110 Phase 4).
//!
//! Pure-Rust public operations only — zero secret-key code and zero
//! `libsecp256k1` dependency. Uses `bech32` (already linked here) to parse
//! BOLT11 HRP amounts and tagged `h` (`description_hash`) fields locally.

use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Event;

/// NIP-57 zap request event kind.
pub const ZAP_REQUEST_KIND: u16 = 9_734;
/// NIP-57 zap receipt event kind.
pub const ZAP_RECEIPT_KIND: u16 = 9_735;
/// NIP-61 nutzap event kind.
pub const NUTZAP_KIND: u16 = 9_321;
/// NIP-61 nutzap mint/relay/P2PK preference list kind.
pub const NUTZAP_INFO_KIND: u16 = 10_019;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ZapParseError {
    #[error("unexpected event kind (expected {expected}, got {actual})")]
    WrongKind { expected: u16, actual: u16 },
    #[error("missing required tag '{0}'")]
    MissingTag(&'static str),
    #[error("invalid embedded NIP-57 zap request in description tag")]
    InvalidZapRequest,
    #[error("NIP-61 P2PK pubkey must be 66 lowercase hex chars prefixed with '02'")]
    InvalidNip61P2pkPubkey,
    #[error("BOLT11 invoice is malformed")]
    InvalidBolt11,
    #[error("BOLT11 invoice amount ({actual:?} msat) does not match expected ({expected} msat)")]
    Bolt11AmountMismatch { expected: u64, actual: Option<u64> },
    #[error("BOLT11 invoice description_hash does not match SHA-256(zap_request_json)")]
    Bolt11DescriptionHashMismatch,
}

/// Parsed NIP-57 zap receipt (`kind: 9735`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedZapReceipt {
    pub receipt_event_id: String,
    pub zap_provider_pubkey: String,
    pub recipient_pubkey: String,
    pub zapped_event_id: Option<String>,
    pub bolt11: String,
    pub preimage: Option<String>,
    pub zap_request_sender: String,
    pub zap_request_id: String,
    pub zap_request_content: String,
}

/// Parsed NIP-61 nutzap (`kind: 9321`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedNutzap {
    pub event_id: String,
    pub sender_pubkey: String,
    pub recipient_pubkey: String,
    pub zapped_event_id: Option<String>,
    pub mint_url: String,
    pub proofs_json: Vec<String>,
    pub content: String,
}

/// Parsed NIP-61 nutzap preference list (`kind: 10019`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedNutzapInfo {
    pub author_pubkey: String,
    pub p2pk_pubkey: String,
    pub mints: Vec<String>,
    pub relays: Vec<String>,
}

fn first_tag_value<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event.tags().iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some(name) {
            tag.get(1).map(String::as_str).filter(|v| !v.is_empty())
        } else {
            None
        }
    })
}

/// Validate NIP-61's `02`-prefixed 66-char compressed P2PK pubkey rule.
pub fn validate_nip61_p2pk_pubkey(raw: &str) -> Result<String, ZapParseError> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.len() != 66
        || !trimmed.starts_with("02")
        || !trimmed.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(ZapParseError::InvalidNip61P2pkPubkey);
    }
    Ok(trimmed)
}

/// Parse and verify a NIP-57 `kind: 9735` zap receipt (verifying the embedded
/// `kind: 9734` zap request's BIP-340 signature).
pub fn parse_zap_receipt(event: &Event) -> Result<ParsedZapReceipt, ZapParseError> {
    if event.kind() != ZAP_RECEIPT_KIND {
        return Err(ZapParseError::WrongKind {
            expected: ZAP_RECEIPT_KIND,
            actual: event.kind(),
        });
    }
    let bolt11 = first_tag_value(event, "bolt11")
        .ok_or(ZapParseError::MissingTag("bolt11"))?
        .to_string();
    let description =
        first_tag_value(event, "description").ok_or(ZapParseError::MissingTag("description"))?;
    let recipient_pubkey = first_tag_value(event, "p")
        .ok_or(ZapParseError::MissingTag("p"))?
        .to_string();
    let zapped_event_id = first_tag_value(event, "e").map(str::to_string);
    let preimage = first_tag_value(event, "preimage").map(str::to_string);

    let zap_req = Event::from_json(description).map_err(|_| ZapParseError::InvalidZapRequest)?;
    if zap_req.kind() != ZAP_REQUEST_KIND {
        return Err(ZapParseError::InvalidZapRequest);
    }

    Ok(ParsedZapReceipt {
        receipt_event_id: hex::encode(event.id()),
        zap_provider_pubkey: hex::encode(event.pubkey()),
        recipient_pubkey,
        zapped_event_id,
        bolt11,
        preimage,
        zap_request_sender: hex::encode(zap_req.pubkey()),
        zap_request_id: hex::encode(zap_req.id()),
        zap_request_content: zap_req.content().to_string(),
    })
}

/// Parse a NIP-61 `kind: 9321` nutzap event.
pub fn parse_nutzap(event: &Event) -> Result<ParsedNutzap, ZapParseError> {
    if event.kind() != NUTZAP_KIND {
        return Err(ZapParseError::WrongKind {
            expected: NUTZAP_KIND,
            actual: event.kind(),
        });
    }
    let mint_url = first_tag_value(event, "u")
        .ok_or(ZapParseError::MissingTag("u"))?
        .to_string();
    let recipient_pubkey = first_tag_value(event, "p")
        .ok_or(ZapParseError::MissingTag("p"))?
        .to_string();
    let zapped_event_id = first_tag_value(event, "e").map(str::to_string);
    let proofs_json: Vec<String> = event
        .tags()
        .iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("proof"))
        .filter_map(|tag| tag.get(1).cloned())
        .filter(|v| !v.is_empty())
        .collect();
    if proofs_json.is_empty() {
        return Err(ZapParseError::MissingTag("proof"));
    }

    Ok(ParsedNutzap {
        event_id: hex::encode(event.id()),
        sender_pubkey: hex::encode(event.pubkey()),
        recipient_pubkey,
        zapped_event_id,
        mint_url,
        proofs_json,
        content: event.content().to_string(),
    })
}

/// Parse a NIP-61 `kind: 10019` nutzap preference list.
///
/// Surfaces mint URLs as untrusted data; callers MUST intersect with the
/// operator's `kask.wallet.allowed_mints` before trusting any mint.
pub fn parse_nutzap_info(event: &Event) -> Result<ParsedNutzapInfo, ZapParseError> {
    if event.kind() != NUTZAP_INFO_KIND {
        return Err(ZapParseError::WrongKind {
            expected: NUTZAP_INFO_KIND,
            actual: event.kind(),
        });
    }
    let raw_pk = first_tag_value(event, "pubkey").ok_or(ZapParseError::MissingTag("pubkey"))?;
    let trimmed_pk = raw_pk.trim().to_ascii_lowercase();
    let p2pk_pubkey = if trimmed_pk.len() == 64 && trimmed_pk.bytes().all(|b| b.is_ascii_hexdigit())
    {
        format!("02{trimmed_pk}")
    } else {
        validate_nip61_p2pk_pubkey(&trimmed_pk)?
    };
    let mints: Vec<String> = event
        .tags()
        .iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("mint"))
        .filter_map(|tag| tag.get(1).cloned())
        .filter(|v| !v.is_empty())
        .collect();
    let relays: Vec<String> = event
        .tags()
        .iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("relay"))
        .filter_map(|tag| tag.get(1).cloned())
        .filter(|v| !v.is_empty())
        .collect();

    Ok(ParsedNutzapInfo {
        author_pubkey: hex::encode(event.pubkey()),
        p2pk_pubkey,
        mints,
        relays,
    })
}

/// Parse BOLT11 HRP amount in millisatoshis without `secp256k1`.
fn parse_bolt11_hrp_msat(hrp: &str) -> Result<Option<u64>, ZapParseError> {
    let lower = hrp.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("lnbcrt") {
        r
    } else if let Some(r) = lower.strip_prefix("lntbs") {
        r
    } else if let Some(r) = lower.strip_prefix("lntb") {
        r
    } else if let Some(r) = lower.strip_prefix("lnbc") {
        r
    } else {
        return Err(ZapParseError::InvalidBolt11);
    };

    if rest.is_empty() {
        return Ok(None);
    }

    let (digits, mult_msat, div) = match rest.as_bytes().last().copied() {
        Some(b'm') => (&rest[..rest.len() - 1], 100_000_000u64, 1u64), // 1 mBTC = 10^8 msat
        Some(b'u') => (&rest[..rest.len() - 1], 100_000u64, 1u64),     // 1 uBTC = 10^5 msat
        Some(b'n') => (&rest[..rest.len() - 1], 100u64, 1u64),         // 1 nBTC = 100 msat
        Some(b'p') => (&rest[..rest.len() - 1], 1u64, 10u64),          // 1 pBTC = 0.1 msat
        Some(b) if b.is_ascii_digit() => (rest, 100_000_000_000u64, 1u64), // 1 BTC = 10^11 msat
        _ => return Err(ZapParseError::InvalidBolt11),
    };

    let n: u64 = digits.parse().map_err(|_| ZapParseError::InvalidBolt11)?;
    if div == 10 {
        if !n.is_multiple_of(10) {
            return Err(ZapParseError::InvalidBolt11);
        }
        Ok(Some(n / 10))
    } else {
        n.checked_mul(mult_msat)
            .map(Some)
            .ok_or(ZapParseError::InvalidBolt11)
    }
}

/// Convert 52 5-bit bech32 words (260 bits -> 32 bytes + 4 zero padding bits)
/// into a 32-byte array.
fn fe5_to_32_bytes(words: &[u8]) -> Result<[u8; 32], ZapParseError> {
    if words.len() != 52 {
        return Err(ZapParseError::InvalidBolt11);
    }
    let mut out = [0u8; 32];
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut idx = 0usize;
    for &w in words {
        acc = (acc << 5) | u32::from(w & 0x1f);
        bits += 5;
        while bits >= 8 && idx < 32 {
            bits -= 8;
            out[idx] = ((acc >> bits) & 0xff) as u8;
            idx += 1;
        }
    }
    if idx != 32 {
        return Err(ZapParseError::InvalidBolt11);
    }
    Ok(out)
}

/// Verify a NIP-57 zap BOLT11 invoice's `amount_msat` and `description_hash`
/// (`h` tagged field) purely via bech32 without linking `secp256k1` (Threat T6).
pub fn verify_bolt11_zap_invoice(
    bolt11: &str,
    expected_msat: u64,
    expected_description_hash_hex: &str,
) -> Result<(), ZapParseError> {
    let trimmed = bolt11
        .trim()
        .strip_prefix("lightning:")
        .or_else(|| bolt11.trim().strip_prefix("LIGHTNING:"))
        .unwrap_or_else(|| bolt11.trim());

    let checked =
        CheckedHrpstring::new::<Bech32>(trimmed).map_err(|_| ZapParseError::InvalidBolt11)?;
    let hrp = checked.hrp();
    let actual_msat = parse_bolt11_hrp_msat(hrp.as_str())?;
    if actual_msat != Some(expected_msat) {
        return Err(ZapParseError::Bolt11AmountMismatch {
            expected: expected_msat,
            actual: actual_msat,
        });
    }

    // Collect 5-bit data words: 7-word timestamp + tagged fields + 104-word signature.
    let data: Vec<u8> = checked.fe32_iter().map(|fe| fe.to_u8()).collect();
    if data.len() < 7 + 104 {
        return Err(ZapParseError::InvalidBolt11);
    }
    let tagged = &data[7..data.len() - 104];
    let mut pos = 0usize;
    let mut found_desc_hash: Option<[u8; 32]> = None;
    while pos + 3 <= tagged.len() {
        let tag_type = tagged[pos];
        let data_len = ((tagged[pos + 1] as usize) << 5) | (tagged[pos + 2] as usize);
        pos += 3;
        if pos + data_len > tagged.len() {
            return Err(ZapParseError::InvalidBolt11);
        }
        // BOLT11 tag `h` (description_hash) = 23 (bech32 char 'h' = 23), 52 5-bit words
        if tag_type == 23 && data_len == 52 {
            found_desc_hash = Some(fe5_to_32_bytes(&tagged[pos..pos + 52])?);
        }
        pos += data_len;
    }

    let expected_norm = expected_description_hash_hex.trim().to_ascii_lowercase();
    match found_desc_hash.map(hex::encode) {
        Some(actual_hex) if actual_hex == expected_norm => Ok(()),
        _ => Err(ZapParseError::Bolt11DescriptionHashMismatch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nip61_p2pk_pubkey_enforces_02_prefix_and_66_hex_chars() {
        let valid = format!("02{}", "ab".repeat(32));
        assert_eq!(validate_nip61_p2pk_pubkey(&valid), Ok(valid));

        let wrong_prefix = format!("03{}", "ab".repeat(32));
        assert_eq!(
            validate_nip61_p2pk_pubkey(&wrong_prefix),
            Err(ZapParseError::InvalidNip61P2pkPubkey)
        );
        let x_only = "ab".repeat(32);
        assert_eq!(
            validate_nip61_p2pk_pubkey(&x_only),
            Err(ZapParseError::InvalidNip61P2pkPubkey)
        );
    }

    #[test]
    fn bolt11_hrp_amount_parser_decodes_all_multipliers() {
        assert_eq!(parse_bolt11_hrp_msat("lnbc10u"), Ok(Some(1_000_000)));
        assert_eq!(parse_bolt11_hrp_msat("lnbcrt21n"), Ok(Some(2_100)));
        assert_eq!(parse_bolt11_hrp_msat("lntbs1m"), Ok(Some(100_000_000)));
        assert_eq!(parse_bolt11_hrp_msat("lnbc"), Ok(None));
    }

    #[test]
    fn verify_bolt11_zap_invoice_accepts_exact_hash_and_rejects_mismatch() {
        use bech32::{Fe32, Fe32IterExt as _, Hrp};

        let desc_hash = [0x5au8; 32];
        let desc_hash_hex = hex::encode(desc_hash);

        // Pack 32 bytes into 52 5-bit words (MSB-first, 4 zero pad bits).
        let mut h_words = Vec::with_capacity(52);
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;
        for b in desc_hash {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                h_words.push(Fe32::try_from(((acc >> bits) & 0x1f) as u8).expect("fe32"));
            }
        }
        if bits > 0 {
            h_words.push(Fe32::try_from(((acc << (5 - bits)) & 0x1f) as u8).expect("fe32"));
        }
        assert_eq!(h_words.len(), 52);

        // Build minimal BOLT11 data: 7 timestamp words + tag 'h' (23, len=52) + 104 sig words.
        let mut data = vec![Fe32::Q; 7];
        data.push(Fe32::try_from(23u8).expect("h tag"));
        data.push(Fe32::try_from((52u8 >> 5) & 0x1f).expect("len hi"));
        data.push(Fe32::try_from(52u8 & 0x1f).expect("len lo"));
        data.extend(h_words);
        data.extend(std::iter::repeat_n(Fe32::Q, 104));

        let hrp = Hrp::parse("lnbc210n").expect("hrp"); // 21,000 msat = 21 sats
        let invoice: String = data
            .into_iter()
            .with_checksum::<Bech32>(&hrp)
            .chars()
            .collect();

        // Exact match passes
        assert_eq!(
            verify_bolt11_zap_invoice(&invoice, 21_000, &desc_hash_hex),
            Ok(())
        );
        // Wrong description_hash fails closed (Threat T6)
        assert_eq!(
            verify_bolt11_zap_invoice(&invoice, 21_000, &"ff".repeat(32)),
            Err(ZapParseError::Bolt11DescriptionHashMismatch)
        );
        // Wrong amount_msat fails closed (Threat T6)
        assert_eq!(
            verify_bolt11_zap_invoice(&invoice, 10_000, &desc_hash_hex),
            Err(ZapParseError::Bolt11AmountMismatch {
                expected: 10_000,
                actual: Some(21_000),
            })
        );
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn verify_bolt11_zap_invoice_never_panics_on_arbitrary_strings(s in ".{0,200}") {
                let _ = verify_bolt11_zap_invoice(&s, 21_000, &"aa".repeat(32));
            }

            #[test]
            fn nip61_p2pk_rejects_any_non_02_prefix(prefix in 0u8..=255u8, rest in "[0-9a-f]{64}") {
                prop_assume!(prefix != 0x02);
                let key = format!("{prefix:02x}{rest}");
                prop_assert_eq!(
                    validate_nip61_p2pk_pubkey(&key),
                    Err(ZapParseError::InvalidNip61P2pkPubkey)
                );
            }
        }
    }
}
