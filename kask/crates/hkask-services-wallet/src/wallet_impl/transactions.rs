//! Withdrawal operations — P2 consent-gated withdraw.

use super::WalletService;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::DataCategory;
use hkask_types::WebID;
use hkask_types::id::WalletId;
use hkask_wallet::{ChainId, PrivacyMode, RJoule, TxHash, WalletError};

impl WalletService {
    /// Withdraw rJoules as USDC to a user's primary wallet address.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  webid identifies the user requesting the withdrawal
    /// post: if consent_manager is Some and consent denied → Err(ConsentDenied)
    /// post: if consent_manager is None → proceeds without consent check
    pub async fn withdraw(
        &self,
        webid: &WebID,
        wallet_id: WalletId,
        amount_rj: RJoule,
        to_address: &str,
        chain: ChainId,
        privacy: PrivacyMode,
    ) -> Result<TxHash, ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.wallet_svc", operation = "withdraw", webid = %webid, wallet_id = %wallet_id, amount_rj = %amount_rj, chain = ?chain, "REG");
        if let Some(ref cm) = self.consent_manager {
            let category = DataCategory::Custom("wallet_withdrawal".into());
            let has_consent = cm.has_consent(&webid.to_string(), &category).map_err(|e| {
                ServiceError::Domain {
                    domain: DomainKind::Consent,
                    kind: ErrorKind::Forbidden,
                    source: None,
                    message: format!(
                        "Consent check failed for {}: {e}. Denying wallet withdrawal by default",
                        webid
                    ),
                }
            })?;
            if !has_consent {
                return Err(ServiceError::Domain {
                    domain: DomainKind::Consent,
                    kind: ErrorKind::Forbidden,
                    source: None,
                    message: format!(
                        "User {} has not granted consent for wallet withdrawal. \
                         Grant consent with: kask sovereignty grant {} wallet_withdrawal",
                        webid, webid
                    ),
                });
            }
        }

        self.manager
            .withdraw(webid, wallet_id, amount_rj, to_address, chain, privacy)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if matches!(e, WalletError::ChainError { .. }) {
                    self.manager
                        .emit_chain_error_for_actor(webid, chain, "withdraw", &msg);
                }
                ServiceError::Domain {
                    domain: DomainKind::Wallet,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: msg,
                }
            })
    }
}
