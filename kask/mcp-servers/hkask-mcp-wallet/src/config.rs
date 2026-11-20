//! Non-secret wallet configuration read from `HKASK_WALLET_*` env vars (D110).
//!
//! No secrets (seed mnemonic, SQLCipher passphrase, NWC URI) ever enter env.

use std::path::PathBuf;
use std::str::FromStr as _;

use hkask_wallet_core::{Satoshis, SpendCapPolicy, UrlError, WalletNetwork};
use thiserror::Error;

/// Errors returned when resolving and validating a mint URL against `WalletConfig`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MintResolutionError {
    #[error("no mint_url provided and kask.wallet.default_mint is empty")]
    MissingMintUrl,
    #[error("mint_url rejected by SSRF perimeter: {0}")]
    RejectedBySsrf(#[from] UrlError),
    #[error("mint_url is not in kask.wallet.allowed_mints")]
    NotAllowlisted,
}

pub const DEFAULT_PER_TX_CAP_SATS: u64 = 10_000;
pub const DEFAULT_DAILY_CAP_SATS: u64 = 50_000;
pub const DEFAULT_MAX_FEE_SATS: u64 = 100;

/// Public policy snapshot for `hkask-mcp-wallet`.
///
/// The editor process (`kask_bridge::wallet_approval`) remains the authoritative
/// enforcer of caps, network, and mint allowlists; this copy lets the child
/// fail fast before making network calls and report policy in `wallet_status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletConfig {
    pub network: WalletNetwork,
    pub mainnet_enabled: bool,
    pub per_tx_cap_sats: u64,
    pub daily_cap_sats: u64,
    pub max_fee_sats: u64,
    pub allowed_mints: Vec<String>,
    pub default_mint: String,
    pub nwc_label: String,
    pub db_path: PathBuf,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            network: WalletNetwork::Regtest,
            mainnet_enabled: false,
            per_tx_cap_sats: DEFAULT_PER_TX_CAP_SATS,
            daily_cap_sats: DEFAULT_DAILY_CAP_SATS,
            max_fee_sats: DEFAULT_MAX_FEE_SATS,
            allowed_mints: Vec::new(),
            default_mint: String::new(),
            nwc_label: String::new(),
            db_path: default_db_path(),
        }
    }
}

impl WalletConfig {
    /// Read public wallet policy from the governed launch environment.
    pub fn from_env() -> Self {
        let network = std::env::var("HKASK_WALLET_NETWORK")
            .ok()
            .and_then(|v| WalletNetwork::from_str(&v).ok())
            .unwrap_or(WalletNetwork::Regtest);

        let mainnet_enabled = std::env::var("HKASK_WALLET_MAINNET_ENABLED")
            .ok()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));

        let per_tx_cap_sats = std::env::var("HKASK_WALLET_PER_TX_CAP_SATS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_PER_TX_CAP_SATS);

        let daily_cap_sats = std::env::var("HKASK_WALLET_DAILY_CAP_SATS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_DAILY_CAP_SATS);

        let max_fee_sats = std::env::var("HKASK_WALLET_MAX_FEE_SATS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_FEE_SATS);

        let allowed_mints = std::env::var("HKASK_WALLET_ALLOWED_MINTS")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let default_mint = std::env::var("HKASK_WALLET_DEFAULT_MINT")
            .ok()
            .map(|v| v.trim().to_string())
            .unwrap_or_default();

        let nwc_label = std::env::var("HKASK_WALLET_NWC_LABEL")
            .ok()
            .map(|v| v.trim().to_string())
            .unwrap_or_default();

        Self {
            network,
            mainnet_enabled,
            per_tx_cap_sats,
            daily_cap_sats,
            max_fee_sats,
            allowed_mints,
            default_mint,
            nwc_label,
            db_path: default_db_path(),
        }
    }

    pub fn to_spend_cap_policy(&self) -> SpendCapPolicy {
        SpendCapPolicy {
            network: self.network,
            mainnet_enabled: self.mainnet_enabled,
            per_tx_cap_sats: Satoshis::new(self.per_tx_cap_sats),
            daily_cap_sats: Satoshis::new(self.daily_cap_sats),
            max_fee_sats: Satoshis::new(self.max_fee_sats),
            allowed_mints: self.allowed_mints.clone(),
        }
    }

    /// Resolve a requested mint URL (or fall back to `default_mint`) and
    /// verify it is both SSRF-safe for `self.network` and in `allowed_mints`.
    pub fn resolve_allowed_mint(
        &self,
        requested: Option<&str>,
    ) -> Result<String, MintResolutionError> {
        let raw = requested
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.default_mint.trim());
        if raw.is_empty() {
            return Err(MintResolutionError::MissingMintUrl);
        }
        let allow_loopback = self.network.allows_loopback_urls();
        let validated = hkask_wallet_core::validate_public_https_url(raw, allow_loopback)?;
        let is_allowed = self.allowed_mints.iter().any(|allowed| {
            hkask_wallet_core::validate_public_https_url(allowed, allow_loopback)
                .is_ok_and(|a| a.as_str() == validated.as_str())
        });
        if !is_allowed {
            return Err(MintResolutionError::NotAllowlisted);
        }
        Ok(validated.as_str().to_string())
    }
}

/// Canonical D28 path: `<data_dir>/mcp/wallet/wallet.db`.
pub fn default_db_path() -> PathBuf {
    hkask_types::agent_paths::resolve_under_data_dir(&hkask_types::agent_paths::mcp_server_subdir(
        hkask_types::wallet_wire::WALLET_SERVER_ID,
        "wallet.db",
    ))
}
