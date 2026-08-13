//! Local ledger (wallet) tools — fund, balance, and history for the local
//! swarm ledger. Split from `hkask_mcp_swarm.rs` (M2).
//!
//! The local ledger is **accounting, not authorization**: local agents run on
//! the operator's own substrate, so `swarm_delegate_local` never refuses for
//! lack of funds (see `LocalSwarmRuntime::delegate`). These tools are the
//! reconciliation surface — what was spent, not what is permitted. Funding
//! gates live on the *cloud* path, where credits buy someone else's compute.
//!
//! Because spend is recorded without a balance precondition, a balance may be
//! **negative**: that is the operator's unreconciled local spend, not a fault.
//! `swarm_fund_local` remains available for operators who want to track a budget
//! against a deposit.
use crate::SwarmServer;
use crate::error::map_local_swarm_error;
use crate::request_types::*;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = ledger_router, vis = "pub")]
impl SwarmServer {
    /// Fund the local swarm ledger. Optional: local delegation does not require
    /// funds (the ledger is accounting, not authorization). Depositing gives the
    /// operator a budget to reconcile spend against, so the balance reads as
    /// "remaining" rather than "consumed".
    #[tool(
        description = "Deposit local credits into the swarm ledger. OPTIONAL - local delegation never refuses for lack of funds; the ledger records spend rather than authorizing it. Fund it only to track spend against a budget. Returns the new balance."
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

    /// Read the local swarm ledger balance. An unfunded ledger that has run
    /// delegations reads **negative** — the accumulated local spend. This is the
    /// read-only sense input for local mode: the panel shows it and the
    /// `swarm-intelligence` skill's local SENSE step reads it instead of
    /// inferring the balance from delegation responses.
    ///
    /// Not a gate. A low or negative balance does not block local delegation; it
    /// reports what has been consumed.
    #[tool(
        description = "Read the local swarm ledger balance (credits). Records local spend rather than gating it, so an unfunded ledger that has run delegations reads negative. Does NOT block delegation. No ABW calls, no spend. Returns balance + asset."
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
