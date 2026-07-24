//! Provider Intelligence — real-time provider cost and usage tracking.
//!
//! Provides the `ProviderIntelligence` trait for discovering current tier,
//! billing-period usage, and actual per-unit costs from provider APIs.
//!
//! Self-tracked providers (Brave, Tavily, Exa, FMP, EODHD) use
//! the hkask-ledger for call-count tracking when the provider has no usage API.
//! Call count is measured as the number of committed transactions, not balance.

use chrono::Datelike;
use hkask_storage::database::driver::DatabaseDriver;
use serde::Deserialize;
use std::sync::Arc;

// ── Error types ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error: {0}")]
    Api(String),
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("usage API not available for this provider")]
    NoUsageApi,
    #[error("ledger error: {0}")]
    Ledger(#[from] hkask_ledger::LedgerError),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::Http(format!("{e}"))
    }
}

// ── Shared types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitUnit {
    Tokens,
    Calls,
    Credits,
    Dollars,
}

#[derive(Debug, Clone)]
pub struct ProviderState {
    pub tier: String,
    pub monthly_limit: Option<u64>,
    pub limit_unit: LimitUnit,
    pub overage_rate: Option<CostRate>,
    pub billing_period_start: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct UsageStatus {
    pub consumed: u64,
    pub limit: u64,
    pub fraction: f64,
    pub estimated_exhaustion: Option<chrono::DateTime<chrono::Utc>>,
}

/// Per-unit cost rate, in nano-rJoules (nJ). 1 nJ = 0.001 µrJ.
///
/// Token-based providers charge per-token with optional cache discounts.
/// Call-based providers use `fixed_nj_per_call`.
/// Multi-dimensional providers (OpenRouter) use the additional fields.
#[derive(Debug, Clone)]
pub struct CostRate {
    /// Standard (uncached) input token cost in nJ per token.
    pub input_nj_per_unit: u64,
    /// Output/completion token cost in nJ per token.
    pub output_nj_per_unit: u64,
    /// Cached input token READ cost in nJ per token (0 = no cache discount).
    pub cache_read_nj_per_unit: u64,
    /// Cached input token WRITE cost in nJ per token (0 = no charge).
    pub cache_write_nj_per_unit: u64,
    /// Fixed per-call cost in nJ (for non-token-based providers).
    pub fixed_nj_per_call: u64,
    /// Per-image cost in nJ (for vision models).
    pub image_nj_per_unit: u64,
    /// Whether this rate represents marginal/overage pricing.
    pub is_marginal: bool,
}

// ── Trait ───────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait ProviderIntelligence: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn discover(&self, api_key: &str) -> Result<ProviderState, ProviderError>;
    async fn usage(&self, api_key: &str) -> Result<UsageStatus, ProviderError>;
    /// Get the actual per-unit cost for the given model. `model_name` is the
    /// full model identifier (e.g., "meta-llama/Llama-3.3-70B-Instruct").
    /// Providers with per-model pricing use it; flat-rate providers ignore it.
    async fn actual_cost(&self, api_key: &str, model_name: &str)
    -> Result<CostRate, ProviderError>;
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

fn default_billing_start() -> chrono::DateTime<chrono::Utc> {
    let now = chrono::Utc::now();
    chrono::DateTime::<chrono::Utc>::from_timestamp(
        chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|dt| dt.and_utc().timestamp())
            .unwrap_or(0),
        0,
    )
    .unwrap_or(now)
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    url: &str,
    api_key: &str,
) -> Result<T, ProviderError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ProviderError::Api(format!("{status}: {body}")));
    }
    Ok(resp.json().await?)
}

fn usage_status(consumed: u64, limit: u64) -> (f64, Option<chrono::DateTime<chrono::Utc>>) {
    if limit == 0 || limit == u64::MAX {
        return (0.0, None);
    }
    let fraction = consumed as f64 / limit as f64;
    let exhaustion = if fraction > 0.0 {
        let remaining = limit.saturating_sub(consumed);
        let days_into = chrono::Utc::now()
            .signed_duration_since(default_billing_start())
            .num_days()
            .max(1) as f64;
        let daily_rate = consumed as f64 / days_into;
        if daily_rate > 0.0 {
            let days_left = remaining as f64 / daily_rate;
            Some(chrono::Utc::now() + chrono::Duration::hours((days_left * 24.0) as i64))
        } else {
            None
        }
    } else {
        None
    };
    (fraction, exhaustion)
}

// ── DeepInfra ───────────────────────────────────────────────────────────────────
// API: GET /payment/usage?from=YYYY.MM
// Response: { months: [{ period, total_cost (cents), items: [{ units, rate, cost }] }] }

pub struct DeepInfraProvider;

impl DeepInfraProvider {
    pub const INPUT_NJ_PER_TOKEN: u64 = 30;
    pub const OUTPUT_NJ_PER_TOKEN: u64 = 60;
}

#[async_trait::async_trait]
impl ProviderIntelligence for DeepInfraProvider {
    fn provider_id(&self) -> &'static str {
        "deepinfra"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        Ok(ProviderState {
            tier: "pay-as-you-go".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Tokens,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, api_key: &str) -> Result<UsageStatus, ProviderError> {
        let now = chrono::Utc::now();
        let from = format!("{}.{:02}", now.year(), now.month());
        let url = format!("https://api.deepinfra.com/payment/usage?from={from}");

        #[derive(Deserialize)]
        struct UsageOut {
            months: Vec<UsageMonth>,
        }
        #[derive(Deserialize)]
        struct UsageMonth {
            total_cost: Option<u64>,
        }

        match fetch_json::<UsageOut>(&url, api_key).await {
            Ok(data) => {
                let consumed = data.months.iter().map(|m| m.total_cost.unwrap_or(0)).sum();
                Ok(UsageStatus {
                    consumed,
                    limit: u64::MAX,
                    fraction: 0.0,
                    estimated_exhaustion: None,
                })
            }
            Err(_) => {
                // Fallback: usage API may not be available for all keys
                Ok(UsageStatus {
                    consumed: 0,
                    limit: u64::MAX,
                    fraction: 0.0,
                    estimated_exhaustion: None,
                })
            }
        }
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        Ok(CostRate {
            input_nj_per_unit: Self::INPUT_NJ_PER_TOKEN,
            output_nj_per_unit: Self::OUTPUT_NJ_PER_TOKEN,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: 0,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── OpenRouter ──────────────────────────────────────────────────────────────────
// API: GET /api/v1/key
// Response: { data: { label, limit, limit_remaining, limit_reset, usage, usage_monthly, ... } }

pub struct OpenRouterProvider;

impl OpenRouterProvider {
    pub const INPUT_NJ_PER_TOKEN: u64 = 50;
    pub const OUTPUT_NJ_PER_TOKEN: u64 = 50;
}

#[async_trait::async_trait]
impl ProviderIntelligence for OpenRouterProvider {
    fn provider_id(&self) -> &'static str {
        "openrouter"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        Ok(ProviderState {
            tier: "credit-based".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Credits,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, api_key: &str) -> Result<UsageStatus, ProviderError> {
        #[derive(Deserialize)]
        struct KeyResp {
            data: KeyData,
        }
        #[derive(Deserialize)]
        struct KeyData {
            #[serde(default)]
            usage: f64,
            #[serde(default)]
            limit: Option<f64>,
        }

        let info: KeyResp = fetch_json("https://openrouter.ai/api/v1/key", api_key).await?;
        let usage = info.data.usage;
        let limit = info.data.limit.unwrap_or(0.0);
        let consumed = (usage * 100.0) as u64;
        let limit_cents = if limit > 0.0 {
            (limit * 100.0) as u64
        } else {
            u64::MAX
        };
        let (fraction, exhaustion) = usage_status(consumed, limit_cents);

        Ok(UsageStatus {
            consumed,
            limit: limit_cents,
            fraction,
            estimated_exhaustion: exhaustion,
        })
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        // OpenRouter pricing is model-specific — the classify_batch caller
        // should use the model's pricing from the /models API or config.
        // These are conservative fallback defaults.
        Ok(CostRate {
            input_nj_per_unit: Self::INPUT_NJ_PER_TOKEN,
            output_nj_per_unit: Self::OUTPUT_NJ_PER_TOKEN,
            cache_read_nj_per_unit: 10, // typical discounted cache read
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: 0,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── Together AI ─────────────────────────────────────────────────────────────────
// Together AI is fully prepaid — no free credits tier.
// API: GET /v1/billing/usage → array of { date, model, input_tokens, output_tokens, total_cost }

pub struct TogetherProvider;

impl TogetherProvider {
    pub const INPUT_NJ_PER_TOKEN: u64 = 20;
    pub const OUTPUT_NJ_PER_TOKEN: u64 = 20;
}

#[async_trait::async_trait]
impl ProviderIntelligence for TogetherProvider {
    fn provider_id(&self) -> &'static str {
        "together"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        // Together is fully prepaid — always pay-as-you-go, no free tier
        Ok(ProviderState {
            tier: "prepaid".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Tokens,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, api_key: &str) -> Result<UsageStatus, ProviderError> {
        #[derive(Deserialize)]
        #[allow(dead_code)] // populated by serde deserialization
        struct UsageEntry {
            #[serde(default)]
            input_tokens: u64,
            #[serde(default)]
            output_tokens: u64,
            #[serde(default)]
            total_cost: Option<f64>,
        }

        match fetch_json::<Vec<UsageEntry>>("https://api.together.xyz/v1/billing/usage", api_key)
            .await
        {
            Ok(entries) => {
                let total_tokens: u64 = entries
                    .iter()
                    .map(|e| e.input_tokens + e.output_tokens)
                    .sum();
                Ok(UsageStatus {
                    consumed: total_tokens,
                    limit: u64::MAX,
                    fraction: 0.0,
                    estimated_exhaustion: None,
                })
            }
            Err(_) => Ok(UsageStatus {
                consumed: 0,
                limit: u64::MAX,
                fraction: 0.0,
                estimated_exhaustion: None,
            }),
        }
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        // Always marginal — prepaid credits consumed at per-token rate
        Ok(CostRate {
            input_nj_per_unit: Self::INPUT_NJ_PER_TOKEN,
            output_nj_per_unit: Self::OUTPUT_NJ_PER_TOKEN,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: 0,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── fal.ai ──────────────────────────────────────────────────────────────────────

/// fal.ai provider — pay-as-you-go, image/video/LLM inference.
/// Always marginal.
pub struct FalProvider;

impl FalProvider {
    pub const INPUT_NJ_PER_TOKEN: u64 = 40;
    pub const OUTPUT_NJ_PER_TOKEN: u64 = 40;
}

#[async_trait::async_trait]
impl ProviderIntelligence for FalProvider {
    fn provider_id(&self) -> &'static str {
        "fal"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        Ok(ProviderState {
            tier: "pay-as-you-go".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Tokens,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, _api_key: &str) -> Result<UsageStatus, ProviderError> {
        // fal.ai does not expose a public usage API
        Ok(UsageStatus {
            consumed: 0,
            limit: u64::MAX,
            fraction: 0.0,
            estimated_exhaustion: None,
        })
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        Ok(CostRate {
            input_nj_per_unit: Self::INPUT_NJ_PER_TOKEN,
            output_nj_per_unit: Self::OUTPUT_NJ_PER_TOKEN,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: 0,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── Self-Tracked Provider ───────────────────────────────────────────────────────
// Uses the cost ledger to count committed transactions per provider.
// Call count = number of transactions referencing cost:api/<provider_id>.
// NOT balance — balance is µrJ, not call count.

/// Configuration for a self-tracked provider (no usage API).
pub struct SelfTrackedConfig {
    pub id: &'static str,
    pub name: &'static str,
    /// Tier-based call limits. Each (tier_name, monthly_call_limit).
    pub tiers: &'static [(&'static str, Option<u64>)],
    /// Overage rate per call in nJ when tier limit is exceeded.
    pub overage_nj_per_call: u64,
    /// Current tier index (into `tiers`).
    pub current_tier: usize,
}

pub struct SelfTrackedProvider {
    config: SelfTrackedConfig,
    driver: Arc<dyn DatabaseDriver>,
}

impl SelfTrackedProvider {
    pub fn new(config: SelfTrackedConfig, driver: Arc<dyn DatabaseDriver>) -> Self {
        Self { config, driver }
    }

    /// Count transactions referencing this provider's cost account.
    fn ledger_call_count(&self) -> u64 {
        let account = format!("cost:api/{}", self.config.id);
        match hkask_ledger::Ledger::from_driver(self.driver.clone()) {
            Ok(ledger) => ledger.transaction_count(&account).unwrap_or(0),
            Err(e) => {
                tracing::warn!(
                    target: "hkask.provider",
                    provider = %self.config.id,
                    error = %e,
                    "Failed to open ledger for call count — returning 0"
                );
                0
            }
        }
    }

    fn tier_limit(&self) -> Option<u64> {
        self.config
            .tiers
            .get(self.config.current_tier)
            .and_then(|t| t.1)
    }
}

#[async_trait::async_trait]
impl ProviderIntelligence for SelfTrackedProvider {
    fn provider_id(&self) -> &'static str {
        self.config.id
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        let tier_name = self
            .config
            .tiers
            .get(self.config.current_tier)
            .map(|t| t.0)
            .unwrap_or("unknown");
        Ok(ProviderState {
            tier: tier_name.into(),
            monthly_limit: self.tier_limit(),
            limit_unit: LimitUnit::Calls,
            overage_rate: Some(CostRate {
                input_nj_per_unit: 0,
                output_nj_per_unit: 0,
                cache_read_nj_per_unit: 0,
                cache_write_nj_per_unit: 0,
                fixed_nj_per_call: self.config.overage_nj_per_call,
                image_nj_per_unit: 0,
                is_marginal: true,
            }),
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, _api_key: &str) -> Result<UsageStatus, ProviderError> {
        let consumed = self.ledger_call_count();
        let limit = self.tier_limit().unwrap_or(u64::MAX);
        let (fraction, exhaustion) = usage_status(consumed, limit);
        Ok(UsageStatus {
            consumed,
            limit,
            fraction,
            estimated_exhaustion: exhaustion,
        })
    }

    async fn actual_cost(
        &self,
        api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        let usage = self.usage(api_key).await?;
        // Providers with no tier cap (limit == u64::MAX) are always-marginal.
        let has_cap = usage.limit > 0 && usage.limit != u64::MAX;
        // Subscription tiers give "up to X calls" — overage starts at X+1.
        let over_limit = has_cap && usage.consumed > usage.limit;
        Ok(CostRate {
            input_nj_per_unit: 0,
            output_nj_per_unit: 0,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: if over_limit {
                self.config.overage_nj_per_call
            } else {
                0
            },
            image_nj_per_unit: 0,
            is_marginal: !has_cap || over_limit,
        })
    }
}

// ── Firecrawl ───────────────────────────────────────────────────────────────────

pub struct FirecrawlProvider {
    driver: Arc<dyn DatabaseDriver>,
}

impl FirecrawlProvider {
    pub fn new(driver: Arc<dyn DatabaseDriver>) -> Self {
        Self { driver }
    }
}

#[async_trait::async_trait]
impl ProviderIntelligence for FirecrawlProvider {
    fn provider_id(&self) -> &'static str {
        "firecrawl"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        Ok(ProviderState {
            tier: "credit-based".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Credits,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, api_key: &str) -> Result<UsageStatus, ProviderError> {
        #[derive(Deserialize)]
        struct AccountResp {
            credits_used: Option<u64>,
        }
        match fetch_json::<AccountResp>("https://api.firecrawl.dev/v1/account", api_key).await {
            Ok(data) => {
                let consumed = data.credits_used.unwrap_or(0);
                Ok(UsageStatus {
                    consumed,
                    limit: u64::MAX,
                    fraction: 0.0,
                    estimated_exhaustion: None,
                })
            }
            Err(_) => {
                // Fallback: count ledger transactions
                let consumed = match hkask_ledger::Ledger::from_driver(self.driver.clone()) {
                    Ok(ledger) => ledger.transaction_count("cost:api/firecrawl").unwrap_or(0),
                    Err(_) => 0,
                };
                Ok(UsageStatus {
                    consumed,
                    limit: u64::MAX,
                    fraction: 0.0,
                    estimated_exhaustion: None,
                })
            }
        }
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        Ok(CostRate {
            input_nj_per_unit: 0,
            output_nj_per_unit: 0,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: 0,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── RunPod (GPU) ────────────────────────────────────────────────────────────────

pub struct RunpodProvider;

impl RunpodProvider {
    pub const COST_NJ_PER_SECOND: u64 = 100_000; // ~$0.0001/sec
}

#[async_trait::async_trait]
impl ProviderIntelligence for RunpodProvider {
    fn provider_id(&self) -> &'static str {
        "runpod"
    }

    async fn discover(&self, _api_key: &str) -> Result<ProviderState, ProviderError> {
        Ok(ProviderState {
            tier: "pay-as-you-go".into(),
            monthly_limit: None,
            limit_unit: LimitUnit::Dollars,
            overage_rate: None,
            billing_period_start: default_billing_start(),
        })
    }

    async fn usage(&self, _api_key: &str) -> Result<UsageStatus, ProviderError> {
        Ok(UsageStatus {
            consumed: 0,
            limit: u64::MAX,
            fraction: 0.0,
            estimated_exhaustion: None,
        })
    }

    async fn actual_cost(
        &self,
        _api_key: &str,
        _model_name: &str,
    ) -> Result<CostRate, ProviderError> {
        Ok(CostRate {
            input_nj_per_unit: 0,
            output_nj_per_unit: 0,
            cache_read_nj_per_unit: 0,
            cache_write_nj_per_unit: 0,
            fixed_nj_per_call: Self::COST_NJ_PER_SECOND,
            image_nj_per_unit: 0,
            is_marginal: true,
        })
    }
}

// ── Provider factory ────────────────────────────────────────────────────────────

/// REQ: P7-provider-factory
/// expect: "I can create a provider by name and it wires up correctly" \[P7\]
/// pre:  provider_id is a known provider string; driver required for self-tracked
/// post: returns Some(provider) for known IDs, None for unknown
/// \[P7\] Constraining: Composition — providers are composable by ID
pub fn create_provider(
    provider_id: &str,
    driver: Option<Arc<dyn DatabaseDriver>>,
) -> Option<Box<dyn ProviderIntelligence>> {
    match provider_id.to_lowercase().as_str() {
        "deepinfra" => Some(Box::new(DeepInfraProvider)),
        "openrouter" => Some(Box::new(OpenRouterProvider)),
        "together" => Some(Box::new(TogetherProvider)),
        "fal" => Some(Box::new(FalProvider)),
        "brave" => driver.map(|d| {
            Box::new(SelfTrackedProvider::new(
                SelfTrackedConfig {
                    id: "brave",
                    name: "Brave Search",
                    tiers: &[("free", Some(2000)), ("base", Some(20000)), ("pro", None)],
                    overage_nj_per_call: 1_000_000,
                    current_tier: 0,
                },
                d,
            )) as Box<dyn ProviderIntelligence>
        }),
        "tavily" => driver.map(|d| {
            Box::new(SelfTrackedProvider::new(
                SelfTrackedConfig {
                    id: "tavily",
                    name: "Tavily",
                    tiers: &[("basic", Some(1000)), ("pro", None)],
                    overage_nj_per_call: 800_000,
                    current_tier: 0,
                },
                d,
            )) as Box<dyn ProviderIntelligence>
        }),
        "exa" => driver.map(|d| {
            Box::new(SelfTrackedProvider::new(
                SelfTrackedConfig {
                    id: "exa",
                    name: "Exa",
                    tiers: &[("basic", Some(1000)), ("pro", None)],
                    overage_nj_per_call: 800_000,
                    current_tier: 0,
                },
                d,
            )) as Box<dyn ProviderIntelligence>
        }),
        "fmp" => driver.map(|d| {
            Box::new(SelfTrackedProvider::new(
                SelfTrackedConfig {
                    id: "fmp",
                    name: "FMP",
                    tiers: &[("basic", Some(500)), ("pro", None)],
                    overage_nj_per_call: 2_000_000,
                    current_tier: 0,
                },
                d,
            )) as Box<dyn ProviderIntelligence>
        }),
        "eodhd" => driver.map(|d| {
            Box::new(SelfTrackedProvider::new(
                SelfTrackedConfig {
                    id: "eodhd",
                    name: "EODHD",
                    tiers: &[("basic", Some(500)), ("pro", None)],
                    overage_nj_per_call: 2_000_000,
                    current_tier: 0,
                },
                d,
            )) as Box<dyn ProviderIntelligence>
        }),
        "firecrawl" => {
            driver.map(|d| Box::new(FirecrawlProvider::new(d)) as Box<dyn ProviderIntelligence>)
        }
        "runpod" => Some(Box::new(RunpodProvider)),
        _ => None,
    }
}
