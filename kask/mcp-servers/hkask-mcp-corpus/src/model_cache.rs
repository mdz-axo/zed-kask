//! Model-list TTL cache — lazy, process-scoped, manually refreshable.
//!
//! # Policy (per product spec)
//!
//! - **Lazy population:** the first `list_models`/`search_models` call fetches
//!   live (this is the "update on start-up" — the first time the user asks, or
//!   the API/TUI probes, the list is built). No proactive fetch at process start,
//!   so a cold cloud provider never blocks launch.
//! - **TTL:** cached for a few hours (default 4h, `HKASK_MODEL_CACHE_TTL_SECS`).
//!   Within the TTL, calls return the cached list — no per-call re-fetch of
//!   Ollama `/v1/models` or cloud `/v1/models`.
//! - **Lazy refresh:** after the TTL expires, the *next* call refetches (on
//!   demand, not a background timer).
//! - **Manual refresh:** `ModelCache::invalidate()` clears the entry so the next
//!   call fetches immediately. Wired to a `/model refresh` REPL command.
//!
//! # Concurrency
//!
//! The lock is a `std::sync::Mutex` held only for the cache check and the store
//! — never across the async fetch. A cold-start race (two concurrent misses both
//! fetching) is accepted: the fetch is idempotent and the last writer wins.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::inference_svc::{InferenceContext, ModelInfo};
use hkask_inference::InferenceRouter;
use hkask_services_core::ServiceError;

/// Default cache time-to-live: 4 hours.
const DEFAULT_TTL_SECS: u64 = 4 * 60 * 60;

struct CacheState {
    entries: Option<Vec<ModelInfo>>,
    fetched_at: Option<Instant>,
    ttl: Duration,
}

impl Default for CacheState {
    fn default() -> Self {
        let ttl = Duration::from_secs(ttl_from_env());
        Self {
            entries: None,
            fetched_at: None,
            ttl,
        }
    }
}

fn ttl_from_env() -> u64 {
    std::env::var("HKASK_MODEL_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s: &u64| s > 0)
        .unwrap_or(DEFAULT_TTL_SECS)
}

/// Access the process-scoped cache cell (lazily initialized on first use).
fn cache() -> &'static Mutex<CacheState> {
    static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CacheState::default()))
}

/// Lock the cache, recovering the guard if a prior holder panicked (poison).
///
/// A poisoned mutex means some thread panicked while holding the lock. The
/// underlying data is still accessible; for a TTL cache the worst case is a
/// stale read, which the next miss overwrites. Recovering the guard (rather
/// than panicking) keeps the daemon alive across an unrelated thread panic.
/// See the eliminate-nested-runtime-panics discipline (ADR-043).
fn lock_cache() -> std::sync::MutexGuard<'static, CacheState> {
    cache().lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Process-scoped model-list cache.
pub struct ModelCache;

impl ModelCache {
    /// Return the cached model list if fresh; otherwise fetch live, store, and
    /// return. The async fetch happens outside the mutex.
    ///
    /// expect: "I can discover available models across providers without re-fetching on every call"
    /// pre:  ctx.inference_config is valid
    /// post: returns the model list (cached if within TTL, freshly fetched otherwise)
    pub async fn list_models(ctx: &InferenceContext) -> Result<Vec<ModelInfo>, ServiceError> {
        // Cache check — hold the lock only for the read.
        let now = Instant::now();
        let cached = {
            let state = lock_cache();
            if let Some(ref entries) = state.entries
                && let Some(at) = state.fetched_at
                && now.duration_since(at) < state.ttl
            {
                Some(entries.clone())
            } else {
                None
            }
        };
        if let Some(entries) = cached {
            return Ok(entries);
        }

        // Miss — fetch live. Lock is NOT held across this await.
        let router = InferenceRouter::new(ctx.inference_config.clone());
        let models: Vec<ModelInfo> = router
            .list_models()
            .await
            .into_iter()
            .map(ModelInfo::from)
            .collect();

        // Store under the lock (last writer wins on a cold-start race).
        {
            let mut state = lock_cache();
            state.entries = Some(models.clone());
            state.fetched_at = Some(now);
        }
        Ok(models)
    }

    /// Force the next `list_models` call to refetch. Idempotent.
    ///
    /// expect: "I can refresh the model list on demand"
    /// post: cache entries cleared; next call fetches live
    pub fn invalidate() {
        let mut state = lock_cache();
        state.entries = None;
        state.fetched_at = None;
    }

    /// Whether the cache is empty or past its TTL (next call will fetch).
    #[must_use]
    pub fn is_stale() -> bool {
        let state = lock_cache();
        match (state.entries.is_some(), state.fetched_at) {
            (false, _) => true,
            (true, None) => true,
            (true, Some(at)) => Instant::now().duration_since(at) >= state.ttl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uncached_config() -> hkask_inference::InferenceConfig {
        // Empty Ollama base URL + no cloud keys -> no backends construct ->
        // list_models returns empty. Guarantees a deterministic live fetch,
        // independent of whether a real Ollama daemon is running.
        hkask_inference::InferenceConfig {
            ollama_base_url: String::new(),
            ..hkask_inference::InferenceConfig::default()
        }
    }

    /// One self-contained lifecycle test — the cache is process-global, so a
    /// single test avoids parallel races on the shared cell.
    #[tokio::test]
    async fn cache_lifecycle_populate_hit_invalidate_refetch() {
        ModelCache::invalidate();
        let ctx = InferenceContext::from_parts(None, "x", uncached_config());

        // 1. Inject a fresh entry; list_models must return it WITHOUT fetching
        //    (a live fetch with the uncached config returns []).
        {
            let mut state = cache().lock().unwrap();
            state.entries = Some(vec![fake_model("cached-A")]);
            state.fetched_at = Some(Instant::now());
        }
        assert!(!ModelCache::is_stale());
        let hit = ModelCache::list_models(&ctx).await.unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].name, "cached-A");

        // 2. invalidate -> next call refetches live (empty, no providers).
        ModelCache::invalidate();
        assert!(ModelCache::is_stale());
        let refetched = ModelCache::list_models(&ctx).await.unwrap();
        assert!(
            refetched.is_empty(),
            "after invalidate, refetch hits live (empty)"
        );

        // 3. Populate from the (empty) live fetch; second call returns cached.
        let _ = ModelCache::list_models(&ctx).await.unwrap();
        let cached = ModelCache::list_models(&ctx).await.unwrap();
        assert!(cached.is_empty());

        // 4. Poison-recovery regression (ADR-043 family): a prior thread panic
        //    poisons the process-global mutex. `list_models` must recover the
        //    guard and return Ok, not panic. See ADR-043 / diagnose skill.
        ModelCache::invalidate();
        let poison_handle = std::thread::spawn(|| {
            let _guard = cache().lock().unwrap();
            panic!("intentional poison for regression test");
        });
        let _ = poison_handle.join();
        let recovered = ModelCache::list_models(&ctx).await;
        assert!(
            recovered.is_ok(),
            "list_models must recover from a poisoned mutex, got: {:?}",
            recovered
        );

        ModelCache::invalidate();
    }

    fn fake_model(name: &str) -> ModelInfo {
        ModelInfo {
            name: name.into(),
            provider: hkask_inference::ProviderId::Ollama,
            family: None,
            parameter_size: None,
            quantization_level: None,
            size_bytes: None,
        }
    }
}
