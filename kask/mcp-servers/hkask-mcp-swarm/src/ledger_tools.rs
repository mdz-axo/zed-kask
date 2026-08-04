//! Local ledger (wallet) tools — fund, balance, and history for the local
//! swarm ledger. Split from `hkask_mcp_swarm.rs` (M2). These are the read/seed
//! surface for the operator-funded local economy (no ABW calls, no spend).
use crate::SwarmServer;
use crate::error::map_local_swarm_error;
use crate::request_types::*;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = ledger_router, vis = "pub")]
impl SwarmServer {
    /// Fund the local swarm ledger. The operator deposits credits that
    /// `swarm_delegate_local` debits per call. The ledger must be
    /// operator-funded — no auto-replenishment (§15.6 — the strongest
    /// objection: a synthetic ledger breaks the corrective feedback loop).
    #[tool(
        description = "Deposit local credits into the swarm ledger. The operator funds the local economy — no auto-replenishment. If unfunded, swarm_delegate_local returns PaymentRequired. Returns the new balance."
    )]
    pub(crate) async fn swarm_fund_local(
        &self,
        parameters: Parameters<FundLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_fund_local", Some("pko"), async {
            let req = parameters.0;
            if req.credits <= 0 {
                return Err(McpToolError::invalid_argument(
                    "credits must be positive".to_string(),
                ));
            }
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            let new_balance = runtime.fund(req.credits).map_err(map_local_swarm_error)?;
            Ok(serde_json::json!({
                "funded": req.credits,
                "balance": new_balance,
                "asset": "credits",
            }))
        })
        .await
    }

    /// Read the local swarm ledger balance. The local economy is
    /// operator-funded (`swarm_fund_local`); an unfunded ledger reads 0.
    /// This is the read-only sense input for local mode — the panel shows it
    /// and the `swarm-intelligence` skill's local SENSE step reads it instead
    /// of inferring the balance from delegation responses.
    #[tool(
        description = "Read the local swarm ledger balance (credits). Operator-funded via swarm_fund_local; unfunded reads 0. No ABW calls, no spend. Returns balance + asset."
    )]
    pub(crate) async fn swarm_balance_local(
        &self,
        _parameters: Parameters<BalanceLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_balance_local", Some("pko"), async {
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            match runtime.balance() {
                // A failed measurement must be distinguishable from a measured
                // zero (the `.rules` trap) — surface it as an error, not 0.
                Some(balance) => Ok(serde_json::json!({
                    "balance": balance,
                    "asset": "credits",
                })),
                None => Err(McpToolError::unavailable(
                    "local ledger balance query failed — cannot verify funds".to_string(),
                )),
            }
        })
        .await
    }

    /// Read the local swarm ledger's recent transactions (funds and debits)
    /// for the operator account, newest first. This is the local-mode run
    /// history / reconciliation surface — the `swarm-intelligence` skill's
    /// local CHECK phase can reconcile actual debits against it, and the
    /// panel can show recent activity. Read-only, no spend.
    #[tool(
        description = "Read the local swarm ledger's recent transactions (fund and debit entries) for the operator account. Newest first. Each entry has id, timestamp, reference, kind (fund/debit), amount (signed), asset. Read-only — no spend, no ABW calls."
    )]
    pub(crate) async fn swarm_local_history(
        &self,
        parameters: Parameters<LocalHistoryRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_local_history", Some("pko"), async {
            let req = parameters.0;
            let limit = req.limit.unwrap_or(50).min(500) as usize;
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            let transactions = runtime.history(limit).map_err(map_local_swarm_error)?;
            Ok(serde_json::json!({
                "count": transactions.len(),
                "transactions": transactions,
            }))
        })
        .await
    }
}
