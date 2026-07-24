//! Wallet-specific Regulation span identifiers.
//!
//! Moved from `hkask_types::regulation::RegulationSpan` during the Regulation refactoring.
//! Implements [`ObservableSpan`] for use with
//! `SpanNamespace::from_observable()`.

use hkask_types::observable_span::ObservableSpan;

/// Wallet lifecycle and operation spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalletSpan {
    /// Wallet balance queried or updated.
    Balance,
    /// Deposit detected on-chain.
    Deposit,
    /// Shielded (private) deposit detected.
    DepositShielded,
    /// Withdrawal submitted to chain.
    Withdrawal,
    /// Currency conversion (rJ to USDC or vice versa).
    Conversion,
    /// API key issued.
    KeyIssued,
    /// API key revoked.
    KeyRevoked,
    /// API key expired.
    KeyExpired,
    /// API key exhausted (usage limit reached).
    KeyExhausted,
    /// Blockchain chain error.
    ChainError,
    /// Wallet created.
    Created,
    /// Wallet balance drawn.
    Draw,
    /// Wallet gas spent.
    Spend,
    /// Wallet balance exhausted.
    Exhausted,
}

impl WalletSpan {
    /// Canonical namespace string (e.g. `"reg.wallet.balance"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            WalletSpan::Balance => "reg.wallet.balance",
            WalletSpan::Deposit => "reg.wallet.deposit",
            WalletSpan::DepositShielded => "reg.wallet.deposit_shielded",
            WalletSpan::Withdrawal => "reg.wallet.withdrawal",
            WalletSpan::Conversion => "reg.wallet.conversion",
            WalletSpan::KeyIssued => "reg.wallet.key_issued",
            WalletSpan::KeyRevoked => "reg.wallet.key_revoked",
            WalletSpan::KeyExpired => "reg.wallet.key_expired",
            WalletSpan::KeyExhausted => "reg.wallet.key_exhausted",
            WalletSpan::ChainError => "reg.wallet.chain_error",
            WalletSpan::Created => "reg.wallet.created",
            WalletSpan::Draw => "reg.wallet.draw",
            WalletSpan::Spend => "reg.wallet.spend",
            WalletSpan::Exhausted => "reg.wallet.exhausted",
        }
    }
}

impl std::fmt::Display for WalletSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ObservableSpan for WalletSpan {
    fn as_str(&self) -> &'static str {
        WalletSpan::as_str(self)
    }
}

impl std::str::FromStr for WalletSpan {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.wallet.balance" => Ok(WalletSpan::Balance),
            "reg.wallet.deposit" => Ok(WalletSpan::Deposit),
            "reg.wallet.deposit_shielded" => Ok(WalletSpan::DepositShielded),
            "reg.wallet.withdrawal" => Ok(WalletSpan::Withdrawal),
            "reg.wallet.conversion" => Ok(WalletSpan::Conversion),
            "reg.wallet.key_issued" => Ok(WalletSpan::KeyIssued),
            "reg.wallet.key_revoked" => Ok(WalletSpan::KeyRevoked),
            "reg.wallet.key_expired" => Ok(WalletSpan::KeyExpired),
            "reg.wallet.key_exhausted" => Ok(WalletSpan::KeyExhausted),
            "reg.wallet.chain_error" => Ok(WalletSpan::ChainError),
            "reg.wallet.created" => Ok(WalletSpan::Created),
            "reg.wallet.draw" => Ok(WalletSpan::Draw),
            "reg.wallet.spend" => Ok(WalletSpan::Spend),
            "reg.wallet.exhausted" => Ok(WalletSpan::Exhausted),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn wallet_span_namespaces_are_canonical() {
        let all = vec![
            WalletSpan::Balance,
            WalletSpan::Deposit,
            WalletSpan::DepositShielded,
            WalletSpan::Withdrawal,
            WalletSpan::Conversion,
            WalletSpan::KeyIssued,
            WalletSpan::KeyRevoked,
            WalletSpan::KeyExpired,
            WalletSpan::KeyExhausted,
            WalletSpan::ChainError,
            WalletSpan::Created,
            WalletSpan::Draw,
            WalletSpan::Spend,
            WalletSpan::Exhausted,
        ];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "WalletSpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }

    #[test]
    fn wallet_span_survives_round_trip() {
        let s = WalletSpan::Balance;
        let parsed: WalletSpan = s.to_string().parse().unwrap();
        assert_eq!(parsed, s);
    }
}
