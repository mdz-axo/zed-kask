//! hKask Runtime Services — text classification and provider intelligence.

mod adaptive_monitor;
mod classify_impl;
mod provider_intel;

pub use adaptive_monitor::AdaptiveMonitor;
pub use classify_impl::{
    ClassifierConfig, TripleExtraction, classify_batch, extract_triples_batch,
    load_classifier_config, parse_triple_extraction,
};
pub use provider_intel::{
    CostRate, DeepInfraProvider, FalProvider, FirecrawlProvider, LimitUnit, OpenRouterProvider,
    ProviderError, ProviderIntelligence, ProviderState, RunpodProvider, SelfTrackedConfig,
    SelfTrackedProvider, TogetherProvider, UsageStatus, create_provider,
};
