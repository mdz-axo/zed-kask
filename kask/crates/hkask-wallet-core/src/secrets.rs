//! Zeroizing secret helpers and BIP-39 / NWC URI validation (Threat T2).
//!
//! Error variants never echo raw mnemonics, passphrases, or NWC secrets.

use rand::TryRngCore as _;
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use url::Url;
use zeroize::Zeroizing;

/// Non-secret summary extracted from a validated NIP-47 NWC connection URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwcUriSummary {
    /// Wallet service 64-char hex pubkey (public).
    pub wallet_pubkey_hex: String,
    /// Relay URL(s) declared in the NWC URI (public).
    pub relays: Vec<String>,
}

/// Refusal reasons for secret validation. Never echoes secret material.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretValidationError {
    #[error("BIP-39 mnemonic is empty")]
    EmptyMnemonic,
    #[error("BIP-39 mnemonic must be 12 or 24 valid English words with a valid checksum")]
    InvalidMnemonic,
    #[error("NWC URI is empty")]
    EmptyNwcUri,
    #[error("NWC URI must use the nostr+walletconnect:// scheme")]
    InvalidNwcScheme,
    #[error("NWC URI must name a 64-character hex wallet pubkey")]
    InvalidNwcPubkey,
    #[error("NWC URI must include at least one ws:// or wss:// relay query parameter")]
    MissingNwcRelay,
    #[error("NWC URI must include a 64-character hex secret query parameter")]
    InvalidNwcSecret,
    #[error("OS CSPRNG failed while generating wallet entropy")]
    RngUnavailable,
}

/// Validate a 12- or 24-word BIP-39 mnemonic phrase (NFKD-normalized English).
/// Returns the word count (12 or 24) without echoing the phrase.
pub fn validate_bip39_mnemonic(phrase: &str) -> Result<usize, SecretValidationError> {
    let trimmed = phrase.trim();
    if trimmed.is_empty() {
        return Err(SecretValidationError::EmptyMnemonic);
    }
    let mnemonic = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, trimmed)
        .map_err(|_| SecretValidationError::InvalidMnemonic)?;
    let count = mnemonic.word_count();
    if count != 12 && count != 24 {
        return Err(SecretValidationError::InvalidMnemonic);
    }
    Ok(count)
}

/// Derive the standard 64-byte BIP-39 seed (empty passphrase, per Cashu NUT-13)
/// from a validated 12- or 24-word English mnemonic. Zeroized on drop.
pub fn derive_nut13_seed_from_mnemonic(
    phrase: &str,
) -> Result<Zeroizing<[u8; 64]>, SecretValidationError> {
    let trimmed = phrase.trim();
    if trimmed.is_empty() {
        return Err(SecretValidationError::EmptyMnemonic);
    }
    let mnemonic = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, trimmed)
        .map_err(|_| SecretValidationError::InvalidMnemonic)?;
    let count = mnemonic.word_count();
    if count != 12 && count != 24 {
        return Err(SecretValidationError::InvalidMnemonic);
    }
    Ok(Zeroizing::new(mnemonic.to_seed("")))
}

/// Derive the standard 64-byte BIP-39 seed (empty passphrase, per Cashu NUT-13)
/// as a lowercase 128-character hex string. Zeroized on drop.
pub fn derive_nut13_seed_hex_from_mnemonic(
    phrase: &str,
) -> Result<Zeroizing<String>, SecretValidationError> {
    let seed = derive_nut13_seed_from_mnemonic(phrase)?;
    Ok(Zeroizing::new(hex::encode(&seed[..])))
}

/// Generate a fresh 12-word BIP-39 mnemonic from 128 bits of OS CSPRNG entropy.
pub fn generate_bip39_mnemonic_12() -> Result<Zeroizing<String>, SecretValidationError> {
    let mut entropy = Zeroizing::new([0u8; 16]);
    rand::rngs::OsRng
        .try_fill_bytes(entropy.as_mut())
        .map_err(|_| SecretValidationError::RngUnavailable)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy[..])
        .map_err(|_| SecretValidationError::InvalidMnemonic)?;
    Ok(Zeroizing::new(mnemonic.to_string()))
}

/// Generate a high-entropy 32-byte (64-hex-char) SQLCipher DB passphrase.
pub fn generate_db_passphrase_hex() -> Result<Zeroizing<String>, SecretValidationError> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng
        .try_fill_bytes(bytes.as_mut())
        .map_err(|_| SecretValidationError::RngUnavailable)?;
    Ok(Zeroizing::new(hex::encode(&bytes[..])))
}

fn is_64_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate a NIP-47 `nostr+walletconnect://<pubkey>?relay=...&secret=<hex>` URI
/// and return only its non-secret public summary.
pub fn validate_nwc_uri(raw: &str) -> Result<NwcUriSummary, SecretValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SecretValidationError::EmptyNwcUri);
    }

    let parsed = Url::parse(trimmed).map_err(|_| SecretValidationError::InvalidNwcScheme)?;
    if parsed.scheme() != "nostr+walletconnect" {
        return Err(SecretValidationError::InvalidNwcScheme);
    }

    let pubkey = parsed
        .host_str()
        .ok_or(SecretValidationError::InvalidNwcPubkey)?;
    if !is_64_hex(pubkey) {
        return Err(SecretValidationError::InvalidNwcPubkey);
    }

    let mut relays = Vec::new();
    let mut has_valid_secret = false;
    for (k, v) in parsed.query_pairs() {
        if k == "relay" {
            let r = v.trim();
            if r.starts_with("wss://") || r.starts_with("ws://") {
                relays.push(r.to_string());
            }
        } else if k == "secret" {
            has_valid_secret = is_64_hex(v.trim());
        }
    }

    if relays.is_empty() {
        return Err(SecretValidationError::MissingNwcRelay);
    }
    if !has_valid_secret {
        return Err(SecretValidationError::InvalidNwcSecret);
    }

    Ok(NwcUriSummary {
        wallet_pubkey_hex: pubkey.to_ascii_lowercase(),
        relays,
    })
}

/// Compute lowercase 64-char hex SHA-256 of arbitrary bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_mnemonic_and_passphrase_validate_cleanly() {
        let mnemonic = generate_bip39_mnemonic_12().expect("mnemonic generated");
        assert_eq!(validate_bip39_mnemonic(&mnemonic), Ok(12));

        let passphrase = generate_db_passphrase_hex().expect("passphrase generated");
        assert_eq!(passphrase.len(), 64);
        assert!(passphrase.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn invalid_mnemonic_error_never_echoes_secret_words() {
        let bogus = "abandon abandon abandon secretword12345 notamnemomic";
        let err = validate_bip39_mnemonic(bogus).expect_err("must reject invalid mnemonic");
        assert!(!err.to_string().contains("secretword12345"));
    }

    #[test]
    fn nwc_uri_validation_checks_shape_and_never_echoes_secret() {
        let pubkey = "a".repeat(64);
        let secret = "b".repeat(64);
        let valid =
            format!("nostr+walletconnect://{pubkey}?relay=wss://relay.example.com&secret={secret}");
        let summary = validate_nwc_uri(&valid).expect("valid NWC URI");
        assert_eq!(summary.wallet_pubkey_hex, pubkey);
        assert_eq!(summary.relays, vec!["wss://relay.example.com".to_string()]);

        let bad_secret = format!(
            "nostr+walletconnect://{pubkey}?relay=wss://relay.example.com&secret=supersecret_short"
        );
        let err = validate_nwc_uri(&bad_secret).expect_err("short secret must fail");
        assert!(!err.to_string().contains("supersecret_short"));
    }
}
