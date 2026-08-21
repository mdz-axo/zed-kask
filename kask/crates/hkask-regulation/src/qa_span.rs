//! QA Regulation spans.
//!
//! Two span families:
//! - `QaRepair*` — mutation/repair loop spans (existing).
//! - `QaRun*` — QA routine pass spans, emitted by
//!   `kask/scripts/qa-mcp-servers.sh` per (tool, category) cell. Registered
//!   in Phase 4 of the MCP server QA strategy
//!   (`kask/docs/qa/mcp-server-qa-strategy.md`).
use hkask_types::ObservableSpan;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum QaSpan {
    QaRepairAttempted,
    QaRepairVerified,
    QaRepairExhausted,
    /// QA routine pass executed a tool's contract category and it passed.
    QaRunPass,
    /// QA routine pass executed a tool's contract category and it failed.
    QaRunFail,
    /// QA routine pass skipped a tool's contract category with a stated reason.
    QaRunSkipped,
}

impl QaSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            QaSpan::QaRepairAttempted => "reg.qa.repair_attempted",
            QaSpan::QaRepairVerified => "reg.qa.repair_verified",
            QaSpan::QaRepairExhausted => "reg.qa.repair_exhausted",
            QaSpan::QaRunPass => "reg.qa.run.pass",
            QaSpan::QaRunFail => "reg.qa.run.fail",
            QaSpan::QaRunSkipped => "reg.qa.run.skipped",
        }
    }
}

impl std::fmt::Display for QaSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ObservableSpan for QaSpan {
    fn as_str(&self) -> &'static str {
        QaSpan::as_str(self)
    }
}
