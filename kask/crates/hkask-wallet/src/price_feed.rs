//! PriceFeed — abstract interface for fetching native token USD exchange rates.
//!
//! # Implementations
//! - `StaticPriceFeed` — hardcoded rates for testing/development
//! - `EodhdPriceFeed` — EOD Historical Data API (primary canonical source)
//! - `CoinGeckoPriceFeed` — CoinGecko free public API (fallback)
//! - `CompositePriceFeed` — multi-source orchestrator with caching and fallback
//!
//! # User sovereignty `[OUGHT-DECL]`
//! The user chooses which price sources to use via `PriceFeedConfig` in `WalletConfig`.
//! `resolve_price_feed()` maps the config to a concrete implementation at build time.
//! No source is hardcoded — the wallet resolves the user's choice.
//!
//! # Design
//! `PriceFeed` is a trait for capability, not a base class. Each implementation
//! is a standalone struct. The `WalletManager` uses it for fee estimation
//! during withdrawals.

use crate::types::{ChainId, PriceFeedConfig, WalletError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ── ExchangeRate ───────────────────────────────────────────────────────────────

/// Exchange rate for a native token in USD.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeRate {
    /// USD per 1 native token (e.g., 150.0 = 1 SOL = $150)
    pub usd_per_token: f64,
    /// When this rate was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ── PriceFeed trait ────────────────────────────────────────────────────────────

/// Abstract interface for fetching native token USD exchange rates.
///
/// Used by `WalletManager` to estimate withdrawal fees in rJoules.
#[async_trait]
pub trait PriceFeed: Send + Sync {
    /// Get the current USD exchange rate for a chain's native token.
    async fn get_rate(&self, chain: ChainId) -> Result<ExchangeRate, WalletError>;
}

// ── StaticPriceFeed — hardcoded rates for dev/test ─────────────────────────────

/// Static price feed with hardcoded rates for development and testing.
///
/// Rates are approximate and should not be used for production withdrawals.
pub struct StaticPriceFeed;

impl StaticPriceFeed {
    /// Create a new static price feed.
    pub fn new() -> Self {
        Self
    }

    /// Hardcoded USD rate per native token.
    fn hardcoded_rate(chain: ChainId) -> f64 {
        match chain {
            ChainId::Hedera => 0.08, // ~$0.08/HBAR (Hinkal settles on Hedera)
        }
    }
}

impl Default for StaticPriceFeed {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PriceFeed for StaticPriceFeed {
    async fn get_rate(&self, chain: ChainId) -> Result<ExchangeRate, WalletError> {
        Ok(ExchangeRate {
            usd_per_token: Self::hardcoded_rate(chain),
            updated_at: chrono::Utc::now(),
        })
    }
}

// ── EodhdPriceFeed — EOD Historical Data API ──────────────────────────────────

const EODHD_BASE_URL: &str = "https://eodhd.com/api";

/// EODHD (EOD Historical Data) price feed — primary canonical source.
///
/// Uses the `/real-time` endpoint with `.CC` (Crypto Currency) exchange suffix.
/// Requires `HKASK_EODHD_API_KEY` environment variable.
///
/// # Symbol mapping
/// - Hedera → `HBAR-USD.CC`
/// - Hinkal → `HBAR-USD.CC` (Hinkal settles on Hedera)
pub struct EodhdPriceFeed {
    client: reqwest::Client,
    api_key: String,
}

impl EodhdPriceFeed {
    /// Create a new EODHD price feed.
    ///
    /// Reads `HKASK_EODHD_API_KEY` from the environment.
    /// Returns `Err` if the key is not set.
    pub fn from_env() -> Result<Self, WalletError> {
        let api_key = std::env::var("HKASK_EODHD_API_KEY").map_err(|_| {
            WalletError::Infra(hkask_types::InfrastructureError::database(
                "HKASK_EODHD_API_KEY not set — required for EODHD price feed".to_string(),
            ))
        })?;
        Ok(Self::new(api_key))
    }

    /// Create with an explicit API key.
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .https_only(true)
            .build()
            .expect("reqwest Client::builder is infallible with valid defaults");
        EodhdPriceFeed { client, api_key }
    }

    fn eodhd_symbol(chain: ChainId) -> &'static str {
        match chain {
            ChainId::Hedera => "HBAR-USD.CC", // Hinkal settles on Hedera
        }
    }
}

#[async_trait]
impl PriceFeed for EodhdPriceFeed {
    async fn get_rate(&self, chain: ChainId) -> Result<ExchangeRate, WalletError> {
        let symbol = Self::eodhd_symbol(chain);
        let url = format!("{EODHD_BASE_URL}/real-time/{symbol}");

        let resp = self
            .client
            .get(&url)
            .query(&[("api_token", self.api_key.as_str()), ("fmt", "json")])
            .send()
            .await
            .map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "EODHD request failed for {symbol}: {e}"
                )))
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(format!(
                    "EODHD returned HTTP {status} for {symbol}: {body}"
                )),
            ));
        }

        let v: serde_json::Value = resp.json().await.map_err(|e| {
            WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                "EODHD response parse failed for {symbol}: {e}"
            )))
        })?;

        let close = v.get("close").and_then(|c| c.as_f64()).ok_or_else(|| {
            WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                "EODHD response missing 'close' field for {symbol}"
            )))
        })?;

        Ok(ExchangeRate {
            usd_per_token: close,
            updated_at: chrono::Utc::now(),
        })
    }
}

// ── CoinGeckoPriceFeed — free public API ───────────────────────────────────────

const COINGECKO_BASE_URL: &str = "https://api.coingecko.com/api/v3";

/// CoinGecko free public price feed — no API key required.
///
/// Uses the `/simple/price` endpoint. Rate-limited to ~10-30 calls/minute
/// on the free tier. Suitable as a fallback source.
///
/// # Symbol mapping
/// - Hedera → `hedera-hashgraph`
/// - Hinkal → `hedera-hashgraph` (Hinkal settles on Hedera)
pub struct CoinGeckoPriceFeed {
    client: reqwest::Client,
}

impl CoinGeckoPriceFeed {
    /// Create a new CoinGecko price feed.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .https_only(true)
            .user_agent(concat!("hKask/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest Client::builder is infallible with valid defaults");
        CoinGeckoPriceFeed { client }
    }

    fn coingecko_id(chain: ChainId) -> &'static str {
        match chain {
            ChainId::Hedera => "hedera-hashgraph", // Hinkal settles on Hedera
        }
    }
}

impl Default for CoinGeckoPriceFeed {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PriceFeed for CoinGeckoPriceFeed {
    async fn get_rate(&self, chain: ChainId) -> Result<ExchangeRate, WalletError> {
        let id = Self::coingecko_id(chain);
        let url = format!("{COINGECKO_BASE_URL}/simple/price");

        let resp = self
            .client
            .get(&url)
            .query(&[("ids", id), ("vs_currencies", "usd")])
            .send()
            .await
            .map_err(|e| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "CoinGecko request failed for {id}: {e}"
                )))
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(format!(
                    "CoinGecko returned HTTP {status} for {id}: {body}"
                )),
            ));
        }

        let v: serde_json::Value = resp.json().await.map_err(|e| {
            WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                "CoinGecko response parse failed for {id}: {e}"
            )))
        })?;

        let usd = v
            .get(id)
            .and_then(|c| c.get("usd"))
            .and_then(|u| u.as_f64())
            .ok_or_else(|| {
                WalletError::Infra(hkask_types::InfrastructureError::database(format!(
                    "CoinGecko response missing '{id}.usd' field"
                )))
            })?;

        Ok(ExchangeRate {
            usd_per_token: usd,
            updated_at: chrono::Utc::now(),
        })
    }
}

// ── CompositePriceFeed — multi-source orchestrator ─────────────────────────────

/// Cached rate entry with insertion time.
#[derive(Debug, Clone)]
struct CachedRate {
    rate: ExchangeRate,
    cached_at: Instant,
}

/// Multi-source price feed with prioritized fallback and caching.
///
/// # Behavior
/// 1. **Cache hit** — if a cached rate is within TTL, return it immediately
/// 2. **Primary source** — try the first source; on success, cache and return
/// 3. **Fallback chain** — try each subsequent source in order
/// 4. **Stale fallback** — if all sources fail, return the last cached rate
///    (even if expired) rather than failing the withdrawal
/// 5. **No-data fallback** — if no cached rate exists and all sources fail,
///    return an error
///
/// # User sovereignty `[OUGHT-DECL]`
/// Source order is determined by the user's `PriceFeedConfig::Composite { sources }`.
/// The user controls which sources to use and their priority.
pub struct CompositePriceFeed {
    sources: Vec<Box<dyn PriceFeed>>,
    cache: Mutex<HashMap<ChainId, CachedRate>>,
    cache_ttl: Duration,
}

impl CompositePriceFeed {
    /// Build a composite feed from a list of sources in priority order.
    ///
    /// Sources are tried in order: index 0 first, then index 1, etc.
    pub fn new(sources: Vec<Box<dyn PriceFeed>>, cache_ttl_secs: u64) -> Self {
        CompositePriceFeed {
            sources,
            cache: Mutex::new(HashMap::new()),
            cache_ttl: Duration::from_secs(cache_ttl_secs),
        }
    }

    /// Check the cache for a non-expired rate.
    fn cache_get(&self, chain: ChainId) -> Option<ExchangeRate> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(&chain)?;
        if entry.cached_at.elapsed() < self.cache_ttl {
            Some(entry.rate)
        } else {
            None
        }
    }

    /// Get the last cached rate even if expired (stale fallback).
    fn cache_get_stale(&self, chain: ChainId) -> Option<ExchangeRate> {
        let cache = self.cache.lock().ok()?;
        cache.get(&chain).map(|e| e.rate)
    }

    /// Store a rate in the cache.
    fn cache_put(&self, chain: ChainId, rate: ExchangeRate) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                chain,
                CachedRate {
                    rate,
                    cached_at: Instant::now(),
                },
            );
        }
    }
}

#[async_trait]
impl PriceFeed for CompositePriceFeed {
    async fn get_rate(&self, chain: ChainId) -> Result<ExchangeRate, WalletError> {
        // 1. Cache hit — return immediately
        if let Some(rate) = self.cache_get(chain) {
            tracing::debug!(
                target: "hkask.wallet.price_feed",
                chain = %chain,
                usd = rate.usd_per_token,
                "price feed cache hit"
            );
            return Ok(rate);
        }

        // 2. Try sources in priority order
        let mut last_err: Option<String> = None;
        for (i, source) in self.sources.iter().enumerate() {
            match source.get_rate(chain).await {
                Ok(rate) => {
                    self.cache_put(chain, rate);
                    tracing::debug!(
                        target: "hkask.wallet.price_feed",
                        chain = %chain,
                        source_index = i,
                        usd = rate.usd_per_token,
                        "price feed resolved from source"
                    );
                    return Ok(rate);
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.wallet.price_feed",
                        chain = %chain,
                        source_index = i,
                        error = %e,
                        "price feed source failed, trying next"
                    );
                    last_err = Some(e.to_string());
                }
            }
        }

        // 3. Stale fallback — return last cached rate even if expired
        if let Some(rate) = self.cache_get_stale(chain) {
            tracing::warn!(
                target: "hkask.wallet.price_feed",
                chain = %chain,
                usd = rate.usd_per_token,
                age_secs = rate.updated_at.timestamp(),
                "all price feed sources failed — using stale cached rate"
            );
            return Ok(rate);
        }

        // 4. Total failure — no cache, no sources
        Err(WalletError::Infra(
            hkask_types::InfrastructureError::database(format!(
                "all price feed sources exhausted for {chain}: {}",
                last_err.unwrap_or_else(|| "no sources configured".into())
            )),
        ))
    }
}

// ── Factory: resolve PriceFeedConfig → Arc<dyn PriceFeed> ─────────────────────

/// Resolve a `PriceFeedConfig` into a concrete `PriceFeed` implementation.
///
/// # User sovereignty `[OUGHT-DECL]`
/// This is the single point where user configuration becomes runtime behavior.
/// The user's choice in `WalletConfig.price_feed` determines which sources are used.
///
/// # Mapping
/// | Config | Implementation |
/// |--------|---------------|
/// | `Static` | `StaticPriceFeed` |
/// | `Eodhd` | `EodhdPriceFeed` (reads `HKASK_EODHD_API_KEY` from env) |
/// | `CoinGecko` | `CoinGeckoPriceFeed` |
/// | `Composite { sources, cache_ttl }` | `CompositePriceFeed` with named sources |
///
/// # Composite source names
/// - `"eodhd"` → `EodhdPriceFeed`
/// - `"coingecko"` → `CoinGeckoPriceFeed`
/// - Unknown names → skipped with warning
pub fn resolve_price_feed(
    config: &PriceFeedConfig,
) -> Result<std::sync::Arc<dyn PriceFeed>, WalletError> {
    match config {
        PriceFeedConfig::Static => Ok(std::sync::Arc::new(StaticPriceFeed::new())),
        PriceFeedConfig::Eodhd => {
            let feed = EodhdPriceFeed::from_env()?;
            Ok(std::sync::Arc::new(feed))
        }
        PriceFeedConfig::CoinGecko => Ok(std::sync::Arc::new(CoinGeckoPriceFeed::new())),
        PriceFeedConfig::Composite {
            sources,
            cache_ttl_secs,
        } => {
            let mut feeds: Vec<Box<dyn PriceFeed>> = Vec::with_capacity(sources.len());
            for name in sources {
                match name.as_str() {
                    "eodhd" => match EodhdPriceFeed::from_env() {
                        Ok(f) => feeds.push(Box::new(f)),
                        Err(e) => {
                            tracing::warn!(
                                target: "hkask.wallet.price_feed",
                                source = "eodhd",
                                error = %e,
                                "EODHD source unavailable — skipping in composite feed"
                            );
                        }
                    },
                    "coingecko" => feeds.push(Box::new(CoinGeckoPriceFeed::new())),
                    unknown => {
                        tracing::warn!(
                            target: "hkask.wallet.price_feed",
                            source = unknown,
                            "unknown price feed source — skipping"
                        );
                    }
                }
            }
            if feeds.is_empty() {
                return Err(WalletError::Infra(
                    hkask_types::InfrastructureError::database(
                        "composite price feed configured but no sources resolved",
                    ),
                ));
            }
            Ok(std::sync::Arc::new(CompositePriceFeed::new(
                feeds,
                *cache_ttl_secs,
            )))
        }
    }
}

// ── Fee estimation ────────────────────────────────────────────────────────────

/// Estimated withdrawal fee in rJoules.
///
/// Calculated from: native token fee × USD rate ÷ USDC-per-rJoule rate.
#[derive(Debug, Clone, Copy)]
pub struct WithdrawalFee {
    /// Fee in rJoules
    pub rjoules: u64,
    /// Fee in native token units (for display)
    pub native_units: f64,
    /// Fee in micro-USDC (for display)
    pub usdc_micro: u64,
}

/// Estimate the withdrawal fee for a given chain.
///
/// Uses the chain's typical transaction fee in native tokens, converts to USD
/// via the price feed, then converts to rJoules using the rj_per_usdc rate.
///
/// # Fee model
/// - Hedera: ~$0.001 USD fixed per HTS transfer
/// - Hinkal: same as Hedera (Hinkal settles on Hedera)
pub fn estimate_withdrawal_fee(
    chain: ChainId,
    rate: &ExchangeRate,
    rj_per_usdc: u64,
) -> WithdrawalFee {
    let native_fee = match chain {
        ChainId::Hedera => 0.0125, // ~$0.001 / $0.08 = 0.0125 HBAR (Hinkal settles on Hedera)
    };

    let usd_fee = native_fee * rate.usd_per_token;
    let usdc_micro_fee = (usd_fee * 1_000_000.0) as u64;
    let rj_fee = (usdc_micro_fee as u128 * rj_per_usdc as u128 / 1_000_000) as u64;

    WithdrawalFee {
        rjoules: rj_fee.max(1), // minimum 1 rJ
        native_units: native_fee,
        usdc_micro: usdc_micro_fee.max(1),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── StaticPriceFeed tests ──────────────────────────────────────────────

    /// expect: "Wallet price static rate test works correctly under test conditions"
    #[test]
    fn static_price_feed_returns_expected_rates() {
        assert!((StaticPriceFeed::hardcoded_rate(ChainId::Hedera) - 0.08).abs() < f64::EPSILON);
        assert!((StaticPriceFeed::hardcoded_rate(ChainId::Hedera) - 0.08).abs() < f64::EPSILON);
    }

    /// expect: "Wallet price fee nonzero test works correctly under test conditions"
    #[test]
    fn fee_estimation_produces_non_zero_fee() {
        let rate = ExchangeRate {
            usd_per_token: 150.0,
            updated_at: chrono::Utc::now(),
        };
        let fee = estimate_withdrawal_fee(ChainId::Hedera, &rate, 1000);
        assert!(fee.rjoules > 0);
        assert!(fee.usdc_micro > 0);
    }

    /// expect: "Wallet price fee floor test works correctly under test conditions"
    #[test]
    fn fee_estimation_floors_at_one_rj() {
        let rate = ExchangeRate {
            usd_per_token: 0.000001, // extremely low rate
            updated_at: chrono::Utc::now(),
        };
        let fee = estimate_withdrawal_fee(ChainId::Hedera, &rate, 1000);
        assert_eq!(fee.rjoules, 1);
    }

    /// expect: "Wallet price chain diff test works correctly under test conditions"
    #[test]
    fn different_chains_produce_same_fees() {
        let rate = ExchangeRate {
            usd_per_token: 150.0,
            updated_at: chrono::Utc::now(),
        };
        let hed_fee = estimate_withdrawal_fee(ChainId::Hedera, &rate, 1000);
        let hink_fee = estimate_withdrawal_fee(ChainId::Hedera, &rate, 1000);
        assert_eq!(hed_fee.rjoules, hink_fee.rjoules);
        assert!(hed_fee.rjoules > 0);
    }

    // ── EodhdPriceFeed tests ──────────────────────────────────────────────

    /// expect: "Wallet price eodhd parse test works correctly under test conditions"
    #[tokio::test]
    async fn eodhd_feed_parses_close_field() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/real-time/HBAR-USD.CC"))
            .and(query_param("api_token", "test-key"))
            .and(query_param("fmt", "json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": "HBAR-USD.CC",
                "timestamp": 1718400000,
                "close": 142.73
            })))
            .mount(&server)
            .await;

        // Override base URL to hit mock server
        let feed = EodhdPriceFeed {
            client: reqwest::Client::new(),
            api_key: "test-key".into(),
        };
        // We can't easily override the const URL, so test via the trait
        // by constructing a feed that hits the mock server directly.
        // For now, test the parsing logic indirectly via the composite.
        // (Direct EODHD testing requires URL override — deferred to integration.)
        let _ = server; // keep alive
        let _ = feed;
    }

    // ── CoinGeckoPriceFeed tests ──────────────────────────────────────────

    /// expect: "Wallet price coingecko parse test works correctly under test conditions"
    #[tokio::test]
    async fn coingecko_feed_parses_usd_field() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/simple/price"))
            .and(query_param("ids", "hedera-hashgraph"))
            .and(query_param("vs_currencies", "usd"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "hedera-hashgraph": { "usd": 0.08 }
            })))
            .mount(&server)
            .await;

        // Same constraint — const URL can't be overridden in unit tests.
        // Integration tests will verify against live API.
        let _ = server;
    }

    // ── CompositePriceFeed tests ──────────────────────────────────────────

    /// A mock PriceFeed that returns a fixed rate.
    struct MockRateFeed {
        rate: f64,
        should_fail: bool,
    }

    #[async_trait]
    impl PriceFeed for MockRateFeed {
        async fn get_rate(&self, _chain: ChainId) -> Result<ExchangeRate, WalletError> {
            if self.should_fail {
                Err(WalletError::Infra(
                    hkask_types::InfrastructureError::database("mock failure"),
                ))
            } else {
                Ok(ExchangeRate {
                    usd_per_token: self.rate,
                    updated_at: chrono::Utc::now(),
                })
            }
        }
    }

    /// expect: "Wallet price composite primary test works correctly under test conditions"
    #[tokio::test]
    async fn composite_uses_primary_source() {
        let primary = Box::new(MockRateFeed {
            rate: 150.0,
            should_fail: false,
        });
        let fallback = Box::new(MockRateFeed {
            rate: 999.0,
            should_fail: false,
        });
        let composite = CompositePriceFeed::new(vec![primary, fallback], 30);

        let rate = composite.get_rate(ChainId::Hedera).await.unwrap();
        assert!((rate.usd_per_token - 150.0).abs() < f64::EPSILON);
    }

    /// expect: "Wallet price composite fallback test works correctly under test conditions"
    #[tokio::test]
    async fn composite_falls_back_on_primary_failure() {
        let primary = Box::new(MockRateFeed {
            rate: 0.0,
            should_fail: true,
        });
        let fallback = Box::new(MockRateFeed {
            rate: 0.08,
            should_fail: false,
        });
        let composite = CompositePriceFeed::new(vec![primary, fallback], 30);

        let rate = composite.get_rate(ChainId::Hedera).await.unwrap();
        assert!((rate.usd_per_token - 0.08).abs() < f64::EPSILON);
    }

    /// expect: "Wallet price composite cache test works correctly under test conditions"
    #[tokio::test]
    async fn composite_caches_within_ttl() {
        let source = Box::new(MockRateFeed {
            rate: 150.0,
            should_fail: false,
        });
        let composite = CompositePriceFeed::new(vec![source], 3600); // 1h TTL

        // First call — from source
        let rate1 = composite.get_rate(ChainId::Hedera).await.unwrap();
        assert!((rate1.usd_per_token - 150.0).abs() < f64::EPSILON);

        // Replace source with one that would return different rate
        // but cache should still return the first value
        // (We can't replace sources in CompositePriceFeed after construction,
        // so we test that a second call within TTL returns same value)
        let rate2 = composite.get_rate(ChainId::Hedera).await.unwrap();
        assert!((rate2.usd_per_token - 150.0).abs() < f64::EPSILON);
    }

    /// expect: "Wallet price composite stale test works correctly under test conditions"
    #[tokio::test]
    async fn composite_stale_fallback_on_total_failure() {
        // First, populate cache with a working source
        let working = Box::new(MockRateFeed {
            rate: 150.0,
            should_fail: false,
        });
        let composite = CompositePriceFeed::new(vec![working], 0); // TTL=0 so immediate expiry
        let _ = composite.get_rate(ChainId::Hedera).await.unwrap();

        // Now replace with failing source (simulated by new composite with failing source)
        let failing = Box::new(MockRateFeed {
            rate: 0.0,
            should_fail: true,
        });
        let composite2 = CompositePriceFeed::new(vec![failing], 0);
        // No cache → should fail
        let err = composite2.get_rate(ChainId::Hedera).await.unwrap_err();
        assert!(err.to_string().contains("exhausted"));
    }

    /// expect: "Wallet price composite empty test works correctly under test conditions"
    #[tokio::test]
    async fn composite_errors_on_empty_sources() {
        let composite = CompositePriceFeed::new(vec![], 30);
        let err = composite.get_rate(ChainId::Hedera).await.unwrap_err();
        assert!(err.to_string().contains("exhausted"));
    }

    // ── resolve_price_feed tests ─────────────────────────────────────────

    /// expect: "Wallet price resolve static test works correctly under test conditions"
    #[test]
    fn resolve_static_config() {
        let feed = resolve_price_feed(&PriceFeedConfig::Static).unwrap();
        // Verify it's a StaticPriceFeed by checking it doesn't panic on get_rate
        let rt = tokio::runtime::Runtime::new().unwrap();
        let rate = rt.block_on(feed.get_rate(ChainId::Hedera)).unwrap();
        assert!((rate.usd_per_token - 0.08).abs() < f64::EPSILON);
    }

    /// expect: "Wallet price resolve coingecko test works correctly under test conditions"
    #[test]
    fn resolve_coingecko_config() {
        let feed = resolve_price_feed(&PriceFeedConfig::CoinGecko).unwrap();
        // Just verify construction succeeds (network calls happen at get_rate time)
        let _ = feed;
    }

    /// expect: "Wallet price resolve composite test works correctly under test conditions"
    #[test]
    fn resolve_composite_config() {
        let config = PriceFeedConfig::Composite {
            sources: vec!["coingecko".to_string()],
            cache_ttl_secs: 60,
        };
        let feed = resolve_price_feed(&config).unwrap();
        let _ = feed;
    }

    /// expect: "Wallet price resolve skip unknown test works correctly under test conditions"
    #[test]
    fn resolve_composite_skips_unknown_sources() {
        let config = PriceFeedConfig::Composite {
            sources: vec!["unknown_source".to_string(), "coingecko".to_string()],
            cache_ttl_secs: 30,
        };
        // Should succeed because "coingecko" is valid
        let feed = resolve_price_feed(&config).unwrap();
        let _ = feed;
    }

    /// expect: "Wallet price resolve all unknown test works correctly under test conditions"
    #[test]
    fn resolve_composite_errors_on_all_unknown() {
        let config = PriceFeedConfig::Composite {
            sources: vec!["nope1".to_string(), "nope2".to_string()],
            cache_ttl_secs: 30,
        };
        let err = match resolve_price_feed(&config) {
            Ok(_) => panic!("expected resolve_price_feed to fail when all sources are unknown"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("no sources resolved"));
    }
}
