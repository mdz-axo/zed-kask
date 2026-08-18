use super::*;

/// SpendEntry — a record of rJoule consumed or added on a task.
///
/// Each entry tracks what operation consumed or granted rJoules, how much,
/// and when. This is the audit trail for subagent resource usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendEntry {
    /// Amount consumed (positive) or added (also positive, context is in `kind`).
    pub amount: u64,
    /// "rJoule spend (consumed) or refill (added by delegator).
    pub kind: String,
    /// What consumed the rJoules: "inference: deepseek-v4", "template: bug-hunt",
    /// "tool: kanban_task_list", etc.
    pub reason: String,
    /// When this entry was recorded.
    pub at: DateTime<Utc>,
}

impl SpendEntry {
    pub fn rjoule_spend(amount: u64, reason: String) -> Self {
        Self {
            amount,
            kind: "rjoule_spend".into(),
            reason,
            at: Utc::now(),
        }
    }
    pub fn rjoule_refill(amount: u64) -> Self {
        Self {
            amount,
            kind: "rjoule_refill".into(),
            reason: "delegator added rJoules".into(),
            at: Utc::now(),
        }
    }
}
