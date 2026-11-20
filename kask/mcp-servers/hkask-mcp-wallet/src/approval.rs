//! Editor IPC boundary for DB unlock and mandatory native spend approval (D110).
//!
//! `ApprovalPort` is the single system-boundary port between `hkask-mcp-wallet`
//! and the editor process (`kask_bridge::wallet_approval`). Production uses
//! `IpcApprovalPort` over `HKASK_INFERENCE_SOCKET`; tests inject a boundary
//! implementation to test approved, declined, and cap-exceeded flows.

use async_trait::async_trait;
use hkask_types::wallet_wire::{
    WalletNwcOperationWire, WalletNwcResultWire, WalletSpendApprovalWire, WalletSpendRequestWire,
};
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApprovalError {
    #[error("editor IPC is not configured (HKASK_INFERENCE_SOCKET not set)")]
    IpcUnavailable,
    #[error("operator declined the payment: {0}")]
    Declined(String),
    #[error("spend policy denied by editor: {0}")]
    PolicyDenied(String),
    #[error("wallet not configured in editor keychain: {0}")]
    NotConfigured(String),
    #[error("payment timed out awaiting editor/NWC response: {0}")]
    Timeout(String),
    #[error("invalid response from editor/NWC: {0}")]
    InvalidResponse(String),
    #[error("payment failed: {0}")]
    PaymentFailed(String),
    #[error("editor approval request failed: {0}")]
    Transport(String),
}

#[async_trait]
pub trait ApprovalPort: Send + Sync {
    /// Request the SQLCipher database passphrase and editor-derived 64-byte
    /// NUT-13 seed hex from the editor keychain.
    async fn request_db_unlock(
        &self,
    ) -> Result<(Zeroizing<String>, Zeroizing<[u8; 64]>), ApprovalError>;

    /// Request mandatory native GPUI operator approval for one bounded spend
    /// (and editor-side NWC execution when `request.rail == "nwc"`).
    async fn request_spend_approval(
        &self,
        request: WalletSpendRequestWire,
    ) -> Result<WalletSpendApprovalWire, ApprovalError>;

    /// Execute a non-spending editor-side NIP-47 NWC query (`get_balance`,
    /// `make_invoice`, `lookup_invoice`).
    async fn nwc_query(
        &self,
        operation: WalletNwcOperationWire,
    ) -> Result<WalletNwcResultWire, ApprovalError>;
}

/// Production `ApprovalPort` backed by `hkask_inference::InferenceIpcClient`.
pub struct IpcApprovalPort {
    client: hkask_inference::InferenceIpcClient,
}

impl IpcApprovalPort {
    pub async fn from_env() -> Option<Self> {
        match hkask_inference::InferenceIpcClient::from_env().await? {
            Ok(client) => Some(Self { client }),
            Err(err) => {
                tracing::warn!(
                    target: "reg.wallet",
                    %err,
                    "failed to connect to editor inference IPC socket; wallet will start locked"
                );
                None
            }
        }
    }
}

fn map_ipc_err(err: hkask_types::InferenceError) -> ApprovalError {
    let msg = err.to_string();
    if msg.contains("Cancelled") || msg.contains("declined") {
        ApprovalError::Declined(msg)
    } else if msg.contains("PolicyDenied") {
        ApprovalError::PolicyDenied(msg)
    } else if msg.contains("NotConfigured") {
        ApprovalError::NotConfigured(msg)
    } else if msg.contains("Timeout") || msg.contains("timed out") {
        ApprovalError::Timeout(msg)
    } else if msg.contains("InvalidResponse") || msg.contains("preimage") {
        ApprovalError::InvalidResponse(msg)
    } else if msg.contains("PaymentFailed") {
        ApprovalError::PaymentFailed(msg)
    } else {
        ApprovalError::Transport(msg)
    }
}

#[async_trait]
impl ApprovalPort for IpcApprovalPort {
    async fn request_db_unlock(
        &self,
    ) -> Result<(Zeroizing<String>, Zeroizing<[u8; 64]>), ApprovalError> {
        let (passphrase, seed_hex) = self.client.wallet_db_unlock().await.map_err(map_ipc_err)?;
        let seed_hex = Zeroizing::new(seed_hex);
        let decoded = Zeroizing::new(hex::decode(seed_hex.as_str()).map_err(|_| {
            ApprovalError::Transport("editor returned malformed NUT-13 seed hex".to_string())
        })?);
        let seed_arr: [u8; 64] = decoded.as_slice().try_into().map_err(|_| {
            ApprovalError::Transport("editor returned wrong-length NUT-13 seed".to_string())
        })?;
        Ok((Zeroizing::new(passphrase), Zeroizing::new(seed_arr)))
    }

    async fn request_spend_approval(
        &self,
        request: WalletSpendRequestWire,
    ) -> Result<WalletSpendApprovalWire, ApprovalError> {
        self.client
            .wallet_spend_approve(request)
            .await
            .map_err(map_ipc_err)
    }

    async fn nwc_query(
        &self,
        operation: WalletNwcOperationWire,
    ) -> Result<WalletNwcResultWire, ApprovalError> {
        self.client
            .wallet_nwc_query(operation)
            .await
            .map_err(map_ipc_err)
    }
}

/// Fail-closed port used when `HKASK_INFERENCE_SOCKET` is absent.
pub struct UnavailableApprovalPort;

#[async_trait]
impl ApprovalPort for UnavailableApprovalPort {
    async fn request_db_unlock(
        &self,
    ) -> Result<(Zeroizing<String>, Zeroizing<[u8; 64]>), ApprovalError> {
        Err(ApprovalError::IpcUnavailable)
    }

    async fn request_spend_approval(
        &self,
        _request: WalletSpendRequestWire,
    ) -> Result<WalletSpendApprovalWire, ApprovalError> {
        Err(ApprovalError::IpcUnavailable)
    }

    async fn nwc_query(
        &self,
        _operation: WalletNwcOperationWire,
    ) -> Result<WalletNwcResultWire, ApprovalError> {
        Err(ApprovalError::IpcUnavailable)
    }
}
