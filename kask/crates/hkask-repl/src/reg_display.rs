//! Regulation algedonic alert display and loop system tick for the REPL.
//!
//! After each inference turn, the Regulation is checked for critical alerts
//! and the LoopScheduler is ticked. Regulation variety counters are updated by
//! the service layer via Regulation spans (reg.chat.request, reg.chat.response) —
//! the REPL only reads alerts, never mutates Regulation state directly.
//!
//! # REQ: P4-reg-access — REPL only reads Regulation alerts (read-only), never mutates Regulation state
//! expect: "I can access all hKask functionality through the kask CLI"

use hkask_services_context::AgentService;

/// Check Regulation algedonic alerts and tick the LoopScheduler after each turn.
/// Only reads Regulation state (alerts) and ticks loop system — no direct mutation.
/// Regulation variety counters are updated by the service layer via Regulation spans.
pub(super) fn update_reg_and_display(ctx: &AgentService, rt: &tokio::runtime::Handle) {
    // Check for Regulation algedonic alerts (read-only observation)
    let ledger_runtime = ctx.ledger().runtime.clone();
    let alerts = rt.block_on(async { ledger_runtime.read().await.critical_alerts().await });
    if !alerts.is_empty() {
        for alert in &alerts {
            println!(
                "  \x1b[31m\u{26a0} Regulation ALERT: {} (deficit: {}/{})\x1b[0m",
                alert.message, alert.deficit, alert.threshold
            );
        }
    }

    // Tick the LoopScheduler to run sense→compare→compute→act for
    // CyberneticsLoop and InferenceLoop. The CyberneticsLoop reads
    // Regulation variety and energy budgets, producing regulatory actions
    // (Throttle, AdjustEnergyBudget, Escalate, Calibrate) visible
    // through tracing output (reg.cybernetics target).
    rt.block_on(async {
        ctx.ledger().loops.tick().await;
    });
}
