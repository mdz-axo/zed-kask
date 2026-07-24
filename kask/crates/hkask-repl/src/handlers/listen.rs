//! `/listen start|stop|view` — voice recording and transcript playback.
//!
//! Pipeline:
//!   /listen start \[duration_secs\]
//!     → audio_capture (MCP media server)
//!     → transcribe_bundle (word-level timestamps)
//!     → save TranscriptBundle JSON to ~/.config/hkask/transcripts/
//!   /listen stop
//!     → show last recording info
//!   /listen view [file]
//!     → open TUI transcript viewer

use crate::ReplState;
use hkask_capability::DelegationAction;
use hkask_capability::DelegationResource;
use hkask_capability::DelegationToken;
use hkask_capability::ToolPort;
use hkask_capability::derive_signing_key;
use std::path::PathBuf;

/// Directory where transcripts are saved.
fn transcripts_dir() -> PathBuf {
    let base = dirs_next().unwrap_or_else(|| PathBuf::from("."));
    base.join(".config").join("hkask").join("transcripts")
}

/// Handle `/listen start|stop|view [args]`.
pub fn handle_listen(
    subcommand: &str,
    arg: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
) {
    match subcommand {
        "start" => handle_start(arg, state, rt),
        "stop" => handle_stop(),
        "view" => handle_view(arg, state, rt),
        "" => {
            println!("  \x1b[1m/listen\x1b[0m — voice recording and transcript playback");
            println!();
            println!(
                "  \x1b[36m/listen start [SECONDS]\x1b[0m  Record audio, transcribe, save bundle"
            );
            println!("  \x1b[36m/listen stop\x1b[0m              Show last recording info");
            println!("  \x1b[36m/listen view [FILE]\x1b[0m        Open TUI transcript viewer");
            println!();
        }
        other => {
            println!("  Unknown /listen subcommand: \x1b[31m{}\x1b[0m", other);
            println!("  Try \x1b[36m/listen\x1b[0m for usage.");
            println!();
        }
    }
}

fn handle_start(duration_arg: &str, state: &mut ReplState, rt: &tokio::runtime::Handle) {
    let duration_secs: f32 = duration_arg.parse().unwrap_or(30.0);
    let duration_secs = duration_secs.clamp(1.0, 3600.0);

    // Mint capability token
    let a2a_secret = match state.resolved_secrets {
        Some(ref secrets) => secrets.a2a_secret.as_bytes(),
        None => {
            println!("  \x1b[31mError:\x1b[0m No A2A secret resolved. Run onboarding first.");
            println!();
            return;
        }
    };

    let token = DelegationToken::new(
        DelegationResource::Tool,
        "listen".to_string(),
        DelegationAction::Execute,
        state.host.resolve_user_webid(),
        state.agent_webid,
        &derive_signing_key(a2a_secret),
    );

    // Step 1: Capture audio
    println!(
        "  \x1b[2mRecording for \x1b[36m{}s\x1b[0m\x1b[2m...\x1b[0m",
        duration_secs
    );

    let capture_result = rt.block_on(async {
        state
            .service_context
            .infra()
            .mcp
            .invoke(
                "hkask-mcp-media",
                "audio_capture",
                serde_json::json!({"duration_secs": duration_secs}),
                &token,
            )
            .await
    });

    let audio_path = match capture_result {
        Ok(value) => {
            let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status != "captured" {
                let err = value
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                println!("  \x1b[31mCapture failed:\x1b[0m {}", err);
                println!();
                return;
            }
            match value.get("output").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => {
                    println!("  \x1b[31mError:\x1b[0m No output path in capture response");
                    println!();
                    return;
                }
            }
        }
        Err(e) => {
            println!("  \x1b[31mCapture failed:\x1b[0m {}", e);
            println!();
            return;
        }
    };

    println!("  \x1b[2mTranscribing...\x1b[0m");

    // Step 2: Transcribe with word-level timestamps
    let transcribe_result = rt.block_on(async {
        state
            .service_context
            .infra()
            .mcp
            .invoke(
                "hkask-mcp-media",
                "transcribe_bundle",
                serde_json::json!({"audio_url": audio_path}),
                &token,
            )
            .await
    });

    let bundle_json = match transcribe_result {
        Ok(value) => value,
        Err(e) => {
            println!("  \x1b[31mTranscription failed:\x1b[0m {}", e);
            println!("  \x1b[2mAudio saved to: {}\x1b[0m", audio_path);
            println!();
            return;
        }
    };

    // Step 3: Save TranscriptBundle JSON
    let transcripts = transcripts_dir();
    if let Err(e) = std::fs::create_dir_all(&transcripts) {
        println!(
            "  \x1b[31mError:\x1b[0m Cannot create transcripts dir: {}",
            e
        );
        println!();
        return;
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("listen_{}.json", timestamp);
    let bundle_path = transcripts.join(&filename);

    match serde_json::to_string_pretty(&bundle_json) {
        Ok(json_str) => {
            if let Err(e) = std::fs::write(&bundle_path, &json_str) {
                println!("  \x1b[31mError:\x1b[0m Cannot save transcript: {}", e);
                println!();
                return;
            }
        }
        Err(e) => {
            println!("  \x1b[31mError:\x1b[0m Cannot serialize transcript: {}", e);
            println!();
            return;
        }
    }

    // Also save as "latest.json" for /listen view default
    let latest_path = transcripts.join("latest.json");
    let _ = std::fs::write(
        &latest_path,
        serde_json::to_string_pretty(&bundle_json).unwrap_or_default(),
    );

    // Summary
    let word_count = bundle_json
        .get("words")
        .and_then(|w| w.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let full_text = bundle_json
        .get("full_text")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    println!();
    println!(
        "  \x1b[32m✓\x1b[0m Recorded \x1b[1m{}s\x1b[0m, \x1b[1m{} words\x1b[0m",
        duration_secs, word_count
    );
    println!("  \x1b[2mSaved to:\x1b[0m {}", bundle_path.display());
    if !full_text.is_empty() {
        let preview: String = full_text.chars().take(120).collect();
        let suffix = if full_text.chars().count() > 120 {
            "…"
        } else {
            ""
        };
        println!("  \x1b[2mPreview:\x1b[0m \"{}{}\"", preview, suffix);
    }
    println!(
        "  \x1b[2mUse \x1b[36m/listen view\x1b[0m\x1b[2m to open the interactive viewer\x1b[0m"
    );
    println!();
}

fn handle_stop() {
    let transcripts = transcripts_dir();
    let latest = transcripts.join("latest.json");

    if latest.exists() {
        match std::fs::read_to_string(&latest) {
            Ok(json_str) => {
                if let Ok(bundle) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    let words = bundle
                        .get("words")
                        .and_then(|w| w.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let text = bundle
                        .get("full_text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    let preview: String = text.chars().take(80).collect();
                    println!("  Last recording: \x1b[1m{} words\x1b[0m", words);
                    if !preview.is_empty() {
                        println!("  \"{}…\"", preview);
                    }
                } else {
                    println!("  Last recording exists but is unreadable.");
                }
            }
            Err(_) => println!("  Last recording exists but is unreadable."),
        }
    } else {
        println!("  No recordings yet. Use \x1b[36m/listen start\x1b[0m to record.");
    }
    println!();
}

#[cfg_attr(not(feature = "tui"), allow(unused_variables))]
fn handle_view(file_arg: &str, state: &crate::ReplState, _rt: &tokio::runtime::Handle) {
    let path = if file_arg.is_empty() {
        // Default: most recent recording
        let latest = transcripts_dir().join("latest.json");
        if !latest.exists() {
            println!("  No recordings yet. Use \x1b[36m/listen start\x1b[0m to record.");
            println!();
            return;
        }
        latest
    } else {
        PathBuf::from(file_arg)
    };

    if !path.exists() {
        println!("  \x1b[31mFile not found:\x1b[0m {}", path.display());
        println!();
        return;
    }

    #[cfg(feature = "tui")]
    {
        println!(
            "  Opening transcript viewer for \x1b[2m{}\x1b[0m...",
            path.display()
        );

        match state.host.open_transcript_viewer(&path) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("  Transcript viewer error: {}", e);
            }
        }
        println!();
    }
    #[cfg(not(feature = "tui"))]
    {
        println!("  Transcript viewer not built — rebuild with `cargo build --features tui`");
        println!();
    }
}

/// Helper: get the user's config directory.
fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}
