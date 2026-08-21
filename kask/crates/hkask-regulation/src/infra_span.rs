//! Infrastructure Regulation spans — used across multiple subsystems.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InfraSpan {
    CiInvariantViolation,
    CuratorConsolidation,
    Chat,
    WalletConversion,
}

impl InfraSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            InfraSpan::CiInvariantViolation => "reg.ci.invariant.violation",
            InfraSpan::CuratorConsolidation => "reg.curator.consolidation",
            InfraSpan::Chat => "reg.chat",
            InfraSpan::WalletConversion => "reg.wallet.conversion",
        }
    }
}

impl std::fmt::Display for InfraSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ObservableSpan for InfraSpan {
    fn as_str(&self) -> &'static str {
        InfraSpan::as_str(self)
    }
}
