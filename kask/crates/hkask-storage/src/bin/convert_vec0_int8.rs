#![forbid(unsafe_code)]
//! `convert-vec0-int8` — quantize a database's vec0 vector column from
//! float32 to int8, in place.
//!
//! # Why this exists
//!
//! The wave-6 quantization layer cuts the vector payload 4x (one byte per
//! dimension instead of four). New stores declare int8 at creation (the
//! schema default); existing databases keep their float32 table until
//! this binary converts them — the conversion is deliberate because it
//! is a measured recall trade (the calibration harness's replicate
//! spread is the gate) and because converting a sealed calibration
//! database changes its byte digest, which a reseal pass must re-bind
//! before federated admission accepts it again.
//!
//! # Why a binary rather than SQL through the sqlcipher CLI
//!
//! The conversion creates and drops vec0 virtual tables; their shadow
//! tables stay consistent only through the module (the bundled extension
//! is statically linked into this crate and loaded per connection by
//! `Database::open`). The system `sqlcipher` CLI has no vec0 module.
//!
//! # Usage
//!
//!   convert-vec0-int8 <db> [<db> ...]
//!
//! `HKASK_DB_PASSPHRASE` resolves exactly like the MCP servers (fail
//! closed). Each database opens read-write under the maintenance lease —
//! a database open elsewhere reports the lease conflict and is skipped,
//! never interleaved. The ledger line per database reports rows,
//! dimensions, state, and the measured file size before and after.
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let databases: Vec<String> = std::env::args().skip(1).collect();
    if databases.is_empty() {
        eprintln!("usage: convert-vec0-int8 <db> [<db> ...]");
        return ExitCode::from(2);
    }
    let passphrase = match std::env::var("HKASK_DB_PASSPHRASE") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => {
            eprintln!("HKASK_DB_PASSPHRASE is set but empty — refusing an empty-key open");
            return ExitCode::from(69);
        }
        Err(_) => {
            eprintln!(
                "HKASK_DB_PASSPHRASE is not set — it is the variable the MCP servers receive; \
                 export it before converting"
            );
            return ExitCode::from(69);
        }
    };

    let mut failures = 0_usize;
    for path in &databases {
        match convert_one(path, &passphrase) {
            Ok(line) => println!("{line}"),
            Err(error) => {
                eprintln!("{path}: FAILED — {error}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!("{failures} of {} database(s) failed", databases.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn convert_one(path: &str, passphrase: &str) -> Result<String, String> {
    if !Path::new(path).is_file() {
        return Err("not found".to_string());
    }
    let bytes_before = std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| error.to_string())?;
    let database = hkask_storage::Database::open(path, passphrase)
        .map_err(|error| format!("open failed: {error}"))?;
    let pool = database
        .sqlite_pool()
        .map_err(|error| format!("pool failed: {error}"))?;
    let connection = pool.get().map_err(|error| format!("pool get: {error}"))?;
    let outcome = hkask_storage::vec0::convert_vec0_to_int8(&connection)
        .map_err(|error| format!("conversion failed: {error}"))?;
    let bytes_after = std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "{path}\trows={}\tdim={}\tstate={}\tbytes={bytes_before}->{bytes_after}",
        outcome.rows,
        outcome.dimensions,
        if outcome.already_int8 {
            "already-int8"
        } else {
            "converted"
        },
    ))
}
