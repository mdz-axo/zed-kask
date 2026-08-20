//! Infrastructure Regulation spans — used across multiple subsystems.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfraSpan {
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

impl std::str::FromStr for InfraSpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.ci.invariant.violation" => Ok(InfraSpan::CiInvariantViolation),
            "reg.curator.consolidation" => Ok(InfraSpan::CuratorConsolidation),
            "reg.chat" => Ok(InfraSpan::Chat),
            "reg.wallet.conversion" => Ok(InfraSpan::WalletConversion),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for InfraSpan {
    fn as_str(&self) -> &'static str {
        InfraSpan::as_str(self)
    }
}
