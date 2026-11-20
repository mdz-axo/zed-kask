//! Spend intents, editor-side spending cap policy, rolling 24h budget ledger,
//! and short-lived HMAC-SHA256 approval receipts (Threats T1, T4, T7, T8, T10).

use hmac::{Hmac, Mac as _};
use rand::TryRngCore as _;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::amount::{AmountError, Satoshis};
use crate::network::{PaymentRail, WalletNetwork};
use crate::url_guard::validate_public_https_url;

type HmacSha256 = Hmac<Sha256>;

/// Rolling budget window duration: 24 hours in seconds.
pub const ROLLING_WINDOW_SECS: u64 = 86_400;

/// Default TTL of a minted spend approval receipt: 120 seconds.
pub const APPROVAL_RECEIPT_TTL_SECS: u64 = 120;

/// A single bounded payment intent submitted for native editor approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendIntent {
    pub intent_id: String,
    pub idempotency_key: String,
    pub rail: PaymentRail,
    pub network: WalletNetwork,
    pub amount: Satoshis,
    pub max_fee: Satoshis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mint_url: Option<String>,
    pub destination_summary: String,
    pub destination_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bolt11: Option<String>,
    pub created_at: u64,
}

impl SpendIntent {
    /// Total maximum satoshi commitment (`amount + max_fee`), checked for overflow.
    pub fn total_commitment(&self) -> Result<Satoshis, AmountError> {
        self.amount.checked_add(self.max_fee)
    }

    /// Structural validation of the intent before policy evaluation.
    pub fn validate(&self) -> Result<(), PolicyRefusal> {
        if self.intent_id.trim().is_empty() {
            return Err(PolicyRefusal::InvalidIntent("intent_id is empty"));
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(PolicyRefusal::InvalidIntent("idempotency_key is empty"));
        }
        if self.amount.is_zero() {
            return Err(PolicyRefusal::ZeroAmount);
        }
        if self.destination_summary.trim().is_empty() {
            return Err(PolicyRefusal::InvalidIntent("destination_summary is empty"));
        }
        if self.destination_hash.len() != 64
            || !self.destination_hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(PolicyRefusal::InvalidIntent(
                "destination_hash must be 64 hex characters",
            ));
        }
        if self.rail == PaymentRail::Cashu
            && self
                .mint_url
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            return Err(PolicyRefusal::InvalidIntent(
                "Cashu spend intent must specify mint_url",
            ));
        }
        self.total_commitment().map_err(PolicyRefusal::Amount)?;
        Ok(())
    }
}

/// Editor-enforced spending policy derived from `KaskWalletSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendCapPolicy {
    pub network: WalletNetwork,
    pub mainnet_enabled: bool,
    pub per_tx_cap_sats: Satoshis,
    pub daily_cap_sats: Satoshis,
    pub max_fee_sats: Satoshis,
    pub allowed_mints: Vec<String>,
}

/// Typed policy refusal reasons. All fail closed before any modal or keychain read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolicyRefusal {
    #[error("invalid spend intent: {0}")]
    InvalidIntent(&'static str),
    #[error("spend amount must be greater than zero satoshis")]
    ZeroAmount,
    #[error("mainnet spending is disabled in settings (kask.wallet.mainnet_enabled is false)")]
    MainnetDisabled,
    #[error("intent network ({actual}) does not match configured wallet network ({expected})")]
    NetworkMismatch {
        expected: WalletNetwork,
        actual: WalletNetwork,
    },
    #[error("Cashu mint URL is not valid for the configured network")]
    InvalidMintUrl,
    #[error("Cashu mint URL is not in kask.wallet.allowed_mints")]
    MintNotAllowlisted,
    #[error("spend amount ({amount}) exceeds per-transaction cap ({cap})")]
    PerTxCapExceeded { amount: Satoshis, cap: Satoshis },
    #[error("max fee ({fee}) exceeds configured max_fee_sats ({cap})")]
    FeeCapExceeded { fee: Satoshis, cap: Satoshis },
    #[error(
        "rolling 24h spend cap exceeded (spent {spent_24h} + requested {requested} > cap {cap})"
    )]
    DailyCapExceeded {
        spent_24h: Satoshis,
        requested: Satoshis,
        cap: Satoshis,
    },
    #[error("duplicate active reservation for intent_id")]
    DuplicateIntent,
    #[error("amount arithmetic error: {0}")]
    Amount(#[from] AmountError),
    #[error("approval receipt expired")]
    ReceiptExpired,
    #[error("approval receipt HMAC verification failed")]
    ReceiptInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpendReservation {
    intent_id: String,
    total_sats: Satoshis,
    timestamp_secs: u64,
}

/// Editor-owned rolling 24-hour spend ledger used to enforce `daily_cap_sats`.
#[derive(Debug, Default, Clone)]
pub struct RollingSpendLedger {
    reservations: Vec<SpendReservation>,
}

impl RollingSpendLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop reservations older than 24 hours relative to `now_secs`.
    pub fn prune(&mut self, now_secs: u64) {
        self.reservations
            .retain(|r| now_secs.saturating_sub(r.timestamp_secs) < ROLLING_WINDOW_SECS);
    }

    /// Sum of reserved/settled satoshis within the rolling 24h window.
    pub fn spent_in_window(&self, now_secs: u64) -> Result<Satoshis, AmountError> {
        let mut total = Satoshis::ZERO;
        for r in &self.reservations {
            if now_secs.saturating_sub(r.timestamp_secs) < ROLLING_WINDOW_SECS {
                total = total.checked_add(r.total_sats)?;
            }
        }
        Ok(total)
    }

    /// Check `intent` against `policy` and the rolling 24h window.
    /// Returns the remaining 24h budget *after* this transaction if admitted.
    pub fn check_policy(
        &self,
        policy: &SpendCapPolicy,
        intent: &SpendIntent,
        now_secs: u64,
    ) -> Result<Satoshis, PolicyRefusal> {
        intent.validate()?;

        if intent.network != policy.network {
            return Err(PolicyRefusal::NetworkMismatch {
                expected: policy.network,
                actual: intent.network,
            });
        }

        if policy.network.is_mainnet() && !policy.mainnet_enabled {
            return Err(PolicyRefusal::MainnetDisabled);
        }

        if intent.rail == PaymentRail::Cashu {
            let raw_mint = intent
                .mint_url
                .as_deref()
                .ok_or(PolicyRefusal::InvalidMintUrl)?;
            let normalized_mint =
                validate_public_https_url(raw_mint, policy.network.allows_loopback_urls())
                    .map_err(|_| PolicyRefusal::InvalidMintUrl)?;
            let allowlisted = policy.allowed_mints.iter().any(|allowed| {
                validate_public_https_url(allowed, policy.network.allows_loopback_urls())
                    .is_ok_and(|a| a.as_str() == normalized_mint.as_str())
            });
            if !allowlisted {
                return Err(PolicyRefusal::MintNotAllowlisted);
            }
        }

        if intent.amount > policy.per_tx_cap_sats {
            return Err(PolicyRefusal::PerTxCapExceeded {
                amount: intent.amount,
                cap: policy.per_tx_cap_sats,
            });
        }

        if intent.max_fee > policy.max_fee_sats {
            return Err(PolicyRefusal::FeeCapExceeded {
                fee: intent.max_fee,
                cap: policy.max_fee_sats,
            });
        }

        let requested = intent.total_commitment()?;
        let spent_24h = self.spent_in_window(now_secs)?;
        let projected = spent_24h.checked_add(requested)?;
        if projected > policy.daily_cap_sats {
            return Err(PolicyRefusal::DailyCapExceeded {
                spent_24h,
                requested,
                cap: policy.daily_cap_sats,
            });
        }

        if self
            .reservations
            .iter()
            .any(|r| r.intent_id == intent.intent_id)
        {
            return Err(PolicyRefusal::DuplicateIntent);
        }

        let remaining = policy.daily_cap_sats.checked_sub(projected)?;
        Ok(remaining)
    }

    /// Atomically reserve an approved intent in the rolling 24h ledger.
    pub fn reserve(
        &mut self,
        policy: &SpendCapPolicy,
        intent: &SpendIntent,
        now_secs: u64,
    ) -> Result<Satoshis, PolicyRefusal> {
        self.prune(now_secs);
        let remaining = self.check_policy(policy, intent, now_secs)?;
        let total_sats = intent.total_commitment()?;
        self.reservations.push(SpendReservation {
            intent_id: intent.intent_id.clone(),
            total_sats,
            timestamp_secs: now_secs,
        });
        Ok(remaining)
    }

    /// Release a reservation (e.g. if a payment failed or was reclaimed unspent).
    pub fn release(&mut self, intent_id: &str) -> bool {
        let before = self.reservations.len();
        self.reservations.retain(|r| r.intent_id != intent_id);
        self.reservations.len() != before
    }
}

/// Short-lived, single-use HMAC-SHA256 approval receipt minted by the editor
/// after native GPUI confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    pub approval_id: String,
    pub intent_id: String,
    pub amount_sats: u64,
    pub max_fee_sats: u64,
    pub destination_hash: String,
    pub network: WalletNetwork,
    pub rail: PaymentRail,
    pub expires_at: u64,
    pub mac_hex: String,
}

/// Generate a fresh 32-byte process-ephemeral HMAC key for approval receipts.
pub fn generate_session_hmac_key() -> Result<Zeroizing<[u8; 32]>, crate::SecretValidationError> {
    let mut key = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng
        .try_fill_bytes(key.as_mut())
        .map_err(|_| crate::SecretValidationError::RngUnavailable)?;
    Ok(key)
}

fn receipt_mac_input(approval_id: &str, intent: &SpendIntent, expires_at: u64) -> Vec<u8> {
    format!(
        "hkask-wallet-approval-v1|{}|{}|{}|{}|{}|{}|{}|{}",
        approval_id,
        intent.intent_id,
        intent.amount.as_u64(),
        intent.max_fee.as_u64(),
        intent.destination_hash.to_ascii_lowercase(),
        intent.network.as_str(),
        intent.rail.as_str(),
        expires_at
    )
    .into_bytes()
}

/// Mint a signed approval receipt bound to `intent` and `expires_at`.
pub fn mint_approval_receipt(
    hmac_key: &[u8],
    approval_id: String,
    intent: &SpendIntent,
    expires_at: u64,
) -> Result<ApprovalReceipt, PolicyRefusal> {
    intent.validate()?;
    let mut mac =
        HmacSha256::new_from_slice(hmac_key).map_err(|_| PolicyRefusal::ReceiptInvalid)?;
    mac.update(&receipt_mac_input(&approval_id, intent, expires_at));
    let mac_hex = hex::encode(mac.finalize().into_bytes());

    Ok(ApprovalReceipt {
        approval_id,
        intent_id: intent.intent_id.clone(),
        amount_sats: intent.amount.as_u64(),
        max_fee_sats: intent.max_fee.as_u64(),
        destination_hash: intent.destination_hash.to_ascii_lowercase(),
        network: intent.network,
        rail: intent.rail,
        expires_at,
        mac_hex,
    })
}

/// Verify an approval receipt in constant time against `intent` and `now_secs`.
pub fn verify_approval_receipt(
    hmac_key: &[u8],
    receipt: &ApprovalReceipt,
    intent: &SpendIntent,
    now_secs: u64,
) -> Result<(), PolicyRefusal> {
    if now_secs > receipt.expires_at {
        return Err(PolicyRefusal::ReceiptExpired);
    }
    if receipt.intent_id != intent.intent_id
        || receipt.amount_sats != intent.amount.as_u64()
        || receipt.max_fee_sats != intent.max_fee.as_u64()
        || receipt.destination_hash != intent.destination_hash.to_ascii_lowercase()
        || receipt.network != intent.network
        || receipt.rail != intent.rail
    {
        return Err(PolicyRefusal::ReceiptInvalid);
    }

    let provided = hex::decode(&receipt.mac_hex).map_err(|_| PolicyRefusal::ReceiptInvalid)?;
    let mut mac =
        HmacSha256::new_from_slice(hmac_key).map_err(|_| PolicyRefusal::ReceiptInvalid)?;
    mac.update(&receipt_mac_input(
        &receipt.approval_id,
        intent,
        receipt.expires_at,
    ));
    mac.verify_slice(&provided)
        .map_err(|_| PolicyRefusal::ReceiptInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy(network: WalletNetwork, mainnet_enabled: bool) -> SpendCapPolicy {
        SpendCapPolicy {
            network,
            mainnet_enabled,
            per_tx_cap_sats: Satoshis::new(10_000),
            daily_cap_sats: Satoshis::new(50_000),
            max_fee_sats: Satoshis::new(100),
            allowed_mints: vec![
                "https://mint.example.com".to_string(),
                "http://127.0.0.1:3338".to_string(),
            ],
        }
    }

    fn test_intent(id: &str, network: WalletNetwork, amount: u64, fee: u64) -> SpendIntent {
        SpendIntent {
            intent_id: id.to_string(),
            idempotency_key: format!("idem-{id}"),
            rail: PaymentRail::Cashu,
            network,
            amount: Satoshis::new(amount),
            max_fee: Satoshis::new(fee),
            mint_url: Some("https://mint.example.com/".to_string()),
            destination_summary: "coffee@example.com".to_string(),
            destination_hash: "ab".repeat(32),
            bolt11: None,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn mainnet_disabled_fails_closed() {
        let ledger = RollingSpendLedger::new();
        let policy = test_policy(WalletNetwork::Mainnet, false);
        let intent = test_intent("1", WalletNetwork::Mainnet, 1_000, 10);
        assert_eq!(
            ledger.check_policy(&policy, &intent, 1_700_000_000),
            Err(PolicyRefusal::MainnetDisabled)
        );
    }

    #[test]
    fn wrong_network_and_unallowlisted_mint_fail_closed() {
        let ledger = RollingSpendLedger::new();
        let policy = test_policy(WalletNetwork::Signet, false);

        let wrong_net = test_intent("1", WalletNetwork::Mainnet, 1_000, 10);
        assert_eq!(
            ledger.check_policy(&policy, &wrong_net, 1_700_000_000),
            Err(PolicyRefusal::NetworkMismatch {
                expected: WalletNetwork::Signet,
                actual: WalletNetwork::Mainnet,
            })
        );

        let mut bad_mint = test_intent("2", WalletNetwork::Signet, 1_000, 10);
        bad_mint.mint_url = Some("https://evil-mint.example.com".to_string());
        assert_eq!(
            ledger.check_policy(&policy, &bad_mint, 1_700_000_000),
            Err(PolicyRefusal::MintNotAllowlisted)
        );
    }

    #[test]
    fn per_tx_cap_fee_cap_and_rolling_24h_cap_enforce_hard_limits() {
        let mut ledger = RollingSpendLedger::new();
        let policy = test_policy(WalletNetwork::Mainnet, true);
        let t0 = 1_700_000_000;

        // Exceeds per-tx cap
        let big = test_intent("big", WalletNetwork::Mainnet, 10_001, 10);
        assert!(matches!(
            ledger.check_policy(&policy, &big, t0),
            Err(PolicyRefusal::PerTxCapExceeded { .. })
        ));

        // Exceeds fee cap
        let high_fee = test_intent("fee", WalletNetwork::Mainnet, 1_000, 101);
        assert!(matches!(
            ledger.check_policy(&policy, &high_fee, t0),
            Err(PolicyRefusal::FeeCapExceeded { .. })
        ));

        // Reserve 5 x 9,900 + 100 = 50,000 sats exactly
        for i in 0..5 {
            let intent = test_intent(&format!("tx-{i}"), WalletNetwork::Mainnet, 9_900, 100);
            let remaining = ledger
                .reserve(&policy, &intent, t0 + i * 60)
                .expect("within daily cap");
            assert_eq!(remaining, Satoshis::new(50_000 - (i + 1) * 10_000));
        }

        // Next 1 sat fails closed
        let over = test_intent("over", WalletNetwork::Mainnet, 1, 0);
        assert!(matches!(
            ledger.check_policy(&policy, &over, t0 + 1_000),
            Err(PolicyRefusal::DailyCapExceeded { .. })
        ));

        // After 24h passes, window clears and spend is admitted again
        assert_eq!(
            ledger.check_policy(&policy, &over, t0 + ROLLING_WINDOW_SECS + 500),
            Ok(Satoshis::new(49_999))
        );
    }

    #[test]
    fn approval_receipt_verifies_and_rejects_tampering_or_expiry() {
        let key = generate_session_hmac_key().expect("key");
        let intent = test_intent("1", WalletNetwork::Regtest, 500, 5);
        let receipt = mint_approval_receipt(&key[..], "appr-1".into(), &intent, 1_700_000_120)
            .expect("minted");

        assert!(verify_approval_receipt(&key[..], &receipt, &intent, 1_700_000_100).is_ok());
        assert_eq!(
            verify_approval_receipt(&key[..], &receipt, &intent, 1_700_000_121),
            Err(PolicyRefusal::ReceiptExpired)
        );

        let mut tampered_intent = intent;
        tampered_intent.amount = Satoshis::new(501);
        assert_eq!(
            verify_approval_receipt(&key[..], &receipt, &tampered_intent, 1_700_000_100),
            Err(PolicyRefusal::ReceiptInvalid)
        );
    }

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn rolling_spend_ledger_never_exceeds_daily_cap_across_arbitrary_sequences(
                amounts in prop::collection::vec(1u64..=12_000, 1..25),
                steps in prop::collection::vec(0u64..=10_000, 1..25),
            ) {
                let mut ledger = RollingSpendLedger::new();
                let policy = test_policy(WalletNetwork::Mainnet, true);
                let mut now = 1_700_000_000u64;
                for (i, amt) in amounts.into_iter().enumerate() {
                    now = now.saturating_add(steps[i % steps.len()]);
                    let intent = test_intent(&format!("p-{i}"), WalletNetwork::Mainnet, amt, 0);
                    let _ = ledger.reserve(&policy, &intent, now);
                    let spent = ledger.spent_in_window(now).expect("sum never overflows");
                    prop_assert!(
                        spent <= policy.daily_cap_sats,
                        "spent {spent} exceeded daily cap {}",
                        policy.daily_cap_sats
                    );
                }
            }
        }
    }
}
