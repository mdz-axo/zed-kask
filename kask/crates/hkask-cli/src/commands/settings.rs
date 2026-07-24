//! Settings command — CLI surface for REPL inference settings.
//!
//! Persists settings to `~/.config/hkask/settings.json` so CLI, API, and
//! interactive REPL share the same configuration. Magna Carta P3 (Generative
//! Space): all settings exposed equally across every surface.
//!
//! Delegates load/save to `hkask_services (via hkask-services-core)` for shared persistence.

use crate::cli::SettingsAction;
use hkask_repl::handlers::ReplSettings;
use hkask_services_core::{load_settings, save_settings};

/// CLI handler for `kask settings {show,set,reset}`.
/// pre:  action is a valid SettingsAction variant (Show, Set, Reset)
/// post: loads/saves REPL settings from ~/.config/hkask/settings.json; prints current values or confirmation
pub fn run(action: SettingsAction) {
    match action {
        SettingsAction::Show { name } => {
            let settings: ReplSettings = load_settings();
            match name {
                None => show_all(&settings),
                Some(key) => show_one(&settings, &key),
            }
        }
        SettingsAction::Set { name, value } => {
            let mut settings: ReplSettings = load_settings();
            match settings.apply(&name, &value) {
                Ok(()) => match save_settings(&settings) {
                    Ok(()) => println!("Saved."),
                    Err(e) => eprintln!("Error saving settings: {}", e),
                },
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        SettingsAction::Reset => {
            let settings = ReplSettings::default();
            match save_settings(&settings) {
                Ok(()) => {
                    println!("Settings reset to defaults:");
                    show_all(&settings);
                }
                Err(e) => eprintln!("Error saving settings: {}", e),
            }
        }
    }
}

fn show_all(settings: &ReplSettings) {
    let s = settings;
    println!("tool_loop_limit:  {}", s.tool_loop_limit);
    println!("context_turns:    {} (0 = no history)", s.context_turns);
    println!("temperature:      {}", s.temperature);
    println!("top_p:            {}", s.top_p);
    println!("top_k:            {}", s.top_k);
    println!("min_p:            {}", s.min_p);
    println!("typical_p:        {}", s.typical_p);
    println!("max_tokens:       {}", s.max_tokens);
    match s.seed {
        Some(v) => println!("seed:             {}", v),
        None => println!("seed:             random"),
    }
    println!("gas_heuristic:    {}", s.gas_heuristic);
    println!("gas_cap:          {}", s.gas_cap);
    println!(
        "auto_condense:     {}",
        if s.auto_condense { "on" } else { "off" }
    );
    if let Some(ref meta) = s.model_meta {
        println!(
            "context_length:   {} (model read-only)",
            meta.context_length
        );
        println!(
            "supports_thinking: {}",
            if meta.supports_thinking { "yes" } else { "no" }
        );
    }
    println!("── model defaults ──");
    println!(
        "disable_thinking:  {}",
        if s.disable_thinking { "yes" } else { "no" }
    );
    println!("embedding_model:  {}", s.embedding_model);
    println!("classifier_model:   {}", s.classifier_model);
    println!("ocr_model:                  {}", s.ocr_model);
    println!("── ocr thresholds ──");
    println!("ocr_simple_max:   {}", s.ocr_simple_max);
    println!("ocr_moderate_max: {}", s.ocr_moderate_max);
    println!("ocr_sample_rate:  {}", s.ocr_sample_rate);
}

fn show_one(settings: &ReplSettings, key: &str) {
    let s = settings;
    match key {
        "tool_loop_limit" | "loops" => println!("{}", s.tool_loop_limit),
        "context_turns" | "context" => println!("{}", s.context_turns),
        "temperature" | "temp" => println!("{}", s.temperature),
        "top_p" => println!("{}", s.top_p),
        "top_k" => println!("{}", s.top_k),
        "min_p" => println!("{}", s.min_p),
        "typical_p" => println!("{}", s.typical_p),
        "max_tokens" => println!("{}", s.max_tokens),
        "seed" => match s.seed {
            Some(v) => println!("{}", v),
            None => println!("random"),
        },
        "gas_heuristic" => println!("{}", s.gas_heuristic),
        "gas_cap" => println!("{}", s.gas_cap),
        "auto_condense" => println!("{}", if s.auto_condense { "on" } else { "off" }),
        "context_length" => match s.model_meta {
            Some(ref m) => println!("{}", m.context_length),
            None => println!("(not set)"),
        },
        "disable_thinking" | "thinking" => {
            println!("{}", if s.disable_thinking { "yes" } else { "no" })
        }
        "embedding_model" | "emb_model" => println!("{}", s.embedding_model),
        "classifier_model" | "cls_model" => println!("{}", s.classifier_model),
        "ocr_model" => println!("{}", s.ocr_model),
        "ocr_simple_max" => println!("{}", s.ocr_simple_max),
        "ocr_moderate_max" => println!("{}", s.ocr_moderate_max),
        "ocr_sample_rate" => println!("{}", s.ocr_sample_rate),
        _ => eprintln!("Unknown setting: {}", key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_settings() -> ReplSettings {
        ReplSettings::default()
    }

    #[test]
    fn apply_setting_rejects_zero_loop_limit() {
        let mut s = default_settings();
        assert!(s.apply("loops", "0").is_err());
        assert_eq!(s.tool_loop_limit, 21);
    }

    #[test]
    fn apply_setting_rejects_negative_loop_limit() {
        let mut s = default_settings();
        assert!(s.apply("loops", "-1").is_err());
        assert_eq!(s.tool_loop_limit, 21);
    }

    #[test]
    fn apply_setting_rejects_temperature_oor() {
        let mut s = default_settings();
        assert!(s.apply("temp", "3.0").is_err());
        assert!((s.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_rejects_top_p_oor() {
        let mut s = default_settings();
        assert!(s.apply("top_p", "1.5").is_err());
        assert!((s.top_p - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_rejects_top_k_zero() {
        let mut s = default_settings();
        assert!(s.apply("top_k", "0").is_err());
        assert_eq!(s.top_k, 40);
    }

    #[test]
    fn apply_setting_rejects_garbage_value() {
        let mut s = default_settings();
        assert!(s.apply("temp", "not-a-number").is_err());
        assert!((s.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_accepts_valid_temperature() {
        let mut s = default_settings();
        assert!(s.apply("temp", "0.3").is_ok());
        assert!((s.temperature - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_accepts_valid_loop_limit() {
        let mut s = default_settings();
        assert!(s.apply("loops", "100").is_ok());
        assert_eq!(s.tool_loop_limit, 100);
    }

    #[test]
    fn apply_setting_accepts_auto_condense_off() {
        let mut s = default_settings();
        assert!(s.apply("auto_condense", "off").is_ok());
        assert!(!s.auto_condense);
    }

    #[test]
    fn apply_setting_accepts_auto_condense_on() {
        let mut s = default_settings();
        s.auto_condense = false;
        assert!(s.apply("auto_condense", "true").is_ok());
        assert!(s.auto_condense);
    }

    #[test]
    fn apply_setting_accepts_seed_value() {
        let mut s = default_settings();
        assert!(s.apply("seed", "42").is_ok());
        assert_eq!(s.seed, Some(42));
    }

    #[test]
    fn apply_setting_accepts_seed_off() {
        let mut s = default_settings();
        s.seed = Some(99);
        assert!(s.apply("seed", "off").is_ok());
        assert_eq!(s.seed, None);
    }

    #[test]
    fn apply_rejects_unknown_setting() {
        let mut s = default_settings();
        assert!(s.apply("nonexistent", "value").is_err());
    }
}
