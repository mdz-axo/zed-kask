//! Wallet network and payment rail enums.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bitcoin/Lightning network the wallet operates on.
///
/// Defaults to [`WalletNetwork::Regtest`] so development and testing never
/// touch real funds unless the operator explicitly enables mainnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalletNetwork {
    #[default]
    Regtest,
    Signet,
    Mainnet,
}

impl WalletNetwork {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Regtest => "regtest",
            Self::Signet => "signet",
            Self::Mainnet => "mainnet",
        }
    }

    pub const fn display_upper(self) -> &'static str {
        match self {
            Self::Regtest => "REGTEST",
            Self::Signet => "SIGNET",
            Self::Mainnet => "MAINNET",
        }
    }

    pub const fn is_mainnet(self) -> bool {
        matches!(self, Self::Mainnet)
    }

    /// Loopback URLs (e.g. local regtest mints) are permitted only on regtest.
    pub const fn allows_loopback_urls(self) -> bool {
        matches!(self, Self::Regtest)
    }
}

impl fmt::Display for WalletNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unsupported wallet network (expected regtest, signet, or mainnet)")]
pub struct ParseWalletNetworkError;

impl FromStr for WalletNetwork {
    type Err = ParseWalletNetworkError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "regtest" => Ok(Self::Regtest),
            "signet" => Ok(Self::Signet),
            "mainnet" | "bitcoin" => Ok(Self::Mainnet),
            _ => Err(ParseWalletNetworkError),
        }
    }
}

/// Payment rail for a wallet operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentRail {
    Cashu,
    Nwc,
}

impl PaymentRail {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cashu => "cashu",
            Self::Nwc => "nwc",
        }
    }
}

impl fmt::Display for PaymentRail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
