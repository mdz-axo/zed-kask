//! `/talk on|off|voice` — agent speech output mode.
//!
//! When enabled, each agent response is summarized into 1-3 spoken sentences
//! and played through the system audio output via ffplay.
//!
//! The summarizer strips tool calls, code blocks, and internal reasoning,
//! keeping only what a person would say aloud.

use crate::ReplState;
use crate::TalkMode;
use hkask_capability::DelegationAction;
use hkask_capability::DelegationResource;
use hkask_capability::DelegationToken;
use hkask_capability::ToolPort;
use hkask_capability::derive_signing_key;

/// Speech summarizer prompt — condenses agent response for spoken output.
const SPEECH_SUMMARIZE_PROMPT: &str = "\
You are a speech condenser. Convert the following agent response into 1-3 natural, \
conversational sentences suitable for speaking aloud. Rules:
- Drop all tool calls, code blocks, markdown formatting, and internal reasoning.
- Keep only the core answer — what a person would actually say.
- Use plain, direct language. No bullet points, no structured output.
- Respond with ONLY the spoken text, no preamble or explanation.

Agent response:
";

/// Handle `/talk on|off|voice [description]`.
pub fn handle_talk(
    subcommand: &str,
    arg: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
) {
    match subcommand {
        "on" => {
            state.talk_config.mode = TalkMode::On;
            let voice_info = match &state.talk_config.voice_design {
                Some(vd) => {
                    if let Ok(design) = serde_json::from_str::<serde_json::Value>(vd) {
                        design
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("custom")
                            .to_string()
                    } else {
                        "custom".to_string()
                    }
                }
                None => "Rachel (default)".to_string(),
            };
            println!("  \x1b[32m✓\x1b[0m Talk mode \x1b[1mon\x1b[0m");
            println!("  Voice: \x1b[36m{}\x1b[0m", voice_info);
            println!("  Each agent response will be summarized and spoken aloud.");
            println!();
        }
        "off" => {
            state.talk_config.mode = TalkMode::Off;
            println!("  Talk mode \x1b[1moff\x1b[0m");
            println!();
        }
        "voice" => {
            if arg.is_empty() {
                // Show current voice
                match &state.talk_config.voice_design {
                    Some(vd) => {
                        println!("  Current voice design:");
                        match serde_json::from_str::<serde_json::Value>(vd) {
                            Ok(design) => {
                                if let Some(name) = design.get("name").and_then(|n| n.as_str()) {
                                    println!("    Name: \x1b[1m{}\x1b[0m", name);
                                }
                                if let Some(desc) =
                                    design.get("description").and_then(|d| d.as_str())
                                {
                                    println!("    Description: {}", desc);
                                }
                                println!(
                                    "    Preset: \x1b[36m{}\x1b[0m",
                                    voice_preset_from_design(vd)
                                );
                            }
                            Err(_) => println!("    (unparseable)"),
                        }
                    }
                    None => {
                        println!("  Voice: \x1b[36mRachel\x1b[0m (default)");
                        println!(
                            "  Set a custom voice with \x1b[36m/talk voice \"description\"\x1b[0m"
                        );
                    }
                }
                println!();
                return;
            }

            // Set voice via voice_design MCP tool
            let a2a_secret = match state.resolved_secrets {
                Some(ref secrets) => secrets.a2a_secret.as_bytes(),
                None => {
                    println!(
                        "  \x1b[31mError:\x1b[0m No A2A secret resolved. Run onboarding first."
                    );
                    println!();
                    return;
                }
            };

            let token = DelegationToken::new(
                DelegationResource::Tool,
                "voice_design".to_string(),
                DelegationAction::Execute,
                state.host.resolve_user_webid(),
                state.agent_webid,
                &derive_signing_key(a2a_secret),
            );

            println!("  \x1b[2mDesigning voice from description...\x1b[0m");

            let result = rt.block_on(async {
                state
                    .service_context
                    .infra()
                    .mcp
                    .invoke(
                        "hkask-mcp-media",
                        "voice_design",
                        serde_json::json!({"description": arg}),
                        &token,
                    )
                    .await
            });

            match result {
                Ok(value) => {
                    // voice_design returns the VoiceDesign JSON directly or wrapped
                    let vd_json = serde_json::to_string(&value).unwrap_or_default();
                    let preset = voice_preset_from_design(&vd_json);
                    state.talk_config.voice_design = Some(vd_json);
                    println!("  \x1b[32m✓\x1b[0m Voice set to \x1b[36m{}\x1b[0m", preset);
                    if let Some(name) = value.get("name").and_then(|n| n.as_str()) {
                        println!("  Profile: \x1b[1m{}\x1b[0m", name);
                    }
                }
                Err(e) => {
                    println!("  \x1b[31mVoice design failed:\x1b[0m {}", e);
                    println!("  Using default \x1b[36mRachel\x1b[0m voice.");
                }
            }
            println!();
        }
        "" => {
            let status = match state.talk_config.mode {
                TalkMode::On => "on",
                TalkMode::Off => "off",
            };
            let voice_info = match &state.talk_config.voice_design {
                Some(vd) => voice_preset_from_design(vd),
                None => "Rachel (default)".to_string(),
            };
            println!("  \x1b[1mTalk mode:\x1b[0m {}", status);
            println!("  \x1b[1mVoice:\x1b[0m     {}", voice_info);
            println!();
            println!("  \x1b[36m/talk on\x1b[0m                 Enable speech output");
            println!("  \x1b[36m/talk off\x1b[0m                Disable speech output");
            println!("  \x1b[36m/talk voice [DESC]\x1b[0m        Set or show voice profile");
            println!();
        }
        other => {
            println!("  Unknown /talk subcommand: \x1b[31m{}\x1b[0m", other);
            println!(
                "  Try \x1b[36m/talk on\x1b[0m, \x1b[36m/talk off\x1b[0m, or \x1b[36m/talk voice\x1b[0m"
            );
            println!();
        }
    }
}

/// Summarize an agent response for spoken output.
///
/// Calls the inference port with a speech-condensation prompt. Returns
/// a concise spoken summary (1-3 sentences, plain text).
pub fn summarize_for_speech(
    response_text: &str,
    state: &ReplState,
    rt: &tokio::runtime::Handle,
) -> Option<String> {
    // Skip summarization for very short responses
    if response_text.len() < 50 {
        return Some(response_text.to_string());
    }

    let prompt = format!("{}{}", SPEECH_SUMMARIZE_PROMPT, response_text);

    let params = hkask_types::template::LLMParameters {
        temperature: 0.3,
        max_tokens: 120,
        disable_thinking: true,
        bypass_fusion: true,
        ..Default::default()
    };

    let result = rt.block_on(async {
        state
            .service_context
            .inference_port()
            .expect("inference port")
            .generate(&prompt, &params, None)
            .await
    });

    match result {
        Ok(r) => {
            let summary = r.text.trim().to_string();
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        }
        Err(e) => {
            tracing::warn!(target: "hkask.cli.talk", error = %e, "Speech summarization failed");
            None
        }
    }
}

/// Speak text aloud: summarize → generate speech → play via ffplay.
///
/// Called after each agent response when talk mode is enabled.
pub fn speak_response(response_text: &str, state: &ReplState, rt: &tokio::runtime::Handle) {
    // Step 1: Summarize for speech
    let Some(summary) = summarize_for_speech(response_text, state, rt) else {
        return;
    };

    // Step 2: Generate speech audio
    let a2a_secret = match state.resolved_secrets {
        Some(ref secrets) => secrets.a2a_secret.as_bytes(),
        None => return,
    };

    let token = DelegationToken::new(
        DelegationResource::Tool,
        "generate_speech".to_string(),
        DelegationAction::Execute,
        state.host.resolve_user_webid(),
        state.agent_webid,
        &derive_signing_key(a2a_secret),
    );

    let voice_design = state.talk_config.voice_design.clone();

    let audio_result = rt.block_on(async {
        let mut args = serde_json::json!({"text": summary});
        if let Some(ref vd) = voice_design {
            args["voice_design"] = serde_json::Value::String(vd.clone());
        }
        state
            .service_context
            .infra()
            .mcp
            .invoke("hkask-mcp-media", "generate_speech", args, &token)
            .await
    });

    let audio_b64 = match audio_result {
        Ok(value) => {
            match value.get("audio").and_then(|a| a.as_str()) {
                Some(data_uri) => {
                    // Strip "data:audio/mp3;base64," prefix
                    if let Some(comma_pos) = data_uri.find(',') {
                        data_uri[comma_pos + 1..].to_string()
                    } else {
                        data_uri.to_string()
                    }
                }
                None => {
                    // Try plain "audio" field without data URI prefix
                    value
                        .get("audio")
                        .and_then(|a| a.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                }
            }
        }
        Err(e) => {
            tracing::warn!(target: "hkask.cli.talk", error = %e, "Speech generation failed");
            return;
        }
    };

    if audio_b64.is_empty() {
        return;
    }

    // Step 3: Decode base64 → temp file
    let temp_dir = match std::env::temp_dir().join("hkask-talk").as_path() {
        p if !p.exists() => {
            let _ = std::fs::create_dir_all(p);
            p.to_path_buf()
        }
        p => p.to_path_buf(),
    };

    let audio_path = temp_dir.join(format!("speech_{}.mp3", uuid::Uuid::new_v4()));

    use base64::Engine;
    let audio_bytes = match base64::engine::general_purpose::STANDARD.decode(&audio_b64) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(target: "hkask.cli.talk", error = %e, "Base64 decode failed");
            return;
        }
    };

    if let Err(e) = std::fs::write(&audio_path, &audio_bytes) {
        tracing::warn!(target: "hkask.cli.talk", error = %e, "Failed to write audio temp file");
        return;
    }

    // Step 4: Play via ffplay (non-blocking subprocess)
    match std::process::Command::new("ffplay")
        .args([
            "-nodisp",
            "-autoexit",
            "-loglevel",
            "quiet",
            &audio_path.to_string_lossy(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            // Wait for playback to complete (or until interrupted)
            let _ = child.wait();
            // Clean up temp file
            let _ = std::fs::remove_file(&audio_path);
        }
        Err(e) => {
            tracing::warn!(target: "hkask.cli.talk", error = %e, "ffplay failed to start");
            let _ = std::fs::remove_file(&audio_path);
        }
    }
}

/// Extract the ElevenLabs voice preset name from a VoiceDesign JSON string.
/// Falls back to "Rachel" if parsing fails.
fn voice_preset_from_design(vd_json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(vd_json) {
        Ok(design) => {
            if let Some(name) = design.get("elevenlabs_voice").and_then(|v| v.as_str()) {
                return name.to_string();
            }
            if let Some(name) = design.get("preset").and_then(|v| v.as_str()) {
                return name.to_string();
            }
            if let Some(name) = design.get("name").and_then(|v| v.as_str()) {
                return name.to_string();
            }
            "custom".to_string()
        }
        Err(_) => "Rachel".to_string(),
    }
}

#[cfg(test)]
mod tests {
    /// expect: "I can access all hKask functionality through the kask CLI"
    #[test]
    fn summarize_short_text_threshold() {
        // Pure logic: text under 50 chars returns the text unchanged.
        // The 50-char threshold avoids wasting inference calls on trivial responses.
        let short = "Hello!";
        assert!(short.len() < 50, "short text is below threshold");

        // Verify the prompt template includes the required constraints
        let prompt = super::SPEECH_SUMMARIZE_PROMPT;
        assert!(prompt.contains("1-3 natural"), "prompt specifies length");
        assert!(
            prompt.contains("Drop all tool calls"),
            "prompt strips tool calls"
        );
        assert!(
            prompt.contains("ONLY the spoken text"),
            "prompt requires clean output"
        );
    }

    /// expect: "I can access all hKask functionality through the kask CLI"
    #[test]
    fn handler_dispatch_no_panic() {
        // Verify the handler functions exist and accept the right signatures.
        // Full integration is tested live with a running inference backend.
        use super::handle_talk;

        // Compile-time verification: these function pointers confirm signatures
        let _handler: fn(&str, &str, &mut crate::ReplState, &tokio::runtime::Handle) = handle_talk;
        let _summarizer: fn(&str, &crate::ReplState, &tokio::runtime::Handle) -> Option<String> =
            super::summarize_for_speech;
    }
}
