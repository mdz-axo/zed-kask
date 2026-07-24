//! Regulation wallet budget registration — WalletBackedBudget creation and Regulation binding.
//!
//! These methods earn their existence through orchestration: composing
//! WalletBackedBudget with the CyberneticsLoop is non-trivial wiring that
//! surfaces should not repeat.

use std::sync::Arc;

use super::WalletService;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::id::{ApiKeyId, WalletId};
use hkask_wallet::RJoule;

impl WalletService {
    // ── Regulation Integration ─────────────────────────────────────────────────────

    /// Register a wallet-backed energy budget for an agent in the Regulation.
    ///
    /// The agent's tool invocations will debit rJoules from the wallet
    /// instead of consuming from the dimensionless gas pool.
    /// The gas→rJoule conversion rate is taken from the WalletManager's config.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  cybernetics must be attached via with_cybernetics(); agent must be a valid WebID; wallet_id must be valid
    /// post: wallet-backed budget is registered in Regulation for the agent; Err(Wallet) if cybernetics not attached
    pub async fn register_wallet_budget(
        &self,
        agent: hkask_types::WebID,
        wallet_id: WalletId,
    ) -> Result<(), ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.wallet_svc", operation = "register_wallet_budget", agent = %agent, wallet_id = %wallet_id, "REG");
        let loop_ = self
            .cybernetics
            .as_ref()
            .ok_or_else(|| ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: "CyberneticsLoop not attached to WalletService — call with_cybernetics() during construction".into(),
            })?;
        let budget = hkask_regulation::WalletBackedBudget::new(
            wallet_id,
            Arc::clone(&self.manager) as Arc<dyn hkask_types::WalletBudgetPort>,
        );
        loop_
            .read()
            .await
            .register_wallet_budget(agent, budget)
            .await;
        Ok(())
    }

    /// Register a wallet-backed energy budget with an API key for encumbrance tracking.
    ///
    /// Unlike `register_wallet_budget`, this attaches the API key so that
    /// gas consumption is debited from the key's encumbrance (not raw wallet
    /// balance). The spending limit is also tracked per-key.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  cybernetics must be attached; agent must be valid; wallet_id and key_id must be valid; spending_limit_rj must be >= 0
    /// post: wallet-backed budget with API key tracking is registered in Regulation; Err(Wallet) if cybernetics not attached
    pub async fn register_wallet_budget_for_key(
        &self,
        agent: hkask_types::WebID,
        wallet_id: WalletId,
        key_id: ApiKeyId,
        spending_limit_rj: RJoule,
    ) -> Result<(), ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.wallet_svc", operation = "register_wallet_budget_for_key", agent = %agent, wallet_id = %wallet_id, key_id = %key_id, "REG");
        let loop_ = self
            .cybernetics
            .as_ref()
            .ok_or_else(|| ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: "CyberneticsLoop not attached to WalletService — call with_cybernetics() during construction".into(),
            })?;
        let budget = hkask_regulation::WalletBackedBudget::new(
            wallet_id,
            Arc::clone(&self.manager) as Arc<dyn hkask_types::WalletBudgetPort>,
        )
        .with_api_key(key_id, spending_limit_rj);
        loop_
            .read()
            .await
            .register_wallet_budget(agent, budget)
            .await;
        Ok(())
    }
}
