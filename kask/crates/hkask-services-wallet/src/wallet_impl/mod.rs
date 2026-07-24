//! WalletService — Composes WalletManager, ApiKeyIssuer, and Regulation integration.
//!
//! Provides a clean interface for CLI and API surfaces. Hides the internal
//! `Arc<>` sharing pattern so callers don't repeat boilerplate at every call site.

use std::collections::HashMap;
use std::sync::Arc;

use hkask_pods::consent::ConsentManager;
use hkask_regulation::CyberneticsLoop;
use hkask_storage::WalletStore;
use hkask_types::event::RegulationSink;
use hkask_wallet::{ApiKeyIssuer, WalletManager};
use hkask_wallet::{ChainId, WalletConfig};
use tokio::sync::RwLock;

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

mod gas_regulation;
mod transactions;

#[cfg(test)]
mod tests;

/// Service for wallet operations — balance, deposits, withdrawals, API keys.
///
/// Wraps `WalletManager` and `ApiKeyIssuer` behind a clean interface.
/// Optionally enforces P2 affirmative consent for withdrawal signing (MUST-4).
/// Constructed during startup — never created directly by surfaces.
#[derive(Clone)]
pub struct WalletService {
    pub(crate) manager: Arc<WalletManager>,
    pub(crate) issuer: Arc<ApiKeyIssuer>,
    /// Optional Regulation loop for registering wallet-backed budgets.
    pub(crate) cybernetics: Option<Arc<RwLock<CyberneticsLoop>>>,
    /// Optional consent manager for P2 affirmative consent (MUST-4).
    pub(crate) consent_manager: Option<Arc<ConsentManager>>,
}

impl WalletService {
    /// Create a new WalletService from its components.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  manager must be a valid `Arc<WalletManager>`; issuer must be a valid `Arc<ApiKeyIssuer>`
    /// post: returns WalletService with manager and issuer wired; cybernetics and consent_manager default to None
    #[must_use]
    pub fn new(manager: Arc<WalletManager>, issuer: Arc<ApiKeyIssuer>) -> Self {
        Self {
            manager,
            issuer,
            cybernetics: None,
            consent_manager: None,
        }
    }

    /// Attach a CyberneticsLoop for Regulation wallet budget registration.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  loop_ must be a valid ``Arc<RwLock<CyberneticsLoop>>``
    /// post: returns self with cybernetics set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_cybernetics(mut self, loop_: Arc<RwLock<CyberneticsLoop>>) -> Self {
        self.cybernetics = Some(loop_);
        self
    }

    /// Attach a ConsentManager for P2 affirmative consent enforcement (MUST-4).
    ///
    /// When configured, withdrawal operations require explicit user consent
    /// via `DataCategory::Custom("wallet_withdrawal")`.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  cm must be a valid `Arc<ConsentManager>`
    /// post: returns self with consent_manager set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_consent_manager(mut self, cm: Arc<ConsentManager>) -> Self {
        self.consent_manager = Some(cm);
        self
    }

    /// Access the underlying WalletManager (for orchestration: ensure_wallet, deposit monitor).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be constructed
    /// post: returns `&Arc<WalletManager>`
    pub fn manager(&self) -> &Arc<WalletManager> {
        &self.manager
    }

    /// Access the underlying ApiKeyIssuer (for key creation, revocation, listing).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be constructed
    /// post: returns `&Arc<ApiKeyIssuer>`
    pub fn issuer(&self) -> &Arc<ApiKeyIssuer> {
        &self.issuer
    }

    /// Build a fully-wired WalletService from config, store, and Regulation infrastructure.
    ///
    /// Encapsulates chain port assembly (Hedera, Hinkal), price feed
    /// resolution, WalletManager construction, and ApiKeyIssuer creation.
    /// This is the single entry point for production wallet construction —
    /// `context.rs` calls this and handles only orchestration (userpod binding,
    /// deposit monitor spawning).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  config must be valid; store must be initialized; event_sink must be valid; cybernetics must be valid
    /// post: returns `Arc<WalletService>` with chain ports, price feed, WalletManager, and ApiKeyIssuer all wired; Err on construction failure
    #[must_use = "result must be used"]
    pub fn build(
        config: &WalletConfig,
        store: Arc<WalletStore>,
        event_sink: Arc<dyn RegulationSink>,
        cybernetics: Arc<RwLock<CyberneticsLoop>>,
    ) -> Result<Arc<Self>, ServiceError> {
        // ── Build chain ports from environment ────────────────────────────
        #[allow(unused_mut)]
        let mut chains: HashMap<ChainId, Arc<dyn hkask_wallet::ChainPort>> = HashMap::new();

        // Hedera — self-custody via mirror node + gRPC
        #[cfg(feature = "hedera")]
        if let Ok(treasury_account) = std::env::var("HEDERA_TREASURY_ACCOUNT") {
            let mirror_url = std::env::var("HEDERA_MIRROR_NODE_URL")
                .unwrap_or_else(|_| "https://mainnet-public.mirrornode.hedera.com".to_string());
            let consensus_url = std::env::var("HEDERA_CONSENSUS_NODE_URL")
                .unwrap_or_else(|_| "https://35.232.244.145:50211".to_string());
            match hkask_wallet::hedera::HederaPort::new(
                &mirror_url,
                &treasury_account,
                None,
                &consensus_url,
            ) {
                Ok(port) => {
                    let port = port.with_event_sink(Arc::clone(&event_sink));
                    tracing::info!(
                        target: "reg.wallet.chain",
                        chain = "hedera",
                        mirror_url = %mirror_url,
                        consensus_url = %consensus_url,
                        "HederaPort initialized"
                    );
                    chains.insert(ChainId::Hedera, Arc::new(port));
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.wallet.chain",
                        chain = "hedera",
                        error = %e,
                        "Failed to initialize HederaPort"
                    );
                }
            }
        }

        // ── Resolve price feed ──────────────────────────────────────────
        let price_feed = hkask_wallet::resolve_price_feed(&config.price_feed).map_err(|e| {
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: "Failed to resolve price feed".into(),
            }
        })?;

        // ── Build WalletManager ──────────────────────────────────────────
        let manager = Arc::new(
            WalletManager::build(config.clone(), Arc::clone(&store), chains, price_feed)
                .map_err(|e| ServiceError::Domain {
                    domain: DomainKind::Wallet,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: "Failed to build WalletManager".into(),
                })?
                .with_event_sink(Arc::clone(&event_sink)),
        );

        // ── Build ApiKeyIssuer ───────────────────────────────────────────
        let issuer = Arc::new(
            ApiKeyIssuer::new(Arc::clone(&store))
                .map_err(|e| ServiceError::Domain {
                    domain: DomainKind::Wallet,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: "Failed to build ApiKeyIssuer".into(),
                })?
                .with_event_sink(Arc::clone(&event_sink)),
        );

        Ok(Arc::new(
            Self::new(manager, issuer).with_cybernetics(cybernetics),
        ))
    }
}
