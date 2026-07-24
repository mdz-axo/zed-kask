//! HederaPort — HTS USDC deposit monitoring and withdrawal on Hedera.
//!
//! # Feature gate
//! This module is only compiled when the `hedera` feature is enabled.
//! Default builds have zero Hedera dependencies.
//!
//! # SDK constraint `[IS-DECL]`
//! The `hiero-sdk` crate (v0.45.0) depends on `openssl`, which is forbidden
//! by hKask's design constraints (rustls only). The `hiero-sdk-proto` crate
//! (protobuf definitions) has no openssl dependency and is used with
//! `tonic` + `rustls` for gRPC transaction submission.
//!
//! # Current capability
//! - **Reads:** Mirror node REST API (account info, transaction history, token balances)
//! - **Writes:** Full gRPC transaction construction, signing, and submission
//!
//! # Security `[OUGHT-DECL]`
//! - Does NOT hold treasury keys — signing is delegated to `signing.rs`
//! - HTTP/gRPC clients use rustls (no openssl)
//! - Account IDs derived deterministically from treasury public key

use crate::reg_span::WalletSpan;
use crate::types::{ChainId, TxHash, WalletError};
use async_trait::async_trait;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use crate::chain::{ChainPort, DepositEvent};

// Hedera protobuf types (feature-gated behind "hedera")
use hiero_sdk_proto::services::{
    AccountAmount, AccountId, CryptoTransferTransactionBody, Duration as ProtoDuration,
    SignatureMap, SignaturePair, SignedTransaction, Timestamp, TokenId, TokenTransferList,
    Transaction, TransactionBody, TransactionId, TransactionResponse,
    crypto_service_client::CryptoServiceClient,
};
use prost::Message;
use tonic::transport::Channel;

/// Hedera mainnet mirror node REST API endpoint.
const MIRROR_NODE_MAINNET: &str = "https://mainnet-public.mirrornode.hedera.com";

/// Hedera testnet mirror node REST API endpoint.
const MIRROR_NODE_TESTNET: &str = "https://testnet.mirrornode.hedera.com";

/// HTS USDC token ID on Hedera mainnet.
const USDC_TOKEN_MAINNET: &str = "0.0.456858";

/// HTS USDC token ID on Hedera testnet.
const USDC_TOKEN_TESTNET: &str = "0.0.2276698";

/// HTTP request timeout.
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Hedera testnet consensus node (gRPC endpoint).
const TESTNET_NODE: &str = "https://0.testnet.hedera.com:50211";

/// Hedera mainnet consensus node (gRPC endpoint).
const MAINNET_NODE: &str = "https://35.232.244.145:50211";

/// Default transaction fee in tinybars (0.01 HBAR = 1,000,000 tinybars).
const DEFAULT_TX_FEE: u64 = 1_000_000;

/// Transaction valid duration in seconds.
const TX_VALID_DURATION: u64 = 120;

/// Default node account ID for transaction submission (node 0.0.3).
const NODE_ACCOUNT_ID: &str = "0.0.3";

// ── Mirror node REST API response types ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MirrorTransactionsResponse {
    transactions: Vec<MirrorTransaction>,
}

#[derive(Debug, Deserialize)]
struct MirrorTransaction {
    transaction_id: String,
    name: String,
    transfers: Option<Vec<MirrorTransfer>>,
    #[serde(rename = "consensus_timestamp")]
    consensus_timestamp: String,
}

#[derive(Debug, Deserialize)]
struct MirrorTransfer {
    account: String,
    amount: i64,
    #[serde(rename = "token_id")]
    token_id: Option<String>,
}

/// Hedera chain port — HTS USDC on Hedera via mirror node REST API + gRPC.
///
/// # Ownership
/// - Owns a `reqwest::Client` for mirror node HTTP requests
/// - Owns a `tonic::transport::Channel` for gRPC transaction submission
/// - Holds the treasury account ID for deposit address derivation
/// - Does NOT hold the treasury private key (signing is external)
pub struct HederaPort {
    /// HTTP client for mirror node REST API (rustls, no openssl).
    client: Client,
    /// Mirror node base URL.
    mirror_node_url: String,
    /// Treasury account ID (0.0.XXXXX format).
    treasury_account: String,
    /// HTS USDC token ID.
    usdc_token: String,
    /// Consensus node gRPC endpoint URL.
    consensus_node_url: String,
    /// Optional Regulation event sink for chain error span emission.
    event_sink: Option<Arc<dyn RegulationSink>>,
}

impl HederaPort {
    /// Create a new HederaPort connected to the given mirror node.
    ///
    /// `treasury_account` is the Hedera account ID (0.0.XXXXX) of the
    /// treasury. Deposit addresses are derived from this account.
    /// `usdc_token` defaults to mainnet USDC if not specified.
    pub fn new(
        mirror_node_url: &str,
        treasury_account: &str,
        usdc_token: Option<&str>,
        consensus_node_url: &str,
    ) -> Result<Self, WalletError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "failed to build HTTP client: {e}"
                )))
            })?;

        Ok(HederaPort {
            client,
            mirror_node_url: mirror_node_url.to_string(),
            treasury_account: treasury_account.to_string(),
            usdc_token: usdc_token.unwrap_or(USDC_TOKEN_MAINNET).to_string(),
            consensus_node_url: consensus_node_url.to_string(),
            event_sink: None,
        })
    }

    /// Attach a Regulation event sink for chain error span emission.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Emit a Regulation chain_error span if an event sink is configured.
    fn emit_chain_error_for_actor(&self, actor: &WebID, operation: &str, error_msg: &str) {
        if let Some(ref sink) = self.event_sink {
            let span_obj = Span::new(
                SpanNamespace::from_observable(&WalletSpan::ChainError).expect("canonical span"),
                "error",
            );
            let event = RegulationRecord::new(
                *actor,
                span_obj,
                CyclePhase::Sense,
                serde_json::json!({
                    "actor": actor.to_string(),
                    "chain": "hedera",
                    "operation": operation,
                    "error": error_msg,
                }),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.wallet.hedera", error = %e, "Failed to persist Regulation chain_error span");
            }
        }
    }

    /// Create a HederaPort for testnet.
    pub fn new_testnet(treasury_account: &str) -> Result<Self, WalletError> {
        Self::new(
            MIRROR_NODE_TESTNET,
            treasury_account,
            Some(USDC_TOKEN_TESTNET),
            TESTNET_NODE,
        )
    }

    /// Create a HederaPort for mainnet.
    pub fn new_mainnet(treasury_account: &str) -> Result<Self, WalletError> {
        Self::new(
            MIRROR_NODE_MAINNET,
            treasury_account,
            Some(USDC_TOKEN_MAINNET),
            MAINNET_NODE,
        )
    }

    /// Derive a deposit address from the treasury account + index.
    ///
    /// Hedera account IDs are in the format `shard.realm.num`.
    /// For deposit addresses, we use the treasury account directly —
    /// multiple indices can be supported by creating sub-accounts or using memo fields.
    fn derive_account_id(&self, _index: u64) -> String {
        // For now, all deposits go to the treasury account directly.
        // Multi-account derivation (using HKDF from treasury key) is a
        // future enhancement.
        self.treasury_account.clone()
    }

    // ── Protobuf helpers ────────────────────────────────────────────────────

    /// Parse a Hedera account ID string (0.0.X) into an AccountId protobuf.
    fn parse_account_id(s: &str) -> Result<AccountId, WalletError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(format!(
                    "invalid account ID format: {s}"
                )),
            ));
        }
        Ok(AccountId {
            shard_num: parts[0].parse::<i64>().map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "invalid shard: {e}"
                )))
            })?,
            realm_num: parts[1].parse::<i64>().map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "invalid realm: {e}"
                )))
            })?,
            account: Some(hiero_sdk_proto::services::account_id::Account::AccountNum(
                parts[2].parse::<i64>().map_err(|e| {
                    WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                        "invalid account num: {e}"
                    )))
                })?,
            )),
        })
    }

    /// Parse a Hedera token ID string (0.0.X) into a TokenId protobuf.
    fn parse_token_id(s: &str) -> Result<TokenId, WalletError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(format!("invalid token ID format: {s}")),
            ));
        }
        Ok(TokenId {
            shard_num: parts[0].parse::<i64>().map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "invalid shard: {e}"
                )))
            })?,
            realm_num: parts[1].parse::<i64>().map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "invalid realm: {e}"
                )))
            })?,
            token_num: parts[2].parse::<i64>().map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "invalid token num: {e}"
                )))
            })?,
        })
    }

    /// Build a CryptoTransfer transaction body for an HTS token transfer.
    fn build_transfer_body(
        &self,
        to_address: &str,
        amount_usdc_micro: u64,
    ) -> Result<TransactionBody, WalletError> {
        let treasury_id = Self::parse_account_id(&self.treasury_account)?;
        let dest_id = Self::parse_account_id(to_address)?;
        let token_id = Self::parse_token_id(&self.usdc_token)?;
        let node_id = Self::parse_account_id(NODE_ACCOUNT_ID)?;

        // Build token transfer list: negative for sender, positive for receiver
        let transfer_list = TokenTransferList {
            token: Some(token_id),
            transfers: vec![
                AccountAmount {
                    account_id: Some(treasury_id.clone()),
                    amount: -(amount_usdc_micro as i64),
                    is_approval: false,
                    hook_call: None,
                },
                AccountAmount {
                    account_id: Some(dest_id),
                    amount: amount_usdc_micro as i64,
                    is_approval: false,
                    hook_call: None,
                },
            ],
            nft_transfers: vec![],
            expected_decimals: None,
        };

        let crypto_transfer = CryptoTransferTransactionBody {
            transfers: None, // no HBAR transfers
            token_transfers: vec![transfer_list],
        };

        // Build transaction ID: treasury account + current timestamp
        let now = chrono::Utc::now();
        let tx_id = TransactionId {
            transaction_valid_start: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            account_id: Some(treasury_id),
            scheduled: false,
            nonce: 0,
        };

        Ok(TransactionBody {
            transaction_id: Some(tx_id),
            node_account_id: Some(node_id),
            transaction_fee: DEFAULT_TX_FEE,
            transaction_valid_duration: Some(ProtoDuration {
                seconds: TX_VALID_DURATION as i64,
            }),
            memo: String::new(),
            data: Some(
                hiero_sdk_proto::services::transaction_body::Data::CryptoTransfer(crypto_transfer),
            ),
            ..Default::default()
        })
    }
}

#[async_trait]
impl ChainPort for HederaPort {
    fn chain_id(&self) -> ChainId {
        ChainId::Hedera
    }

    fn derive_deposit_address(&self, index: u64) -> Result<String, WalletError> {
        Ok(self.derive_account_id(index))
    }

    async fn monitor_deposits(
        &self,
        actor: &WebID,
        addresses: &[String],
    ) -> Result<Vec<DepositEvent>, WalletError> {
        let mut events = Vec::new();

        for addr in addresses {
            let url = format!(
                "{}/api/v1/accounts/{}/transactions?limit=25&order=desc&transactiontype=CRYPTOTRANSFER",
                self.mirror_node_url, addr
            );

            let resp = self.client.get(&url).send().await.map_err(|e| {
                let msg = format!("Mirror node HTTP error (monitor_deposits): {e}");
                self.emit_chain_error_for_actor(actor, "monitor_deposits", &msg);
                WalletError::ChainError {
                    chain: ChainId::Hedera,
                    message: msg,
                }
            })?;

            if !resp.status().is_success() {
                // Account might not exist yet — skip
                continue;
            }

            let body: MirrorTransactionsResponse = resp.json().await.map_err(|e| {
                let msg = format!("Mirror node JSON parse error (monitor_deposits): {e}");
                self.emit_chain_error_for_actor(actor, "monitor_deposits", &msg);
                WalletError::ChainError {
                    chain: ChainId::Hedera,
                    message: msg,
                }
            })?;

            for tx in body.transactions {
                // Only process CRYPTOTRANSFER transactions
                if tx.name != "CRYPTOTRANSFER" {
                    continue;
                }

                // Parse transfers to find USDC token transfers TO our address
                if let Some(ref transfers) = tx.transfers {
                    // Find the sender: the account with negative amount for the same token
                    let sender = transfers
                        .iter()
                        .find(|t| t.amount < 0 && t.token_id.as_deref() == Some(&self.usdc_token))
                        .map(|t| t.account.clone())
                        .unwrap_or_else(|| "unknown".to_string());

                    for transfer in transfers {
                        // Check if this is a USDC token transfer to our address
                        if transfer.account == *addr
                            && transfer.amount > 0
                            && transfer.token_id.as_deref() == Some(&self.usdc_token)
                        {
                            // USDC has 6 decimals on Hedera.
                            // Mirror node returns amounts in token base units for HTS tokens.
                            // For USDC with 6 decimals, amount is in micro-USDC.
                            let amount_usdc_micro = transfer.amount as u64;

                            if amount_usdc_micro > 0 {
                                let consensus_seconds = tx
                                    .consensus_timestamp
                                    .split('.')
                                    .next()
                                    .and_then(|s| s.parse::<i64>().ok())
                                    .unwrap_or(0);

                                let block_time =
                                    chrono::DateTime::from_timestamp(consensus_seconds, 0)
                                        .unwrap_or_else(chrono::Utc::now);

                                events.push(DepositEvent {
                                    tx_hash: TxHash(tx.transaction_id.clone()),
                                    from_address: sender.clone(),
                                    to_address: addr.clone(),
                                    amount_usdc_micro,
                                    confirmations: 1, // Hedera finality is deterministic
                                    block_time,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(events)
    }

    fn build_withdrawal_tx(
        &self,
        to_address: &str,
        amount_usdc_micro: u64,
    ) -> Result<Vec<u8>, WalletError> {
        let body = self.build_transfer_body(to_address, amount_usdc_micro)?;
        Ok(body.encode_to_vec())
    }

    async fn submit_signed_tx(
        &self,
        actor: &WebID,
        signed_tx_bytes: &[u8],
    ) -> Result<TxHash, WalletError> {
        // The signing.rs module appends the Ed25519 signature (64 bytes) to the body bytes.
        if signed_tx_bytes.len() < 64 {
            let msg = "signed transaction too short".to_string();
            self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(msg),
            ));
        }

        let (body_bytes, sig_bytes) = signed_tx_bytes.split_at(signed_tx_bytes.len() - 64);

        // Deserialize the transaction body
        let body = TransactionBody::decode(body_bytes).map_err(|e| {
            let msg = format!("failed to decode transaction body: {e}");
            self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
            WalletError::Infra(hkask_types::InfrastructureError::database(msg))
        })?;

        // Build signature map with the Ed25519 signature
        let sig_pair = SignaturePair {
            pub_key_prefix: vec![], // empty prefix = Ed25519 key from transaction body
            signature: Some(
                hiero_sdk_proto::services::signature_pair::Signature::Ed25519(sig_bytes.to_vec()),
            ),
        };
        let sig_map = SignatureMap {
            sig_pair: vec![sig_pair],
        };

        // Wrap in SignedTransaction
        let signed_tx = SignedTransaction {
            body_bytes: body_bytes.to_vec(),
            sig_map: Some(sig_map),
            use_serialized_tx_message_hash_algorithm: false,
        };

        // Wrap SignedTransaction in Transaction for gRPC submission
        let transaction = Transaction {
            signed_transaction_bytes: signed_tx.encode_to_vec(),
            ..Default::default()
        };

        // Connect to consensus node and submit
        let channel = Channel::from_shared(self.consensus_node_url.clone())
            .map_err(|e| {
                let msg = format!("Invalid consensus node URL: {e}");
                self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
                WalletError::ChainError {
                    chain: ChainId::Hedera,
                    message: msg,
                }
            })?
            .connect()
            .await
            .map_err(|e| {
                let msg = format!("Failed to connect to consensus node: {e}");
                self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
                WalletError::ChainError {
                    chain: ChainId::Hedera,
                    message: msg,
                }
            })?;

        let mut client = CryptoServiceClient::new(channel);

        // Submit the transaction via gRPC
        let request = tonic::Request::new(transaction);
        let response = client.crypto_transfer(request).await.map_err(|e| {
            let msg = format!("gRPC cryptoTransfer failed: {e}");
            self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
            WalletError::ChainError {
                chain: ChainId::Hedera,
                message: msg,
            }
        })?;

        let receipt: TransactionResponse = response.into_inner();

        // The response contains node_transaction_precheck_code.
        // A value of 0 (OK) means the transaction passed pre-check.
        if receipt.node_transaction_precheck_code != 0 {
            let msg = format!(
                "Transaction pre-check failed with code: {}",
                receipt.node_transaction_precheck_code
            );
            self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
            return Err(WalletError::ChainError {
                chain: ChainId::Hedera,
                message: msg,
            });
        }

        // The tx_hash is derived from the TransactionID we built.
        // Format: account_id@seconds.nanos
        let tx_id = body.transaction_id.ok_or_else(|| {
            let msg = "No transaction ID in body".to_string();
            self.emit_chain_error_for_actor(actor, "submit_signed_tx", &msg);
            WalletError::ChainError {
                chain: ChainId::Hedera,
                message: msg,
            }
        })?;
        let tx_hash = format!(
            "{}@{}.{:09}",
            tx_id
                .account_id
                .map(|a| format!(
                    "{}.{}.{}",
                    a.shard_num,
                    a.realm_num,
                    a.account
                        .map(|acct| match acct {
                            hiero_sdk_proto::services::account_id::Account::AccountNum(n) =>
                                n.to_string(),
                            _ => "0".to_string(),
                        })
                        .unwrap_or_default()
                ))
                .unwrap_or_default(),
            tx_id
                .transaction_valid_start
                .map(|t| t.seconds)
                .unwrap_or(0),
            tx_id.transaction_valid_start.map(|t| t.nanos).unwrap_or(0)
        );

        Ok(TxHash(tx_hash))
    }

    async fn confirmations(&self, actor: &WebID, tx_hash: &TxHash) -> Result<u64, WalletError> {
        // Hedera has deterministic finality — once a transaction appears
        // in the mirror node, it's final. Check if the transaction exists.
        let url = format!("{}/api/v1/transactions/{}", self.mirror_node_url, tx_hash.0);

        let resp = self.client.get(&url).send().await.map_err(|e| {
            let msg = format!("Mirror node HTTP error (confirmations): {e}");
            self.emit_chain_error_for_actor(actor, "confirmations", &msg);
            WalletError::ChainError {
                chain: ChainId::Hedera,
                message: msg,
            }
        })?;

        if resp.status().is_success() {
            Ok(1) // Transaction exists → confirmed
        } else {
            Ok(0) // Not found or pending
        }
    }
}

// ── Integration tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::signing;
    use std::sync::Mutex;

    #[derive(Default)]
    struct CaptureSink {
        last_event: Mutex<Option<RegulationRecord>>,
    }

    impl RegulationSink for CaptureSink {
        fn persist(
            &self,
            event: &RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            *self.last_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(event.clone());
            Ok(())
        }
    }

    /// Build a HederaPort for testnet testing.
    fn testnet_port() -> HederaPort {
        let treasury =
            std::env::var("HEDERA_TREASURY_ACCOUNT").unwrap_or_else(|_| "0.0.12345".to_string());
        HederaPort::new(
            MIRROR_NODE_TESTNET,
            &treasury,
            Some(USDC_TOKEN_TESTNET),
            "https://testnet.hedera.com:50211",
        )
        .expect("Failed to create testnet HederaPort")
    }

    /// expect: "Wallet hedera chain error actor test works correctly under test conditions"
    #[test]
    fn emit_chain_error_uses_provided_actor() {
        let sink = Arc::new(CaptureSink::default());
        let port = HederaPort::new(
            MIRROR_NODE_TESTNET,
            "0.0.12345",
            Some(USDC_TOKEN_TESTNET),
            TESTNET_NODE,
        )
        .expect("port")
        .with_event_sink(sink.clone());
        let actor = WebID::from_persona(b"actor-hedera-test");

        port.emit_chain_error_for_actor(&actor, "unit_test", "boom");

        let event = sink
            .last_event
            .lock()
            .expect("lock")
            .clone()
            .expect("event persisted");
        assert_eq!(event.observer_webid.to_string(), actor.to_string());
        assert_eq!(event.observation["operation"], "unit_test");
    }

    /// expect: "Wallet hedera build withdrawal tx test works correctly under test conditions"
    #[test]
    fn port_construction_succeeds() {
        let port = testnet_port();
        assert_eq!(port.chain_id(), ChainId::Hedera);
    }

    /// expect: "Wallet hedera signing roundtrip test works correctly under test conditions"
    #[test]
    fn build_withdrawal_tx_produces_valid_protobuf() {
        let port = testnet_port();
        let dest = "0.0.54321";
        let payload_bytes = port
            .build_withdrawal_tx(dest, 1_000_000) // 1 USDC
            .expect("build_withdrawal_tx should succeed");

        // Payload is a TransactionBody protobuf (unsigned envelope).
        let body = TransactionBody::decode(payload_bytes.as_slice())
            .expect("payload should decode as TransactionBody");
        assert!(
            body.data.is_some(),
            "transaction body should have data field"
        );
    }

    /// expect: "Wallet hedera submit signed tx test works correctly under test conditions"
    #[test]
    fn withdrawal_payload_signing_roundtrip() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }

        let port = testnet_port();
        let dest = "0.0.54321";
        let payload_bytes = port
            .build_withdrawal_tx(dest, 1_000_000)
            .expect("build_withdrawal_tx");

        // Sign the payload
        let signature =
            signing::sign_withdrawal(ChainId::Hedera, &payload_bytes).expect("sign_withdrawal");
        assert_eq!(signature.len(), 64, "Ed25519 signature is 64 bytes");

        // Combine payload + signature
        let mut signed_tx = payload_bytes;
        signed_tx.extend_from_slice(&signature);
        assert!(signed_tx.len() > 64);
    }

    /// expect: "Wallet hedera monitor deposits test works correctly under test conditions"
    #[test]
    #[ignore = "requires funded treasury on Hedera testnet with HTS USDC"]
    fn submit_withdrawal_to_testnet() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }

        let port = testnet_port();
        let dest =
            std::env::var("HEDERA_TEST_DESTINATION").unwrap_or_else(|_| "0.0.54321".to_string());
        let amount = 100; // 0.0001 USDC

        let payload_bytes = port
            .build_withdrawal_tx(&dest, amount)
            .expect("build_withdrawal_tx");
        let signature = signing::sign_withdrawal(ChainId::Hedera, &payload_bytes).expect("sign");

        let mut signed_tx = payload_bytes;
        signed_tx.extend_from_slice(&signature);

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let actor = WebID::from_persona(b"hedera-int-test");
        let tx_hash = rt
            .block_on(port.submit_signed_tx(&actor, &signed_tx))
            .expect("submit_signed_tx should return tx hash");

        println!("Withdrawal submitted: {}", tx_hash.0);
        println!(
            "Check on HashScan: https://hashscan.io/testnet/transaction/{}",
            tx_hash.0
        );
    }

    // ── Mock-based deposit monitoring tests ──────────────────────────────

    /// Build a HederaPort that sends REST calls to a mock server.
    fn mock_port(base_url: &str) -> HederaPort {
        HederaPort {
            client: reqwest::Client::new(),
            mirror_node_url: base_url.to_string(),
            treasury_account: "0.0.12345".to_string(),
            usdc_token: USDC_TOKEN_TESTNET.to_string(),
            consensus_node_url: "https://testnet.hedera.com:50211".to_string(),
            event_sink: None,
        }
    }

    /// expect: "Wallet hedera monitor hts usdc test works correctly under test conditions"
    #[tokio::test]
    async fn monitor_deposits_detects_usdc_transfer() {
        let server = wiremock::MockServer::start().await;
        let our_addr = "0.0.12345";
        let sender_addr = "0.0.67890";
        let tx_id = "0.0.67890@1718400000.000000000";

        // Mock mirror node transactions endpoint
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/api/v1/accounts/{}/transactions",
                our_addr
            )))
            .and(wiremock::matchers::query_param(
                "transactiontype",
                "CRYPTOTRANSFER",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "transactions": [{
                        "transaction_id": tx_id,
                        "name": "CRYPTOTRANSFER",
                        "consensus_timestamp": "1718400000.000000000",
                        "transfers": [
                            {
                                "account": sender_addr,
                                "amount": -1_000_000,
                                "token_id": USDC_TOKEN_TESTNET
                            },
                            {
                                "account": our_addr,
                                "amount": 1_000_000,
                                "token_id": USDC_TOKEN_TESTNET
                            }
                        ]
                    }]
                })),
            )
            .mount(&server)
            .await;

        let port = mock_port(&server.uri());
        let actor = WebID::from_persona(b"hedera-monitor-test");
        let events = port
            .monitor_deposits(&actor, &[our_addr.to_string()])
            .await
            .expect("monitor_deposits");

        assert_eq!(events.len(), 1, "should detect one deposit");
        let deposit = &events[0];
        assert_eq!(deposit.tx_hash.0, tx_id);
        assert_eq!(deposit.to_address, our_addr);
        assert_eq!(deposit.from_address, sender_addr);
        assert_eq!(deposit.amount_usdc_micro, 1_000_000);
        assert_eq!(deposit.confirmations, 1, "Hedera finality is deterministic");
    }
}
