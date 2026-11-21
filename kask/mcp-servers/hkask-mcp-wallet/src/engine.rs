//! Cashu Wallet Engine backed by CDK (`cdk@=0.18.3`) + SQLCipher +
//! append-only `wallet_ledger` (D110 Phase 2).
//!
//! Security invariants enforced here:
//! - Every spend (`pay_invoice`, `send_ecash`) and recovery (`restore_from_seed`)
//!   requires explicit editor approval via [`ApprovalPort`] before moving funds.
//! - Every mint URL is checked against `config.allowed_mints` and the D110 SSRF
//!   perimeter (`validate_public_https_url`) before constructing a CDK wallet.
//! - Every spend enforces `idempotency_key` + unique active `payment_hash` in
//!   `wallet_ledger` (Threat T8).
//! - BOLT11 invoices are decoded and strictly validated (network, non-zero exact
//!   amount, expiry) via `hkask_wallet_core::bolt11` before quoting or approval.
//! - Zero secrets or raw spendable tokens appear in error messages or logs.

use std::collections::HashMap;
use std::str::FromStr as _;
use std::sync::Arc;

use cdk::Amount;
use cdk::amount::SplitTarget;
use cdk::nuts::{
    CurrencyUnit, PaymentMethod, Proof, PublicKey, SecretKey, SpendingConditions, Token,
};
use cdk::wallet::{
    HttpClient, MintConnector, ReceiveOptions, SendMemo, SendOptions, Wallet, WalletBuilder,
};
use cdk_common::database::WalletDatabase;
use cdk_common::nut00::KnownMethod;
use hkask_types::wallet_wire::{
    WalletNwcOperationWire, WalletNwcResultWire, WalletSpendRequestWire,
};
use hkask_wallet_core::{
    Satoshis, decode_bolt11, sha256_hex, validate_bolt11_for_payment,
    verify_preimage_matches_payment_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use crate::approval::{ApprovalError, ApprovalPort};
use crate::config::WalletConfig;
use crate::storage::{LedgerError, LedgerRow, SqlcipherConfig, WalletLedger, WalletSqlcipherDb};

/// Factory for `MintConnector`s so production uses SSRF-validated HTTP while
/// tests inject an in-process `cdk::mint::Mint` backed by `cdk-fake-wallet`.
#[async_trait::async_trait]
pub trait ConnectorFactory: Send + Sync {
    /// Pre-connect DNS-rebinding check for live HTTP mints (default no-op for
    /// offline in-process test connectors).
    async fn verify_dns(&self, _mint_url: &str) -> Result<(), WalletError> {
        Ok(())
    }

    fn build(&self, mint_url: &str) -> Result<Arc<dyn MintConnector + Send + Sync>, WalletError>;
}

/// Production HTTP connector factory enforcing the D110 SSRF perimeter and
/// pre-connect DNS-rebinding IP validation (Threat T5).
pub struct HttpConnectorFactory {
    allow_loopback: bool,
}

impl HttpConnectorFactory {
    pub fn new(allow_loopback: bool) -> Self {
        Self { allow_loopback }
    }
}

#[async_trait::async_trait]
impl ConnectorFactory for HttpConnectorFactory {
    async fn verify_dns(&self, mint_url: &str) -> Result<(), WalletError> {
        let validated = hkask_wallet_core::validate_public_https_url(mint_url, self.allow_loopback)
            .map_err(|e| WalletError::PolicyDenied(format!("mint URL rejected: {e}")))?;
        let addrs = tokio::net::lookup_host((validated.host(), validated.port()))
            .await
            .map_err(|e| WalletError::Cashu(format!("mint DNS resolution failed: {e}")))?;
        let mut count = 0usize;
        for addr in addrs {
            count += 1;
            hkask_wallet_core::validate_resolved_ip(addr.ip(), self.allow_loopback).map_err(
                |e| WalletError::PolicyDenied(format!("mint DNS resolved to blocked IP: {e}")),
            )?;
        }
        if count == 0 {
            return Err(WalletError::Cashu(
                "mint DNS resolution returned no addresses".to_string(),
            ));
        }
        Ok(())
    }

    fn build(&self, mint_url: &str) -> Result<Arc<dyn MintConnector + Send + Sync>, WalletError> {
        let validated = hkask_wallet_core::validate_public_https_url(mint_url, self.allow_loopback)
            .map_err(|e| WalletError::PolicyDenied(format!("mint URL rejected: {e}")))?;
        let parsed = validated
            .as_str()
            .parse()
            .map_err(|_| WalletError::InvalidArgument("invalid mint URL".to_string()))?;
        Ok(Arc::new(HttpClient::new(parsed, None)))
    }
}

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("wallet database is locked or uninitialized: {0}")]
    Locked(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("approval refused: {0}")]
    Approval(#[from] ApprovalError),
    #[error("duplicate spend request: {0}")]
    Duplicate(String),
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
    #[error("cashu protocol error: {0}")]
    Cashu(String),
    #[error("lightning NWC rail is not wired yet (scheduled for Phase 3)")]
    NwcNotYetImplemented,
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintBalanceSummary {
    pub mint_url: String,
    pub confirmed_sats: u64,
    pub pending_sats: u64,
    pub reserved_sats: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceReport {
    pub network: String,
    pub total_confirmed_sats: u64,
    pub total_pending_sats: u64,
    pub total_reserved_sats: u64,
    pub mints: Vec<MintBalanceSummary>,
    pub nwc_configured: bool,
    pub nwc_balance_sats: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub network: String,
    pub mainnet_enabled: bool,
    pub db_unlocked: bool,
    pub per_tx_cap_sats: u64,
    pub daily_cap_sats: u64,
    pub max_fee_sats: u64,
    pub allowed_mints: Vec<String>,
    pub default_mint: String,
    pub nwc_label: String,
    pub pending_reconciliation_count: u64,
    /// Compressed `02`-prefixed 66-char secp256k1 P2PK pubkey derived from the
    /// wallet seed for NIP-61 inbound nutzaps (`None` while locked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p2pk_pubkey: Option<String>,
    /// 64-char x-only hex P2PK pubkey for NIP-61 `kind: 10019` `pubkey` tags
    /// (`None` while locked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p2pk_xonly_pubkey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedPaymentReport {
    pub kind: String,
    pub network: Option<String>,
    pub amount_sats: Option<u64>,
    pub amount_msat: Option<u64>,
    pub mint_url: Option<String>,
    pub payment_hash: Option<String>,
    pub description: Option<String>,
    pub description_hash: Option<String>,
    pub expires_at: Option<u64>,
    pub expired: Option<bool>,
    pub proof_count: Option<usize>,
    pub p2pk_locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInvoiceReport {
    pub rail: String,
    pub mint_url: Option<String>,
    pub quote_id: String,
    pub bolt11: String,
    pub amount_sats: u64,
    pub expiry: u64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveEcashReport {
    pub ledger_id: String,
    pub mint_url: String,
    pub amount_sats: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEcashReport {
    pub ledger_id: String,
    pub approval_id: String,
    pub mint_url: String,
    pub amount_sats: u64,
    pub fee_sats: u64,
    pub p2pk_locked: bool,
    pub token: String,
    /// Individual serialized Cashu proof JSON strings (for NIP-61 `kind: 9321`
    /// `["proof", ...]` tags).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proofs_json: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayInvoiceReport {
    pub ledger_id: String,
    pub approval_id: String,
    pub rail: String,
    pub mint_url: Option<String>,
    pub payment_hash: String,
    pub amount_sats: u64,
    pub fee_paid_sats: u64,
    pub preimage: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileMintReport {
    pub mint_url: String,
    pub recovered_sagas: usize,
    pub pending_proofs_checked_sats: u64,
    pub minted_paid_quotes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub mints: Vec<ReconcileMintReport>,
    pub nwc_reconciled_settled: usize,
    pub nwc_reconciled_failed: usize,
    pub pending_ledger_rows_remaining: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreMintReport {
    pub mint_url: String,
    pub unspent_sats: u64,
    pub spent_sats: u64,
    pub pending_sats: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReport {
    pub approval_id: String,
    pub mints: Vec<RestoreMintReport>,
    pub total_restored_unspent_sats: u64,
}

/// Core Cashu wallet runtime shared by all 10 MCP tool handlers.
pub struct WalletEngine {
    pub config: WalletConfig,
    approval: Arc<dyn ApprovalPort>,
    connector_factory: Arc<dyn ConnectorFactory>,
    localstore: Arc<dyn WalletDatabase<cdk_common::database::Error> + Send + Sync>,
    ledger: Arc<WalletLedger>,
    /// 64-byte BIP-39 derived seed for NUT-13 deterministic secrets.
    /// Zeroized on drop.
    seed: Zeroizing<[u8; 64]>,
    /// Optional P2PK receiving secret key (for NUT-11 / NIP-61 redemption).
    p2pk_signing_keys: Vec<SecretKey>,
    wallets: RwLock<HashMap<String, Arc<Wallet>>>,
    db_unlocked: bool,
}

/// Derive a deterministic secp256k1 P2PK receiving `SecretKey` from the 64-byte
/// wallet seed whose compressed public key is guaranteed to carry the NIP-61
/// `02` even-Y prefix.
pub fn derive_nip61_p2pk_key(seed: &[u8; 64]) -> SecretKey {
    for counter in 0u8..=255 {
        let mut preimage = Vec::with_capacity(27 + 64 + 1);
        preimage.extend_from_slice(b"hkask-wallet-nip61-p2pk-v1:");
        preimage.extend_from_slice(seed);
        preimage.push(counter);
        let hex_digest = sha256_hex(&preimage);
        if let Ok(sk) = SecretKey::from_hex(&hex_digest) {
            if sk.public_key().to_hex().starts_with("02") {
                return sk;
            }
        }
    }
    // Statistically unreachable (~2^-256); deterministic fallback.
    let mut preimage = Vec::with_capacity(33 + 64);
    preimage.extend_from_slice(b"hkask-wallet-nip61-p2pk-fallback:");
    preimage.extend_from_slice(seed);
    SecretKey::from_hex(&sha256_hex(&preimage)).expect("valid secp256k1 scalar")
}

impl WalletEngine {
    /// Open an encrypted SQLCipher-backed `WalletEngine` with an explicit seed
    /// and passphrase (used in production after IPC unlock and in encrypted tests).
    pub async fn open_encrypted(
        config: WalletConfig,
        passphrase: Zeroizing<String>,
        seed: Zeroizing<[u8; 64]>,
        approval: Arc<dyn ApprovalPort>,
        connector_factory: Arc<dyn ConnectorFactory>,
    ) -> Result<Self, WalletError> {
        let db = WalletSqlcipherDb::new(SqlcipherConfig::encrypted_file(
            config.db_path.clone(),
            passphrase.clone(),
        ))
        .await
        .map_err(|e| WalletError::Locked(format!("failed to open SQLCipher wallet DB: {e}")))?;

        let ledger = WalletLedger::open_encrypted(&config.db_path, &passphrase)?;
        let default_p2pk = derive_nip61_p2pk_key(&seed);

        Ok(Self {
            config,
            approval,
            connector_factory,
            localstore: Arc::new(db),
            ledger: Arc::new(ledger),
            seed,
            p2pk_signing_keys: vec![default_p2pk],
            wallets: RwLock::new(HashMap::new()),
            db_unlocked: true,
        })
    }

    /// Construct an in-memory `WalletEngine` for deterministic tests.
    pub async fn open_in_memory(
        config: WalletConfig,
        seed: Zeroizing<[u8; 64]>,
        approval: Arc<dyn ApprovalPort>,
        connector_factory: Arc<dyn ConnectorFactory>,
    ) -> Result<Self, WalletError> {
        let db = WalletSqlcipherDb::new(SqlcipherConfig::in_memory())
            .await
            .map_err(|e| WalletError::Locked(format!("failed to open in-memory DB: {e}")))?;
        let ledger = WalletLedger::open_in_memory()?;
        let default_p2pk = derive_nip61_p2pk_key(&seed);
        Ok(Self {
            config,
            approval,
            connector_factory,
            localstore: Arc::new(db),
            ledger: Arc::new(ledger),
            seed,
            p2pk_signing_keys: vec![default_p2pk],
            wallets: RwLock::new(HashMap::new()),
            db_unlocked: true,
        })
    }

    /// Register a P2PK signing key so `receive_ecash` can redeem NUT-11 locked tokens.
    pub fn with_p2pk_key(mut self, key: SecretKey) -> Self {
        self.p2pk_signing_keys.push(key);
        self
    }

    /// Get or lazily construct the CDK `Wallet` for an allowlisted mint URL.
    pub async fn wallet_for_mint(
        &self,
        requested_mint: Option<&str>,
    ) -> Result<Arc<Wallet>, WalletError> {
        let mint_url_str = self
            .config
            .resolve_allowed_mint(requested_mint)
            .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;

        if let Some(existing) = self.wallets.read().await.get(&mint_url_str) {
            return Ok(Arc::clone(existing));
        }

        self.connector_factory.verify_dns(&mint_url_str).await?;
        let connector = self.connector_factory.build(&mint_url_str)?;
        let mint_url = mint_url_str
            .parse()
            .map_err(|_| WalletError::InvalidArgument("invalid mint URL".to_string()))?;

        let wallet = WalletBuilder::new()
            .mint_url(mint_url)
            .unit(CurrencyUnit::Sat)
            .localstore(Arc::clone(&self.localstore))
            .seed(*self.seed)
            .shared_client(connector)
            .build()
            .map_err(|e| WalletError::Cashu(format!("failed to build CDK wallet: {e}")))?;

        let wallet = Arc::new(wallet);
        self.wallets
            .write()
            .await
            .insert(mint_url_str, Arc::clone(&wallet));
        Ok(wallet)
    }

    /// 1. `wallet_status`
    pub fn status(&self) -> Result<StatusReport, WalletError> {
        let pending = self.ledger.count_pending()?;
        let p2pk_pubkey = self
            .p2pk_signing_keys
            .first()
            .map(|sk| sk.public_key().to_hex().to_ascii_lowercase());
        let p2pk_xonly_pubkey = p2pk_pubkey
            .as_deref()
            .and_then(|pk| pk.strip_prefix("02").or_else(|| pk.strip_prefix("03")))
            .map(str::to_string);
        Ok(StatusReport {
            network: self.config.network.as_str().to_string(),
            mainnet_enabled: self.config.mainnet_enabled,
            db_unlocked: self.db_unlocked,
            per_tx_cap_sats: self.config.per_tx_cap_sats,
            daily_cap_sats: self.config.daily_cap_sats,
            max_fee_sats: self.config.max_fee_sats,
            allowed_mints: self.config.allowed_mints.clone(),
            default_mint: self.config.default_mint.clone(),
            nwc_label: self.config.nwc_label.clone(),
            pending_reconciliation_count: pending,
            p2pk_pubkey,
            p2pk_xonly_pubkey,
        })
    }

    /// 2. `wallet_balance`
    pub async fn balance(&self) -> Result<BalanceReport, WalletError> {
        let mut mints = Vec::new();
        let mut total_confirmed = Satoshis::ZERO;
        let mut total_pending = Satoshis::ZERO;
        let mut total_reserved = Satoshis::ZERO;

        for allowed in &self.config.allowed_mints {
            let wallet = self.wallet_for_mint(Some(allowed)).await?;
            let confirmed = u64::from(
                wallet
                    .total_balance()
                    .await
                    .map_err(|e| WalletError::Cashu(e.to_string()))?,
            );
            let pending = u64::from(
                wallet
                    .total_pending_balance()
                    .await
                    .map_err(|e| WalletError::Cashu(e.to_string()))?,
            );
            let reserved = u64::from(
                wallet
                    .total_reserved_balance()
                    .await
                    .map_err(|e| WalletError::Cashu(e.to_string()))?,
            );
            total_confirmed = total_confirmed
                .checked_add(Satoshis::new(confirmed))
                .map_err(|e| WalletError::InvalidArgument(e.to_string()))?;
            total_pending = total_pending
                .checked_add(Satoshis::new(pending))
                .map_err(|e| WalletError::InvalidArgument(e.to_string()))?;
            total_reserved = total_reserved
                .checked_add(Satoshis::new(reserved))
                .map_err(|e| WalletError::InvalidArgument(e.to_string()))?;

            mints.push(MintBalanceSummary {
                mint_url: allowed.clone(),
                confirmed_sats: confirmed,
                pending_sats: pending,
                reserved_sats: reserved,
            });
        }

        let nwc_configured = !self.config.nwc_label.is_empty();
        let nwc_balance_sats = if nwc_configured {
            match self
                .approval
                .nwc_query(WalletNwcOperationWire::GetBalance)
                .await
            {
                Ok(WalletNwcResultWire::Balance { balance_sats }) => Some(balance_sats),
                _ => None,
            }
        } else {
            None
        };

        Ok(BalanceReport {
            network: self.config.network.as_str().to_string(),
            total_confirmed_sats: total_confirmed.as_u64(),
            total_pending_sats: total_pending.as_u64(),
            total_reserved_sats: total_reserved.as_u64(),
            mints,
            nwc_configured,
            nwc_balance_sats,
        })
    }

    /// 3. `wallet_transactions`
    pub fn transactions(
        &self,
        rail: Option<&str>,
        status: Option<&str>,
        since: Option<u64>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<LedgerRow>, WalletError> {
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let offset = offset.unwrap_or(0);
        Ok(self.ledger.list(rail, status, since, limit, offset)?)
    }

    /// 4. `wallet_decode_payment` (pure local — contacts nothing)
    pub fn decode_payment(&self, input: &str) -> Result<DecodedPaymentReport, WalletError> {
        decode_payment_input(input)
    }

    /// 5. `wallet_reconcile` (NUT-07 proof state checks + incomplete saga recovery + paid mint quotes)
    pub async fn reconcile(&self) -> Result<ReconcileReport, WalletError> {
        let mut mint_reports = Vec::new();
        for allowed in &self.config.allowed_mints {
            let wallet = self.wallet_for_mint(Some(allowed)).await?;

            // 1. Recover any incomplete send/melt/swap sagas persisted before a crash
            let saga_report = wallet
                .recover_incomplete_sagas()
                .await
                .map_err(|e| WalletError::Cashu(format!("saga recovery failed: {e}")))?;

            // 2. Check all pending proofs via NUT-07 and reclaim unspent
            let checked = wallet
                .check_all_pending_proofs()
                .await
                .map_err(|e| WalletError::Cashu(format!("NUT-07 pending check failed: {e}")))?;

            // 3. Mint any paid-but-unissued NUT-04 mint quotes
            let unissued = wallet
                .get_unissued_mint_quotes()
                .await
                .map_err(|e| WalletError::Cashu(format!("unissued quotes query failed: {e}")))?;
            let mut minted_count = 0usize;
            for q in unissued {
                if let Ok(updated) = wallet.check_mint_quote_status(&q.id).await {
                    if updated.state == cdk::nuts::MintQuoteState::Paid {
                        if wallet
                            .mint(&q.id, SplitTarget::default(), None)
                            .await
                            .is_ok()
                        {
                            minted_count += 1;
                        }
                    }
                }
            }

            mint_reports.push(ReconcileMintReport {
                mint_url: allowed.clone(),
                recovered_sagas: saga_report.recovered + saga_report.compensated,
                pending_proofs_checked_sats: u64::from(checked),
                minted_paid_quotes: minted_count,
            });
        }

        // Reconcile any pending/reconciling NWC payments via editor-side lookup_invoice
        let mut nwc_reconciled_settled = 0usize;
        let mut nwc_reconciled_failed = 0usize;
        for status_filter in ["pending", "reconciling"] {
            let rows = self
                .ledger
                .list(Some("nwc"), Some(status_filter), None, 200, 0)?;
            for row in rows {
                let Some(payment_hash) = row.payment_hash.as_deref() else {
                    continue;
                };
                if let Ok(WalletNwcResultWire::Lookup {
                    state,
                    preimage,
                    fee_paid_sats,
                    ..
                }) = self
                    .approval
                    .nwc_query(WalletNwcOperationWire::LookupInvoice {
                        payment_hash: payment_hash.to_string(),
                    })
                    .await
                {
                    if state == "settled" {
                        if let Some(pre) = preimage.as_deref() {
                            if verify_preimage_matches_payment_hash(pre, payment_hash).is_ok() {
                                let _ = self.ledger.update_status(
                                    &row.id,
                                    "settled",
                                    fee_paid_sats,
                                    None,
                                    now_unix_secs(),
                                );
                                nwc_reconciled_settled += 1;
                            }
                        }
                    } else if state == "failed" || state == "expired" {
                        let _ = self.ledger.update_status(
                            &row.id,
                            "failed",
                            None,
                            None,
                            now_unix_secs(),
                        );
                        nwc_reconciled_failed += 1;
                    }
                }
            }
        }

        let pending_remaining = self.ledger.count_pending()?;
        Ok(ReconcileReport {
            mints: mint_reports,
            nwc_reconciled_settled,
            nwc_reconciled_failed,
            pending_ledger_rows_remaining: pending_remaining,
        })
    }

    /// 6. `wallet_create_invoice` (Cashu NUT-04 mint quote; NWC deferred to Phase 3)
    pub async fn create_invoice(
        &self,
        rail: &str,
        amount_sats: u64,
        mint_url: Option<&str>,
        description: Option<String>,
    ) -> Result<CreateInvoiceReport, WalletError> {
        if amount_sats == 0 {
            return Err(WalletError::InvalidArgument(
                "amount_sats must be greater than zero".to_string(),
            ));
        }
        match rail.trim().to_ascii_lowercase().as_str() {
            "cashu" => {
                let resolved_mint = self
                    .config
                    .resolve_allowed_mint(mint_url)
                    .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;
                let wallet = self.wallet_for_mint(Some(&resolved_mint)).await?;
                let quote = wallet
                    .mint_quote(
                        PaymentMethod::Known(KnownMethod::Bolt11),
                        Some(Amount::from(amount_sats)),
                        description,
                        None,
                    )
                    .await
                    .map_err(|e| WalletError::Cashu(format!("mint_quote failed: {e}")))?;
                Ok(CreateInvoiceReport {
                    rail: "cashu".to_string(),
                    mint_url: Some(resolved_mint),
                    quote_id: quote.id,
                    bolt11: quote.request,
                    amount_sats,
                    expiry: quote.expiry,
                    state: quote.state.to_string(),
                })
            }
            "nwc" => {
                let resp = self
                    .approval
                    .nwc_query(WalletNwcOperationWire::MakeInvoice {
                        amount_sats,
                        description,
                    })
                    .await?;
                match resp {
                    WalletNwcResultWire::Invoice {
                        bolt11,
                        payment_hash,
                        amount_sats,
                        expiry,
                    } => Ok(CreateInvoiceReport {
                        rail: "nwc".to_string(),
                        mint_url: None,
                        quote_id: payment_hash,
                        bolt11,
                        amount_sats,
                        expiry,
                        state: "UNPAID".to_string(),
                    }),
                    _ => Err(WalletError::Approval(ApprovalError::InvalidResponse(
                        "unexpected NWC response variant for make_invoice".to_string(),
                    ))),
                }
            }
            _ => Err(WalletError::InvalidArgument(
                "unsupported rail (expected cashu or nwc)".to_string(),
            )),
        }
    }

    /// Complete a paid NUT-04 mint quote (used by `wallet_reconcile` and tests).
    pub async fn mint_paid_quote(
        &self,
        mint_url: Option<&str>,
        quote_id: &str,
    ) -> Result<u64, WalletError> {
        let wallet = self.wallet_for_mint(mint_url).await?;
        let _ = wallet
            .check_mint_quote_status(quote_id)
            .await
            .map_err(|e| WalletError::Cashu(format!("check_mint_quote_status failed: {e}")))?;
        let proofs = wallet
            .mint(quote_id, SplitTarget::default(), None)
            .await
            .map_err(|e| WalletError::Cashu(format!("mint failed: {e}")))?;
        let total: u64 = proofs.iter().map(|p| u64::from(p.amount)).sum();
        Ok(total)
    }

    /// 7. `wallet_receive_ecash` (allowlist check + NUT-12 DLEQ + NUT-11 P2PK swap)
    pub async fn receive_ecash(
        &self,
        encoded_token: &str,
    ) -> Result<ReceiveEcashReport, WalletError> {
        let trimmed = encoded_token.trim();
        let token = Token::from_str(trimmed)
            .map_err(|_| WalletError::InvalidArgument("invalid Cashu token".to_string()))?;
        let raw_mint = token
            .mint_url()
            .map_err(|_| WalletError::InvalidArgument("token missing mint URL".to_string()))?
            .to_string();
        let resolved_mint = self
            .config
            .resolve_allowed_mint(Some(&raw_mint))
            .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;

        let wallet = self.wallet_for_mint(Some(&resolved_mint)).await?;
        let opts = ReceiveOptions {
            p2pk_signing_keys: self.p2pk_signing_keys.clone(),
            ..Default::default()
        };
        let amount = wallet
            .receive(trimmed, opts)
            .await
            .map_err(|e| WalletError::Cashu(format!("receive failed: {e}")))?;
        let amount_sats = u64::from(amount);

        let now = now_unix_secs();
        let ledger_id = uuid::Uuid::new_v4().to_string();
        let row = LedgerRow {
            id: ledger_id.clone(),
            idempotency_key: format!("recv-{}", sha256_hex(trimmed.as_bytes())),
            direction: "inbound".to_string(),
            rail: "cashu".to_string(),
            network: self.config.network.as_str().to_string(),
            mint_url: Some(resolved_mint.clone()),
            payment_hash: None,
            amount_sats,
            fee_sats: 0,
            status: "settled".to_string(),
            target_summary: format!("received ecash from {resolved_mint}"),
            nostr_event_id: None,
            approval_id: None,
            created_at: now,
            updated_at: now,
        };
        let _ = self.ledger.insert(&row);

        Ok(ReceiveEcashReport {
            ledger_id,
            mint_url: resolved_mint,
            amount_sats,
            status: "settled".to_string(),
        })
    }

    /// Redeem inbound NIP-61 `kind: 9321` P2PK-locked Cashu proofs directly
    /// from `(mint_url, proofs_json, nostr_event_id)`. Validates the mint
    /// against `allowed_mints`, verifies DLEQ (NUT-12), signs NUT-11 P2PK
    /// witnesses with the wallet's registered P2PK keys, swaps at the mint,
    /// and records the inbound settlement with `nostr_event_id`.
    pub async fn receive_nutzap_proofs(
        &self,
        mint_url: &str,
        proofs_json: &[String],
        nostr_event_id: Option<&str>,
    ) -> Result<ReceiveEcashReport, WalletError> {
        if proofs_json.is_empty() {
            return Err(WalletError::InvalidArgument(
                "proofs_json must contain at least one Cashu proof".to_string(),
            ));
        }
        let resolved_mint = self
            .config
            .resolve_allowed_mint(Some(mint_url.trim()))
            .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;

        let mut proofs = Vec::with_capacity(proofs_json.len());
        for raw in proofs_json {
            let proof: Proof = serde_json::from_str(raw).map_err(|e| {
                WalletError::InvalidArgument(format!("invalid NIP-61 Cashu proof JSON: {e}"))
            })?;
            proofs.push(proof);
        }

        let parsed_mint = resolved_mint
            .parse()
            .map_err(|_| WalletError::InvalidArgument("invalid mint URL".to_string()))?;
        let token = Token::new(parsed_mint, proofs, None, CurrencyUnit::Sat);
        let encoded = token.to_string();

        let wallet = self.wallet_for_mint(Some(&resolved_mint)).await?;
        let opts = ReceiveOptions {
            p2pk_signing_keys: self.p2pk_signing_keys.clone(),
            ..Default::default()
        };
        let amount = wallet
            .receive(&encoded, opts)
            .await
            .map_err(|e| WalletError::Cashu(format!("NIP-61 nutzap redemption failed: {e}")))?;
        let amount_sats = u64::from(amount);

        let now = now_unix_secs();
        let ledger_id = uuid::Uuid::new_v4().to_string();
        let idempotency_key = match nostr_event_id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(ev_id) => format!("nutzap-{ev_id}"),
            None => format!("nutzap-{}", sha256_hex(encoded.as_bytes())),
        };
        let row = LedgerRow {
            id: ledger_id.clone(),
            idempotency_key,
            direction: "inbound".to_string(),
            rail: "cashu".to_string(),
            network: self.config.network.as_str().to_string(),
            mint_url: Some(resolved_mint.clone()),
            payment_hash: None,
            amount_sats,
            fee_sats: 0,
            status: "settled".to_string(),
            target_summary: format!("redeemed NIP-61 nutzap from {resolved_mint}"),
            nostr_event_id: nostr_event_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            approval_id: None,
            created_at: now,
            updated_at: now,
        };
        let _ = self.ledger.insert(&row);

        Ok(ReceiveEcashReport {
            ledger_id,
            mint_url: resolved_mint,
            amount_sats,
            status: "settled".to_string(),
        })
    }

    /// 8. `wallet_pay_invoice` (approval-gated BOLT11 payment via Cashu NUT-05 melt or NWC)
    pub async fn pay_invoice(
        &self,
        idempotency_key: &str,
        bolt11: &str,
        rail: &str,
        mint_url: Option<&str>,
    ) -> Result<PayInvoiceReport, WalletError> {
        let idem = idempotency_key.trim();
        if idem.is_empty() {
            return Err(WalletError::InvalidArgument(
                "idempotency_key is required".to_string(),
            ));
        }

        // T8: check idempotency_key before touching mints or editor approval
        if let Some(existing) = self.ledger.get_by_idempotency_key(idem)? {
            return Err(WalletError::Duplicate(format!(
                "idempotency_key '{idem}' already exists with status '{}' (use wallet_reconcile or wallet_transactions)",
                existing.status
            )));
        }

        // T6: decode and strictly validate BOLT11 before quoting or approval
        let decoded =
            decode_bolt11(bolt11).map_err(|e| WalletError::InvalidArgument(e.to_string()))?;
        let now = now_unix_secs();
        let amount_sats =
            validate_bolt11_for_payment(&decoded, self.config.network, None, None, now)
                .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;

        if self.config.network.is_mainnet() && !self.config.mainnet_enabled {
            return Err(WalletError::PolicyDenied(
                "mainnet spending is disabled (kask.wallet.mainnet_enabled is false)".to_string(),
            ));
        }

        match rail.trim().to_ascii_lowercase().as_str() {
            "cashu" => {
                let resolved_mint = self
                    .config
                    .resolve_allowed_mint(mint_url)
                    .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;
                let wallet = self.wallet_for_mint(Some(&resolved_mint)).await?;

                // Quote melt fee from mint (NUT-05)
                let quote = wallet
                    .melt_quote(
                        PaymentMethod::Known(KnownMethod::Bolt11),
                        bolt11.trim(),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| WalletError::Cashu(format!("melt_quote failed: {e}")))?;
                let max_fee_sats = u64::from(quote.fee_reserve);

                // Draft ledger row (enforces unique active outbound payment_hash — T8)
                let intent_id = uuid::Uuid::new_v4().to_string();
                let ledger_row = LedgerRow {
                    id: intent_id.clone(),
                    idempotency_key: idem.to_string(),
                    direction: "outbound".to_string(),
                    rail: "cashu".to_string(),
                    network: self.config.network.as_str().to_string(),
                    mint_url: Some(resolved_mint.clone()),
                    payment_hash: Some(decoded.payment_hash.clone()),
                    amount_sats: amount_sats.as_u64(),
                    fee_sats: max_fee_sats,
                    status: "awaiting_approval".to_string(),
                    target_summary: decoded
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("BOLT11 {}", &decoded.payment_hash[..16])),
                    nostr_event_id: None,
                    approval_id: None,
                    created_at: now,
                    updated_at: now,
                };
                self.ledger.insert(&ledger_row).map_err(|e| match e {
                    LedgerError::Duplicate(k) => WalletError::Duplicate(format!(
                        "duplicate payment_hash or idempotency_key ({k}); inspect wallet_transactions or run wallet_reconcile"
                    )),
                    other => WalletError::Ledger(other),
                })?;

                // Mandatory native editor approval (T1)
                let approval_req = WalletSpendRequestWire {
                    intent_id: intent_id.clone(),
                    idempotency_key: idem.to_string(),
                    rail: "cashu".to_string(),
                    network: self.config.network.as_str().to_string(),
                    amount_sats: amount_sats.as_u64(),
                    max_fee_sats,
                    mint_url: Some(resolved_mint.clone()),
                    destination_summary: ledger_row.target_summary.clone(),
                    destination_hash: sha256_hex(bolt11.trim().as_bytes()),
                    bolt11: Some(bolt11.trim().to_string()),
                    created_at: now,
                };
                let approval = match self.approval.request_spend_approval(approval_req).await {
                    Ok(appr) => appr,
                    Err(err) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "declined",
                            None,
                            None,
                            now_unix_secs(),
                        );
                        return Err(WalletError::Approval(err));
                    }
                };

                self.ledger.update_status(
                    &intent_id,
                    "pending",
                    None,
                    Some(&approval.approval_id),
                    now_unix_secs(),
                )?;

                // Execute NUT-05 melt (CDK automatically includes NUT-08 change outputs)
                let prepared = match wallet.prepare_melt(&quote.id, HashMap::new()).await {
                    Ok(p) => p,
                    Err(err) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "failed",
                            None,
                            Some(&approval.approval_id),
                            now_unix_secs(),
                        );
                        return Err(WalletError::Cashu(format!("prepare_melt failed: {err}")));
                    }
                };

                match prepared.confirm().await {
                    Ok(finalized) => {
                        let fee_paid = u64::from(finalized.fee_paid());
                        let preimage = finalized.payment_proof().map(str::to_string);
                        self.ledger.update_status(
                            &intent_id,
                            "settled",
                            Some(fee_paid),
                            Some(&approval.approval_id),
                            now_unix_secs(),
                        )?;
                        Ok(PayInvoiceReport {
                            ledger_id: intent_id,
                            approval_id: approval.approval_id,
                            rail: "cashu".to_string(),
                            mint_url: Some(resolved_mint),
                            payment_hash: decoded.payment_hash,
                            amount_sats: amount_sats.as_u64(),
                            fee_paid_sats: fee_paid,
                            preimage,
                            status: "settled".to_string(),
                        })
                    }
                    Err(err) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "reconciling",
                            None,
                            Some(&approval.approval_id),
                            now_unix_secs(),
                        );
                        Err(WalletError::Cashu(format!(
                            "melt did not settle cleanly (marked reconciling — run wallet_reconcile): {err}"
                        )))
                    }
                }
            }
            "nwc" => {
                let max_fee_sats = self.config.max_fee_sats;
                let intent_id = uuid::Uuid::new_v4().to_string();
                let ledger_row = LedgerRow {
                    id: intent_id.clone(),
                    idempotency_key: idem.to_string(),
                    direction: "outbound".to_string(),
                    rail: "nwc".to_string(),
                    network: self.config.network.as_str().to_string(),
                    mint_url: None,
                    payment_hash: Some(decoded.payment_hash.clone()),
                    amount_sats: amount_sats.as_u64(),
                    fee_sats: max_fee_sats,
                    status: "awaiting_approval".to_string(),
                    target_summary: decoded
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("BOLT11 {}", &decoded.payment_hash[..16])),
                    nostr_event_id: None,
                    approval_id: None,
                    created_at: now,
                    updated_at: now,
                };
                self.ledger.insert(&ledger_row).map_err(|e| match e {
                    LedgerError::Duplicate(k) => WalletError::Duplicate(format!(
                        "duplicate payment_hash or idempotency_key ({k}); inspect wallet_transactions or run wallet_reconcile"
                    )),
                    other => WalletError::Ledger(other),
                })?;

                let approval_req = WalletSpendRequestWire {
                    intent_id: intent_id.clone(),
                    idempotency_key: idem.to_string(),
                    rail: "nwc".to_string(),
                    network: self.config.network.as_str().to_string(),
                    amount_sats: amount_sats.as_u64(),
                    max_fee_sats,
                    mint_url: None,
                    destination_summary: ledger_row.target_summary.clone(),
                    destination_hash: sha256_hex(bolt11.trim().as_bytes()),
                    bolt11: Some(bolt11.trim().to_string()),
                    created_at: now,
                };

                match self.approval.request_spend_approval(approval_req).await {
                    Ok(appr) => {
                        let Some(preimage) = appr.nwc_preimage.as_deref() else {
                            let _ = self.ledger.update_status(
                                &intent_id,
                                "reconciling",
                                None,
                                Some(&appr.approval_id),
                                now_unix_secs(),
                            );
                            return Err(WalletError::Approval(ApprovalError::InvalidResponse(
                                "editor NWC approval omitted preimage; marked reconciling"
                                    .to_string(),
                            )));
                        };
                        if verify_preimage_matches_payment_hash(preimage, &decoded.payment_hash)
                            .is_err()
                        {
                            let _ = self.ledger.update_status(
                                &intent_id,
                                "reconciling",
                                None,
                                Some(&appr.approval_id),
                                now_unix_secs(),
                            );
                            return Err(WalletError::Approval(ApprovalError::InvalidResponse(
                                "NWC preimage failed SHA-256 verification against payment_hash"
                                    .to_string(),
                            )));
                        }
                        let fee_paid = appr.nwc_fee_paid_sats.unwrap_or(0);
                        self.ledger.update_status(
                            &intent_id,
                            "settled",
                            Some(fee_paid),
                            Some(&appr.approval_id),
                            now_unix_secs(),
                        )?;
                        Ok(PayInvoiceReport {
                            ledger_id: intent_id,
                            approval_id: appr.approval_id,
                            rail: "nwc".to_string(),
                            mint_url: None,
                            payment_hash: decoded.payment_hash,
                            amount_sats: amount_sats.as_u64(),
                            fee_paid_sats: fee_paid,
                            preimage: Some(preimage.to_ascii_lowercase()),
                            status: "settled".to_string(),
                        })
                    }
                    Err(ApprovalError::Timeout(msg)) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "reconciling",
                            None,
                            None,
                            now_unix_secs(),
                        );
                        Err(WalletError::Approval(ApprovalError::Timeout(msg)))
                    }
                    Err(ApprovalError::PaymentFailed(msg)) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "failed",
                            None,
                            None,
                            now_unix_secs(),
                        );
                        Err(WalletError::Approval(ApprovalError::PaymentFailed(msg)))
                    }
                    Err(err) => {
                        let _ = self.ledger.update_status(
                            &intent_id,
                            "declined",
                            None,
                            None,
                            now_unix_secs(),
                        );
                        Err(WalletError::Approval(err))
                    }
                }
            }
            _ => Err(WalletError::InvalidArgument(
                "unsupported rail (expected cashu or nwc)".to_string(),
            )),
        }
    }

    /// 9. `wallet_send_ecash` (approval-gated Cashu send, optional NUT-11 P2PK lock)
    pub async fn send_ecash(
        &self,
        idempotency_key: &str,
        amount_sats: u64,
        mint_url: Option<&str>,
        p2pk_pubkey: Option<&str>,
        memo: Option<String>,
    ) -> Result<SendEcashReport, WalletError> {
        let idem = idempotency_key.trim();
        if idem.is_empty() {
            return Err(WalletError::InvalidArgument(
                "idempotency_key is required".to_string(),
            ));
        }
        if amount_sats == 0 {
            return Err(WalletError::InvalidArgument(
                "amount_sats must be greater than zero".to_string(),
            ));
        }
        if let Some(existing) = self.ledger.get_by_idempotency_key(idem)? {
            return Err(WalletError::Duplicate(format!(
                "idempotency_key '{idem}' already exists with status '{}'",
                existing.status
            )));
        }
        if self.config.network.is_mainnet() && !self.config.mainnet_enabled {
            return Err(WalletError::PolicyDenied(
                "mainnet spending is disabled (kask.wallet.mainnet_enabled is false)".to_string(),
            ));
        }

        let resolved_mint = self
            .config
            .resolve_allowed_mint(mint_url)
            .map_err(|e| WalletError::PolicyDenied(e.to_string()))?;
        let wallet = self.wallet_for_mint(Some(&resolved_mint)).await?;

        let conditions = match p2pk_pubkey.map(str::trim).filter(|s| !s.is_empty()) {
            Some(pk_hex) => {
                let pk = PublicKey::from_hex(pk_hex).map_err(|_| {
                    WalletError::InvalidArgument(
                        "p2pk_pubkey must be a valid compressed secp256k1 hex public key"
                            .to_string(),
                    )
                })?;
                Some(SpendingConditions::new_p2pk(pk, None))
            }
            None => None,
        };
        let p2pk_locked = conditions.is_some();

        let opts = SendOptions {
            conditions,
            ..Default::default()
        };
        let prepared = wallet
            .prepare_send(Amount::from(amount_sats), opts)
            .await
            .map_err(|e| WalletError::Cashu(format!("prepare_send failed: {e}")))?;
        let fee_sats = u64::from(prepared.fee());

        let now = now_unix_secs();
        let intent_id = uuid::Uuid::new_v4().to_string();
        let target_summary = match p2pk_pubkey.map(str::trim).filter(|s| !s.is_empty()) {
            Some(pk) => format!("Cashu P2PK send to {}…", &pk[..pk.len().min(16)]),
            None => format!("Cashu bearer ecash send ({resolved_mint})"),
        };

        let ledger_row = LedgerRow {
            id: intent_id.clone(),
            idempotency_key: idem.to_string(),
            direction: "outbound".to_string(),
            rail: "cashu".to_string(),
            network: self.config.network.as_str().to_string(),
            mint_url: Some(resolved_mint.clone()),
            payment_hash: None,
            amount_sats,
            fee_sats,
            status: "awaiting_approval".to_string(),
            target_summary: target_summary.clone(),
            nostr_event_id: None,
            approval_id: None,
            created_at: now,
            updated_at: now,
        };
        self.ledger.insert(&ledger_row)?;

        let dest_hash = sha256_hex(
            format!(
                "{resolved_mint}|{}|{amount_sats}",
                p2pk_pubkey.unwrap_or("bearer")
            )
            .as_bytes(),
        );
        let approval_req = WalletSpendRequestWire {
            intent_id: intent_id.clone(),
            idempotency_key: idem.to_string(),
            rail: "cashu".to_string(),
            network: self.config.network.as_str().to_string(),
            amount_sats,
            max_fee_sats: fee_sats,
            mint_url: Some(resolved_mint.clone()),
            destination_summary: target_summary,
            destination_hash: dest_hash,
            bolt11: None,
            created_at: now,
        };

        let approval = match self.approval.request_spend_approval(approval_req).await {
            Ok(appr) => appr,
            Err(err) => {
                // Cancel prepared send to release reserved proofs immediately!
                let _ = prepared.cancel().await;
                let _ =
                    self.ledger
                        .update_status(&intent_id, "declined", None, None, now_unix_secs());
                return Err(WalletError::Approval(err));
            }
        };

        self.ledger.update_status(
            &intent_id,
            "pending",
            None,
            Some(&approval.approval_id),
            now_unix_secs(),
        )?;

        let send_memo = memo.map(|m| SendMemo::for_token(&m));
        match prepared.confirm(send_memo).await {
            Ok(token) => {
                let proofs_json: Vec<String> = match &token {
                    Token::TokenV3(v3) => v3
                        .token
                        .iter()
                        .flat_map(|t| t.proofs.iter())
                        .filter_map(|p| serde_json::to_string(p).ok())
                        .collect(),
                    Token::TokenV4(v4) => {
                        let keysets = wallet
                            .localstore
                            .get_mint_keysets(wallet.mint_url.clone())
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        v4.proofs(&keysets)
                            .ok()
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|p| serde_json::to_string(p).ok())
                            .collect()
                    }
                };
                self.ledger.update_status(
                    &intent_id,
                    "settled",
                    Some(fee_sats),
                    Some(&approval.approval_id),
                    now_unix_secs(),
                )?;
                Ok(SendEcashReport {
                    ledger_id: intent_id,
                    approval_id: approval.approval_id,
                    mint_url: resolved_mint,
                    amount_sats,
                    fee_sats,
                    p2pk_locked,
                    token: token.to_string(),
                    proofs_json,
                })
            }
            Err(err) => {
                let _ = self.ledger.update_status(
                    &intent_id,
                    "reconciling",
                    None,
                    Some(&approval.approval_id),
                    now_unix_secs(),
                );
                Err(WalletError::Cashu(format!(
                    "send confirm failed (marked reconciling — run wallet_reconcile): {err}"
                )))
            }
        }
    }

    /// 10. `wallet_restore_from_seed` (approval-gated NUT-09 / NUT-13 deterministic restore)
    pub async fn restore_from_seed(
        &self,
        mint_url: Option<&str>,
    ) -> Result<RestoreReport, WalletError> {
        let targets: Vec<String> = match mint_url.map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) => vec![
                self.config
                    .resolve_allowed_mint(Some(m))
                    .map_err(|e| WalletError::PolicyDenied(e.to_string()))?,
            ],
            None => self.config.allowed_mints.clone(),
        };
        if targets.is_empty() {
            return Err(WalletError::InvalidArgument(
                "no allowlisted mints configured for restore".to_string(),
            ));
        }

        // Recovery requires explicit operator confirmation via the approval seam
        let now = now_unix_secs();
        let intent_id = uuid::Uuid::new_v4().to_string();
        let req = WalletSpendRequestWire {
            intent_id: intent_id.clone(),
            idempotency_key: format!("restore-{intent_id}"),
            rail: "cashu".to_string(),
            network: self.config.network.as_str().to_string(),
            amount_sats: 1,
            max_fee_sats: 0,
            mint_url: Some(targets[0].clone()),
            destination_summary: format!(
                "NUT-09/NUT-13 seed restore across {} allowlisted mint(s)",
                targets.len()
            ),
            destination_hash: sha256_hex(targets.join(",").as_bytes()),
            bolt11: None,
            created_at: now,
        };
        let approval = self.approval.request_spend_approval(req).await?;

        let mut restored_mints = Vec::new();
        let mut total_unspent = Satoshis::ZERO;
        for target in &targets {
            let wallet = self.wallet_for_mint(Some(target)).await?;
            let restored = wallet
                .restore()
                .await
                .map_err(|e| WalletError::Cashu(format!("restore failed on {target}: {e}")))?;
            let unspent_sats = u64::from(restored.unspent);
            let spent_sats = u64::from(restored.spent);
            let pending_sats = u64::from(restored.pending);
            total_unspent = total_unspent
                .checked_add(Satoshis::new(unspent_sats))
                .map_err(|e| WalletError::InvalidArgument(e.to_string()))?;
            restored_mints.push(RestoreMintReport {
                mint_url: target.clone(),
                unspent_sats,
                spent_sats,
                pending_sats,
            });
        }

        Ok(RestoreReport {
            approval_id: approval.approval_id,
            mints: restored_mints,
            total_restored_unspent_sats: total_unspent.as_u64(),
        })
    }
}

/// Pure-local decoder for BOLT11 invoices, Lightning Addresses (`user@domain`),
/// and Cashu tokens (`cashuA`/`cashuB`). Contacts nothing and works even when
/// the wallet SQLCipher database is locked.
pub fn decode_payment_input(input: &str) -> Result<DecodedPaymentReport, WalletError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(WalletError::InvalidArgument("input is empty".to_string()));
    }

    // Cashu token (`cashuA...` / `cashuB...`)
    if trimmed.starts_with("cashuA") || trimmed.starts_with("cashuB") {
        let token = Token::from_str(trimmed)
            .map_err(|_| WalletError::InvalidArgument("invalid Cashu token".to_string()))?;
        let mint_url = token.mint_url().ok().map(|u| u.to_string());
        let amount_sats = token.value().ok().map(u64::from);
        let (proof_count, p2pk_locked) = match &token {
            Token::TokenV3(v3) => {
                let proofs: Vec<_> = v3.token.iter().flat_map(|t| t.proofs.iter()).collect();
                let p2pk = proofs
                    .iter()
                    .any(|p| p.secret.to_string().contains("\"P2PK\""));
                (Some(proofs.len()), Some(p2pk))
            }
            Token::TokenV4(v4) => {
                let count: usize = v4.token.iter().map(|t| t.proofs.len()).sum();
                let p2pk = v4
                    .token
                    .iter()
                    .flat_map(|t| t.proofs.iter())
                    .any(|p| p.secret.to_string().contains("\"P2PK\""));
                (Some(count), Some(p2pk))
            }
        };
        return Ok(DecodedPaymentReport {
            kind: "cashu_token".to_string(),
            network: None,
            amount_sats,
            amount_msat: amount_sats.and_then(|s| s.checked_mul(1_000)),
            mint_url,
            payment_hash: None,
            description: token.memo().clone(),
            description_hash: None,
            expires_at: None,
            expired: None,
            proof_count,
            p2pk_locked,
        });
    }

    // Lightning Address (`user@domain`)
    if let Some((user, domain)) = trimmed.split_once('@') {
        if !user.is_empty() && domain.contains('.') && !trimmed.contains(' ') {
            return Ok(DecodedPaymentReport {
                kind: "lightning_address".to_string(),
                network: None,
                amount_sats: None,
                amount_msat: None,
                mint_url: None,
                payment_hash: None,
                description: Some(format!("{user}@{domain}")),
                description_hash: None,
                expires_at: None,
                expired: None,
                proof_count: None,
                p2pk_locked: None,
            });
        }
    }

    // BOLT11 invoice
    let decoded =
        decode_bolt11(trimmed).map_err(|e| WalletError::InvalidArgument(e.to_string()))?;
    let now = now_unix_secs();
    Ok(DecodedPaymentReport {
        kind: "bolt11".to_string(),
        network: Some(decoded.network.as_str().to_string()),
        amount_sats: decoded.amount_sats_exact.map(Satoshis::as_u64),
        amount_msat: decoded.amount_msat.map(|m| m.as_u64()),
        mint_url: None,
        payment_hash: Some(decoded.payment_hash.clone()),
        description: decoded.description.clone(),
        description_hash: decoded.description_hash.clone(),
        expires_at: Some(decoded.expires_at_secs),
        expired: Some(decoded.is_expired(now)),
        proof_count: None,
        p2pk_locked: None,
    })
}
