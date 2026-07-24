//! Fusion mode handler for the REPL.
//!
//! Controls multi-model deliberation from within a session.
//! Supports OpenRouter (client-side panel+judge) and KiloCode (server-side auto-routing).

#[allow(unsafe_code)]
pub fn handle_fusion(arg1: &str, _state: &mut super::super::ReplState) {
    match arg1 {
        "" | "status" => {
            let config = hkask_inference::InferenceConfig::from_env();
            match &config.fusion {
                Some(f) => {
                    println!();
                    println!("  \x1b[1;33m⚡ Fusion mode active\x1b[0m");
                    println!("  Judge:   \x1b[36m{}\x1b[0m", f.judge);
                    println!("  Panel:   \x1b[36m{}\x1b[0m", f.panel.join(", "));
                    println!();
                    println!(
                        "  \x1b[2mConfigure:  HKASK_FUSION_JUDGE_MODEL + HKASK_FUSION_PANEL_MODELS\x1b[0m"
                    );
                    println!("  \x1b[2mDisable:    /fusion off\x1b[0m");
                }
                None => {
                    println!();
                    println!("  Fusion mode is \x1b[1;31mOFF\x1b[0m.");
                    println!(
                        "  Enable:  \x1b[36m/fusion on\x1b[0m  (requires inference provider configured)"
                    );
                }
            }
            println!();
        }
        "off" => {
            // SAFETY: called in the REPL event loop, single-threaded.
            unsafe {
                std::env::set_var("HKASK_FUSION_DISABLED", "1");
            }
            println!();
            println!("  Fusion mode \x1b[1;31mdisabled\x1b[0m for this session.");
            println!("  Restart hKask or use \x1b[36m/fusion on\x1b[0m to re-enable.");
            println!();
        }
        "on" => {
            let config = hkask_inference::InferenceConfig::from_env();
            if config.deepinfra_api_key.is_empty()
                && config.fal_api_key.is_empty()
                && config.together_api_key.is_empty()
                && config.openrouter_api_key.is_empty()
                && config.kilocode_api_key.is_empty()
            {
                println!();
                println!(
                    "  \x1b[31mCannot enable fusion:\x1b[0m no inference provider configured."
                );
                println!("  Set at least one provider API key (DI_API_KEY, KC_API_KEY, etc.).");
                println!();
                return;
            }
            // SAFETY: called in the REPL event loop, single-threaded.
            unsafe {
                std::env::remove_var("HKASK_FUSION_DISABLED");
            }
            println!();
            println!("  Fusion mode \x1b[1;32menabled\x1b[0m.");
            println!("  New inference requests will use multi-model deliberation.");
            println!("  \x1b[2mUsing kask defaults (glm-5.2 judge, 4-model panel)\x1b[0m");
            println!();
        }
        _ => {
            println!();
            println!("  \x1b[1m/fusion\x1b[0m — Manage multi-model deliberation");
            println!();
            println!("  \x1b[36m/fusion\x1b[0m          Show current status");
            println!("  \x1b[36m/fusion on\x1b[0m       Enable fusion (uses kask defaults)");
            println!("  \x1b[36m/fusion off\x1b[0m      Disable fusion");
            println!();
            println!("  \x1b[2mConfigure panel:  HKASK_FUSION_JUDGE_MODEL=KC/z-ai/glm-5.2\x1b[0m");
            println!(
                "  \x1b[2m                   HKASK_FUSION_PANEL_MODELS=Kimi2.7,Qwen3.7 Max,...\x1b[0m"
            );
            println!();
        }
    }
}
