//! REPL /model handler — model listing, switching, and fuzzy search

use hkask_services_inference::{InferenceContext, InferenceService};

/// Resolved model switch — resolved name + rendered detail.
///
/// Internal to `hkask-repl`: the REPL `/model` handler prints it, and the
/// TUI `SettingsBridge` (in `hkask-repl`'s `tui` module, behind the `tui` feature)
/// maps this to `crate::tui::ModelSwitchResult` at the trait boundary. Kept
/// local so the resolver compiles without the optional `tui` feature.
pub(crate) struct ResolvedModel {
    pub resolved_name: String,
    pub detail: String,
}

/// Resolve `name` against the model catalog and apply it to `state`.
/// Single match => switch the active model. Zero/err => store verbatim.
/// Multiple => leave model unchanged, return candidate list as detail.
///
/// `model_meta` is intentionally left untouched: the catalog does not expose
/// `context_length`, so fabricating one (the old `16384` magic number) would
/// corrupt the context-pressure loop. Real metadata population awaits a
/// provider fetch (REPL spec Phase 15) and is tracked separately.
pub(crate) fn resolve_and_set_model(
    state: &mut super::super::ReplState,
    rt: &tokio::runtime::Handle,
    name: &str,
) -> ResolvedModel {
    let ctx = InferenceContext::from(state.service_context.as_ref());
    match rt.block_on(InferenceService::search_models(&ctx, name)) {
        Ok(models) if models.len() == 1 => {
            let m = &models[0];
            state.current_model = m.name.clone();
            let mut detail = String::new();
            if let Some(ref family) = m.family {
                detail.push_str(&format!("Family: {}\n", family));
            }
            if let Some(ref params) = m.parameter_size {
                detail.push_str(&format!("Parameters: {}\n", params));
            }
            if let Some(ref quant) = m.quantization_level {
                detail.push_str(&format!("Quantization: {}\n", quant));
            }
            ResolvedModel {
                resolved_name: state.current_model.clone(),
                detail,
            }
        }
        Ok(models) if models.is_empty() => {
            state.current_model = name.to_string();
            ResolvedModel {
                resolved_name: name.to_string(),
                detail: "Provider unreachable — model name stored for next inference.".to_string(),
            }
        }
        Ok(models) => {
            let mut d = format!("Multiple matches ({}):\n", models.len());
            for m in &models {
                d.push_str(&format!("  {}\n", m.name));
            }
            d.push_str("Use /model <exact-name> to switch.");
            ResolvedModel {
                resolved_name: state.current_model.clone(),
                detail: d,
            }
        }
        Err(_) => {
            state.current_model = name.to_string();
            ResolvedModel {
                resolved_name: name.to_string(),
                detail: "Provider unreachable — model name stored for next inference.".to_string(),
            }
        }
    }
}

pub fn handle_model(arg1: &str, rt: &tokio::runtime::Handle, state: &mut super::super::ReplState) {
    if arg1.eq_ignore_ascii_case("refresh") || arg1.eq_ignore_ascii_case("update") {
        // Force a live re-fetch: invalidate the TTL cache, then re-list.
        hkask_services_inference::ModelCache::invalidate();
        let ctx = InferenceContext::from(state.service_context.as_ref());
        let models = rt.block_on(InferenceService::search_models(&ctx, ""));
        match models {
            Ok(models) if models.is_empty() => {
                println!(
                    "  \x1b[33mRefreshed — no models reachable. Check providers / Ollama daemon.\x1b[0m"
                );
            }
            Ok(models) => {
                println!(
                    "  \x1b[32mRefreshed — {} models available.\x1b[0m",
                    models.len()
                );
                println!("  Use \x1b[36m/model list\x1b[0m to browse them.");
            }
            Err(e) => {
                println!("  \x1b[31mRefresh failed: {}\x1b[0m", e);
            }
        }
        println!();
        return;
    }
    if arg1.is_empty() || arg1.eq_ignore_ascii_case("list") {
        if arg1.eq_ignore_ascii_case("list") {
            let ctx = InferenceContext::from(state.service_context.as_ref());
            let models = rt.block_on(InferenceService::search_models(&ctx, ""));
            match models {
                Ok(models) if models.is_empty() => {
                    println!("  No models found — no providers reachable.");
                }
                Ok(models) => {
                    println!("  \x1b[1mAvailable models ({}):\x1b[0m", models.len());
                    println!("  {:<30} {:<12} {:<15} SIZE", "NAME", "FAMILY", "PARAMS");
                    println!("  {}", "-".repeat(70));
                    for m in &models {
                        let family = m.family.as_deref().unwrap_or("-");
                        let params = m.parameter_size.as_deref().unwrap_or("-");
                        let size_str = m
                            .size_bytes
                            .map(|s| format!("{:.1} GB", s as f64 / 1_073_741_824.0))
                            .unwrap_or_else(|| "-".to_string());
                        println!(
                            "  \x1b[36m{:<30}\x1b[0m {:<12} {:<15} {}",
                            m.name, family, params, size_str
                        );
                    }
                    println!();
                    println!("  Use \x1b[36m/model <name>\x1b[0m to switch to a specific model");
                }
                Err(e) => {
                    println!("  No models found — error listing models: {}", e);
                }
            }
        } else {
            println!("  Current model: \x1b[1m{}\x1b[0m", state.current_model);
            println!(
                "  Use \x1b[36m/model <name>\x1b[0m to switch, \x1b[36m/model <query>\x1b[0m to search"
            );
        }
    } else {
        let result = resolve_and_set_model(state, rt, arg1);
        println!("  Model set to: \x1b[1m{}\x1b[0m", result.resolved_name);
        if !result.detail.is_empty() {
            for line in result.detail.lines() {
                println!("  {}", line);
            }
        }
    }
    println!();
}
