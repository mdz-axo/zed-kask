//! Encumbrance — rJoule lock/release/consume for API key allocation.

use super::*;

use hkask_types::regulation::RegulationSpan;

impl WalletManager {
    pub fn encumber(
        &self,
        wallet_id: WalletId,
        key_id: ApiKeyId,
        amount: RJoule,
    ) -> Result<(), WalletError> {
        self.store.encumber_rjoules(wallet_id, key_id, amount)?;
        self.emit_core_span(
            RegulationSpan::Gas,
            "encumbered",
            CyclePhase::Act,
            serde_json::json!({
                "key_id": key_id.to_string(),
                "wallet_id": wallet_id.to_string(),
                "amount_rj": amount.as_u64(),
            }),
        );
        Ok(())
    }

    pub fn release_encumbrance(&self, key_id: ApiKeyId) -> Result<(), WalletError> {
        self.store.release_encumbrance(key_id)?;
        self.emit_core_span(
            RegulationSpan::Gas,
            "released",
            CyclePhase::Act,
            serde_json::json!({
                "key_id": key_id.to_string(),
            }),
        );
        Ok(())
    }

    pub fn consume(&self, key_id: ApiKeyId, gas_rj: RJoule) -> Result<(), WalletError> {
        self.store.consume_encumbrance(key_id, gas_rj)?;
        Ok(())
    }

    pub fn get_encumbrance(&self, key_id: ApiKeyId) -> Result<Option<Encumbrance>, WalletError> {
        self.store.get_encumbrance(key_id)
    }
}
