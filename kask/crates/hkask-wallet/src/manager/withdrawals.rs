//! Withdrawal pipeline and shield operations.

use super::*;
use crate::signing;

impl WalletManager {
    pub async fn withdraw(
        &self,
        actor: &WebID,
        wallet_id: WalletId,
        amount_rj: RJoule,
        to_address: &str,
        chain: ChainId,
        privacy: PrivacyMode,
    ) -> Result<TxHash, WalletError> {
        self.verify_withdrawal_chain(chain, privacy)?;

        let amount_usdc_micro = self.rjoules_to_usdc(amount_rj);
        self.emit_conversion_span(actor, wallet_id, amount_rj, amount_usdc_micro);

        let tx_hash = self
            .build_and_submit_withdrawal(actor, to_address, amount_usdc_micro, chain, privacy)
            .await?;

        self.emit_span_with_actor(actor, WalletSpan::Withdrawal, "submitted", CyclePhase::Act,
            serde_json::json!({"actor": actor.to_string(), "chain": chain.to_string(), "tx_hash": tx_hash.0}));

        // Debit after successful chain submission with the real tx_hash.
        // The transaction is recorded atomically by debit_rjoules.
        self.store.debit_rjoules(
            wallet_id,
            amount_rj,
            TransactionType::Withdrawal {
                chain,
                privacy,
                tx_hash: tx_hash.0.clone(),
                amount_usdc_micro,
            },
        )?;

        Ok(tx_hash)
    }

    fn verify_withdrawal_chain(
        &self,
        chain: ChainId,
        _privacy: PrivacyMode,
    ) -> Result<(), WalletError> {
        self.chains.get(&chain).ok_or(WalletError::ChainError {
            chain,
            message: "chain not enabled".to_string(),
        })?;
        Ok(())
    }

    fn emit_conversion_span(
        &self,
        actor: &WebID,
        wallet_id: WalletId,
        amount_rj: RJoule,
        amount_usdc_micro: u64,
    ) {
        self.emit_span_with_actor(actor, WalletSpan::Conversion, "converted", CyclePhase::Act,
            serde_json::json!({"actor": actor.to_string(), "wallet_id": wallet_id.to_string(),
                "rjoules": amount_rj.as_u64(), "usdc_micro": amount_usdc_micro, "direction": "rj_to_usdc"}));
    }

    async fn build_and_submit_withdrawal(
        &self,
        actor: &WebID,
        to_address: &str,
        amount_usdc_micro: u64,
        chain: ChainId,
        _privacy: PrivacyMode,
    ) -> Result<TxHash, WalletError> {
        let port = self.chains.get(&chain).ok_or(WalletError::ChainError {
            chain,
            message: "chain port not found after verification".to_string(),
        })?;
        let tx_bytes = port.build_withdrawal_tx(to_address, amount_usdc_micro)?;
        self.emit_span_with_actor(actor, WalletSpan::Withdrawal, "built", CyclePhase::Act,
            serde_json::json!({"actor": actor.to_string(), "chain": chain.to_string(),
                "to_address": to_address, "amount_usdc_micro": amount_usdc_micro, "privacy": "transparent"}));
        let signature = signing::sign_withdrawal(chain, &tx_bytes)?;
        self.emit_span_with_actor(
            actor,
            WalletSpan::Withdrawal,
            "signed",
            CyclePhase::Act,
            serde_json::json!({"actor": actor.to_string(), "chain": chain.to_string()}),
        );
        let mut signed_tx = tx_bytes;
        signed_tx.extend_from_slice(&signature);
        port.submit_signed_tx(actor, &signed_tx).await
    }
}
