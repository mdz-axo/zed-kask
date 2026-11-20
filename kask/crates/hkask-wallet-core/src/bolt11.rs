//! BOLT11 invoice decoding and strict pre-payment validation (Threat T6).
//!
//! Uses `lightning-invoice` to parse and cryptographically recover the payee
//! key before any approval modal or spend occurs. Error variants never echo
//! the raw invoice string.

use std::str::FromStr;

use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef, Currency};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::amount::{AmountError, Millisatoshis, Satoshis};
use crate::network::WalletNetwork;

/// Decoded fields of a valid BOLT11 invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedBolt11 {
    pub network: WalletNetwork,
    pub amount_msat: Option<Millisatoshis>,
    pub amount_sats_exact: Option<Satoshis>,
    pub payment_hash: String,
    pub description: Option<String>,
    pub description_hash: Option<String>,
    pub payee_pubkey: String,
    pub timestamp_secs: u64,
    pub expiry_secs: u64,
    pub expires_at_secs: u64,
}

impl DecodedBolt11 {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at_secs
    }
}

/// Typed BOLT11 decoding and validation refusals. Never echoes the raw invoice.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Bolt11Error {
    #[error("BOLT11 invoice is empty or could not be parsed")]
    InvalidFormat,
    #[error("BOLT11 invoice targets an unsupported network (testnet3 is not supported)")]
    UnsupportedNetwork,
    #[error("BOLT11 invoice network ({actual}) does not match configured network ({expected})")]
    NetworkMismatch {
        expected: WalletNetwork,
        actual: WalletNetwork,
    },
    #[error("zero-amount (amountless) BOLT11 invoices are refused by default")]
    MissingOrZeroAmount,
    #[error("BOLT11 invoice amount ({actual}) does not match expected ({expected})")]
    AmountMismatch {
        expected: Millisatoshis,
        actual: Millisatoshis,
    },
    #[error("BOLT11 invoice amount conversion failed: {0}")]
    Amount(#[from] AmountError),
    #[error("BOLT11 invoice is expired")]
    Expired,
    #[error("BOLT11 invoice timestamp + expiry overflowed")]
    ExpiryOverflow,
    #[error("BOLT11 invoice description_hash does not match the NIP-57 zap request")]
    DescriptionHashMismatch,
}

/// Decode a BOLT11 invoice string without contacting any network service.
///
/// Strips an optional `lightning:` URI prefix (case-insensitive) and trims
/// surrounding whitespace before parsing.
pub fn decode_bolt11(raw: &str) -> Result<DecodedBolt11, Bolt11Error> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("lightning:")
        .or_else(|| trimmed.strip_prefix("LIGHTNING:"))
        .unwrap_or(trimmed)
        .trim();

    if stripped.is_empty() {
        return Err(Bolt11Error::InvalidFormat);
    }

    let invoice = Bolt11Invoice::from_str(stripped).map_err(|_| Bolt11Error::InvalidFormat)?;

    let network = match invoice.currency() {
        Currency::Bitcoin => WalletNetwork::Mainnet,
        Currency::Signet => WalletNetwork::Signet,
        Currency::Regtest => WalletNetwork::Regtest,
        Currency::BitcoinTestnet | Currency::Simnet => return Err(Bolt11Error::UnsupportedNetwork),
    };

    let amount_msat = invoice.amount_milli_satoshis().map(Millisatoshis::new);
    let amount_sats_exact = amount_msat.and_then(|msat| msat.to_satoshis_exact().ok());

    let payment_hash = invoice.payment_hash().to_string();
    let payee_pubkey = invoice.recover_payee_pub_key().to_string();

    let (description, description_hash) = match invoice.description() {
        Bolt11InvoiceDescriptionRef::Direct(desc) => (Some(desc.as_inner().to_string()), None),
        Bolt11InvoiceDescriptionRef::Hash(hash) => (None, Some(hash.0.to_string())),
    };

    let timestamp_secs = invoice.duration_since_epoch().as_secs();
    let expiry_secs = invoice.expiry_time().as_secs();
    let expires_at_secs = timestamp_secs
        .checked_add(expiry_secs)
        .ok_or(Bolt11Error::ExpiryOverflow)?;

    Ok(DecodedBolt11 {
        network,
        amount_msat,
        amount_sats_exact,
        payment_hash,
        description,
        description_hash,
        payee_pubkey,
        timestamp_secs,
        expiry_secs,
        expires_at_secs,
    })
}

/// Strictly validate a decoded BOLT11 invoice before requesting operator approval.
///
/// Enforces:
/// 1. `decoded.network == expected_network`
/// 2. Non-zero amount present, exact satoshi multiple
/// 3. Optional `expected_msat` exact match (e.g. LNURL callback verification)
/// 4. Not expired at `now_secs`
/// 5. Optional `expected_description_hash` exact match (NIP-57 zap verification)
pub fn validate_bolt11_for_payment(
    decoded: &DecodedBolt11,
    expected_network: WalletNetwork,
    expected_msat: Option<Millisatoshis>,
    expected_description_hash: Option<&str>,
    now_secs: u64,
) -> Result<Satoshis, Bolt11Error> {
    if decoded.network != expected_network {
        return Err(Bolt11Error::NetworkMismatch {
            expected: expected_network,
            actual: decoded.network,
        });
    }

    let msat = decoded
        .amount_msat
        .filter(|m| !m.is_zero())
        .ok_or(Bolt11Error::MissingOrZeroAmount)?;

    if let Some(expected) = expected_msat {
        if msat != expected {
            return Err(Bolt11Error::AmountMismatch {
                expected,
                actual: msat,
            });
        }
    }

    let sats = msat.to_satoshis_exact()?;

    if decoded.is_expired(now_secs) {
        return Err(Bolt11Error::Expired);
    }

    if let Some(expected_hash) = expected_description_hash {
        let normalized_expected = expected_hash.trim().to_ascii_lowercase();
        let actual = decoded
            .description_hash
            .as_deref()
            .map(|h| h.trim().to_ascii_lowercase());
        if actual.as_deref() != Some(normalized_expected.as_str()) {
            return Err(Bolt11Error::DescriptionHashMismatch);
        }
    }

    Ok(sats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_decoded(
        network: WalletNetwork,
        amount_msat: Option<u64>,
        expires_at_secs: u64,
        description_hash: Option<&str>,
    ) -> DecodedBolt11 {
        let amount_msat = amount_msat.map(Millisatoshis::new);
        let amount_sats_exact = amount_msat.and_then(|m| m.to_satoshis_exact().ok());
        DecodedBolt11 {
            network,
            amount_msat,
            amount_sats_exact,
            payment_hash: "00".repeat(32),
            description: if description_hash.is_none() {
                Some("coffee".to_string())
            } else {
                None
            },
            description_hash: description_hash.map(str::to_string),
            payee_pubkey: "02".to_string() + &"11".repeat(32),
            timestamp_secs: 1_700_000_000,
            expiry_secs: 3600,
            expires_at_secs,
        }
    }

    #[test]
    fn invalid_bolt11_error_never_echoes_raw_input() {
        let secret_like = "lnbc1secret_token_do_not_echo_12345";
        let err = decode_bolt11(secret_like).expect_err("must reject invalid invoice");
        let msg = err.to_string();
        assert!(!msg.contains(secret_like), "error echoed input: {msg}");
    }

    #[test]
    fn validate_bolt11_rejects_wrong_network_zero_amount_expired_and_hash_mismatch() {
        let now = 1_700_001_000;
        let ok_inv = sample_decoded(WalletNetwork::Regtest, Some(21_000), now + 600, None);
        assert_eq!(
            validate_bolt11_for_payment(
                &ok_inv,
                WalletNetwork::Regtest,
                Some(Millisatoshis::new(21_000)),
                None,
                now
            ),
            Ok(Satoshis::new(21))
        );

        // Wrong network
        assert_eq!(
            validate_bolt11_for_payment(&ok_inv, WalletNetwork::Mainnet, None, None, now),
            Err(Bolt11Error::NetworkMismatch {
                expected: WalletNetwork::Mainnet,
                actual: WalletNetwork::Regtest,
            })
        );

        // Zero / missing amount
        let zero_inv = sample_decoded(WalletNetwork::Regtest, None, now + 600, None);
        assert_eq!(
            validate_bolt11_for_payment(&zero_inv, WalletNetwork::Regtest, None, None, now),
            Err(Bolt11Error::MissingOrZeroAmount)
        );

        // Sub-satoshi amount
        let sub_sat = sample_decoded(WalletNetwork::Regtest, Some(1_500), now + 600, None);
        assert_eq!(
            validate_bolt11_for_payment(&sub_sat, WalletNetwork::Regtest, None, None, now),
            Err(Bolt11Error::Amount(AmountError::NonExactSubSatoshi))
        );

        // Expired
        let expired = sample_decoded(WalletNetwork::Regtest, Some(10_000), now, None);
        assert_eq!(
            validate_bolt11_for_payment(&expired, WalletNetwork::Regtest, None, None, now),
            Err(Bolt11Error::Expired)
        );

        // Amount substitution
        assert_eq!(
            validate_bolt11_for_payment(
                &ok_inv,
                WalletNetwork::Regtest,
                Some(Millisatoshis::new(10_000)),
                None,
                now
            ),
            Err(Bolt11Error::AmountMismatch {
                expected: Millisatoshis::new(10_000),
                actual: Millisatoshis::new(21_000),
            })
        );

        // NIP-57 description_hash mismatch
        let zap_inv = sample_decoded(
            WalletNetwork::Regtest,
            Some(21_000),
            now + 600,
            Some(&"aa".repeat(32)),
        );
        assert_eq!(
            validate_bolt11_for_payment(
                &zap_inv,
                WalletNetwork::Regtest,
                Some(Millisatoshis::new(21_000)),
                Some(&"bb".repeat(32)),
                now
            ),
            Err(Bolt11Error::DescriptionHashMismatch)
        );
        assert_eq!(
            validate_bolt11_for_payment(
                &zap_inv,
                WalletNetwork::Regtest,
                Some(Millisatoshis::new(21_000)),
                Some(&"AA".repeat(32)),
                now
            ),
            Ok(Satoshis::new(21))
        );
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn decode_bolt11_never_panics_and_never_echoes_arbitrary_input(s in ".{12,120}") {
                if let Err(err) = decode_bolt11(&s) {
                    let msg = err.to_string();
                    prop_assert!(!msg.contains(&s), "error echoed arbitrary input");
                }
            }
        }
    }
}
