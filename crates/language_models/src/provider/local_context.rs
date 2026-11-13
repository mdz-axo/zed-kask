//! zed-kask: D107 — the default context budget for models served on this
//! machine.
//!
//! A discovered local model reports the context length it was trained with
//! (`qwen3:1.7b` reports 40,960). Using that as the request's context made a
//! CPU-only 15 GiB laptop evaluate a prompt it could not finish in useful
//! time. Discovery is therefore capped by a tier keyed on this machine's
//! memory. An explicit `context_window` or per-model `max_tokens` setting is
//! never capped.

use http_client::{Host, Url};
use std::sync::LazyLock;
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

const GIB: u64 = 1024 * 1024 * 1024;

/// Ollama's own default context tiers and thresholds (`server/routes.go`,
/// "vram-based default context": 4k, 32k at 23 GiB, 256k at 47 GiB). Ollama
/// keys them on GPU memory; this keys them on system memory, which is what a
/// CPU-only machine serves the model from.
pub(crate) fn context_tier_for_memory(total_memory_bytes: u64) -> u64 {
    if total_memory_bytes >= 47 * GIB {
        262_144
    } else if total_memory_bytes >= 23 * GIB {
        32_768
    } else {
        4_096
    }
}

/// The cap for a discovered model's context window, or `None` when the
/// server is not on this machine: this machine's memory says nothing about
/// a remote server's. An empty `api_url` is the provider's default, which is
/// local.
pub(crate) fn discovered_context_cap(api_url: &str) -> Option<u64> {
    (api_url.is_empty() || is_loopback_api_url(api_url))
        .then(|| context_tier_for_memory(total_system_memory()))
}

fn is_loopback_api_url(api_url: &str) -> bool {
    let Ok(url) = Url::parse(api_url) else {
        return false;
    };
    match url.host() {
        Some(Host::Domain(domain)) => {
            let domain = domain.to_ascii_lowercase();
            domain == "localhost" || domain.ends_with(".localhost")
        }
        Some(Host::Ipv4(address)) => address.is_loopback() || address.is_unspecified(),
        Some(Host::Ipv6(address)) => address.is_loopback() || address.is_unspecified(),
        None => false,
    }
}

/// Total system memory in bytes. Unit tests see the top tier so provider
/// tests do not depend on the machine that runs them; the tiers themselves
/// are tested through [`context_tier_for_memory`].
fn total_system_memory() -> u64 {
    if cfg!(test) {
        return u64::MAX;
    }
    static TOTAL_MEMORY: LazyLock<u64> = LazyLock::new(|| {
        System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
        )
        .total_memory()
    });
    *TOTAL_MEMORY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_tiers_follow_ollamas_thresholds() {
        // The reported machine: 15 GiB, CPU-only.
        assert_eq!(context_tier_for_memory(15 * GIB), 4_096);
        assert_eq!(context_tier_for_memory(23 * GIB - 1), 4_096);
        assert_eq!(context_tier_for_memory(23 * GIB), 32_768);
        assert_eq!(context_tier_for_memory(47 * GIB - 1), 32_768);
        assert_eq!(context_tier_for_memory(47 * GIB), 262_144);
        assert_eq!(context_tier_for_memory(0), 4_096);
    }

    #[test]
    fn only_a_server_on_this_machine_is_capped() {
        for local in [
            "",
            "http://localhost:11434",
            "http://LOCALHOST:8080",
            "http://127.0.0.1:11434",
            "http://127.0.0.2:8080/",
            "http://[::1]:11434",
            "http://0.0.0.0:11434",
            "http://ollama.localhost:11434",
        ] {
            assert!(
                discovered_context_cap(local).is_some(),
                "{local:?} is served from this machine"
            );
        }
        for remote in [
            "https://ollama.com",
            "http://192.168.1.20:11434",
            "http://notlocalhost:11434",
            "http://localhost.example.com:11434",
            "not a url",
        ] {
            assert!(
                discovered_context_cap(remote).is_none(),
                "{remote:?} is not known to be this machine"
            );
        }
    }
}
