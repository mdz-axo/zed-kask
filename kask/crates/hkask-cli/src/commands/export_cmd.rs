//! Export command handlers for `kask export`
//!
//! REQ: P1-deploy-backup-export — P1 User Sovereignty: CLI access to sovereignty archive operations.
//! expect: "I can export and upload my encrypted h_mem archive for data portability"

use crate::cli::ExportAction;
use base64::Engine;
use std::path::PathBuf;

/// pre:  action is ExportAction::Create or ExportAction::Upload
/// post: dispatches to create or upload handler
pub fn run(rt: &tokio::runtime::Runtime, action: ExportAction) {
    match action {
        ExportAction::Create { passphrase } => run_create(rt, &passphrase),
        ExportAction::Upload {
            archive,
            passphrase,
        } => run_upload(rt, &archive, &passphrase),
    }
}

/// Create a sovereignty backup archive.
///
/// Calls POST /api/v1/export/create with the provided passphrase.
fn run_create(rt: &tokio::runtime::Runtime, passphrase: &str) {
    if passphrase.len() < 8 {
        eprintln!("Error: Passphrase must be at least 8 characters.");
        std::process::exit(1);
    }

    let result: Result<(), Box<dyn std::error::Error>> = rt.block_on(async {
        let client = reqwest::Client::new();
        let base_url =
            std::env::var("HKASK_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

        let resp = client
            .post(format!("{base_url}/api/v1/export/create"))
            .json(&serde_json::json!({"passphrase": passphrase}))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            eprintln!("Export failed: {body}");
            std::process::exit(1);
        }

        let export: serde_json::Value =
            resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
        println!(
            "Archive created: {}",
            export["archive_path"].as_str().unwrap_or("unknown")
        );
        println!("Triples: {}", export["triple_count"].as_u64().unwrap_or(0));
        println!("Size: {} bytes", export["bytes"].as_u64().unwrap_or(0));
        println!(
            "Duration: {} ms",
            export["duration_ms"].as_u64().unwrap_or(0)
        );

        Ok(())
    });

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

/// Upload a sovereignty archive for migration.
///
/// Reads the archive file, base64-encodes it, and sends it to
/// POST /api/v1/export/upload.
fn run_upload(rt: &tokio::runtime::Runtime, archive_path: &PathBuf, passphrase: &str) {
    if passphrase.len() < 8 {
        eprintln!("Error: Passphrase must be at least 8 characters.");
        std::process::exit(1);
    }

    if !archive_path.exists() {
        eprintln!("Error: Archive file not found: {}", archive_path.display());
        std::process::exit(1);
    }

    let archive_bytes = match std::fs::read(archive_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading archive: {e}");
            std::process::exit(1);
        }
    };

    let archive_base64 = base64::engine::general_purpose::STANDARD.encode(&archive_bytes);

    let result: Result<(), Box<dyn std::error::Error>> = rt.block_on(async {
        let client = reqwest::Client::new();
        let base_url =
            std::env::var("HKASK_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

        let resp = client
            .post(format!("{base_url}/api/v1/export/upload"))
            .json(&serde_json::json!({
                "archive_base64": archive_base64,
                "passphrase": passphrase,
            }))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            eprintln!("Upload failed: {body}");
            std::process::exit(1);
        }

        let receipt: serde_json::Value =
            resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
        println!(
            "Import complete. {} h_mems imported.",
            receipt["triple_count"].as_u64().unwrap_or(0)
        );

        Ok(())
    });

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
