//! `kask repair` — detect and fix passphrase-mismatched databases and stranded
//! Matrix registration markers.
//!
//! Scans all userpod directories for:
//! 1. SQLCipher databases that can't be opened with the current passphrase
//! 2. Stranded Matrix pod registration entries in the OS keychain
//!
//! Reports findings and (optionally) deletes broken artifacts so they can be
//! regenerated with the correct passphrase on next startup.

use hkask_storage::DatabaseError;
use hkask_storage::check_passphrase;
use std::path::PathBuf;

/// Database files found under a userpod directory, keyed by path.
const AGENT_DB_FILES: &[&str] = &[
    "memory.db",
    "kanban.db",
    "pod.db",
    "wallet.db",
    "training.db",
    "style.db",
];

/// Discover all userpod directories under `userpods/` in the current working directory.
fn discover_agent_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let agents_dir = std::path::Path::new(hkask_types::agent_paths::USERPODS_DIR);
    if !agents_dir.is_dir() {
        return dirs;
    }
    if let Ok(entries) = std::fs::read_dir(agents_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(entry.path());
            }
        }
    }
    dirs
}

/// Discover all database files across all agent directories.
fn discover_databases() -> Vec<PathBuf> {
    let mut dbs = Vec::new();
    for agent_dir in discover_agent_dirs() {
        for &db_name in AGENT_DB_FILES {
            let db_path = agent_dir.join(db_name);
            if db_path.is_file() {
                dbs.push(db_path);
            }
        }
    }
    dbs
}

/// Run the repair command.
pub fn run(dry_run: bool, force: bool) {
    let passphrase = match std::env::var("HKASK_DB_PASSPHRASE") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!("HKASK_DB_PASSPHRASE is not set — cannot verify databases.");
            eprintln!("Set it to your userpod passphrase and try again.");
            eprintln!("  export HKASK_DB_PASSPHRASE=<your-passphrase>");
            std::process::exit(1);
        }
    };

    let databases = discover_databases();

    if databases.is_empty() {
        println!("No userpod databases found under userpods/.");
        println!("Nothing to repair.");
        return;
    }

    let mut broken = Vec::new();
    let mut healthy = Vec::new();

    println!(
        "Checking {} database(s) under userpods/...\n",
        databases.len()
    );

    for db_path in &databases {
        let path_str = db_path.to_string_lossy();
        match check_passphrase(&path_str, &passphrase) {
            Ok(()) => {
                healthy.push(db_path.clone());
            }
            Err(DatabaseError::PassphraseMismatch(_)) => {
                println!("  BROKEN: {}  (wrong passphrase)", path_str);
                broken.push(db_path.clone());
            }
            Err(e) => {
                println!("  CORRUPT: {}  ({})", path_str, e);
                broken.push(db_path.clone());
            }
        }
    }

    if !healthy.is_empty() {
        println!("  OK: {} database(s) healthy", healthy.len());
    }

    // ── Keychain scan: Matrix registration markers ──
    let keychain = hkask_keystore::Keychain::default();
    let mut matrix_issues = Vec::new();

    // Check for pending onboarding registration.
    if keychain
        .retrieve_by_key(hkask_types::keychain_keys::KEY_MATRIX_PENDING_RECOVERY)
        .unwrap_or_default()
        == "true"
    {
        let homeserver = keychain
            .retrieve_by_key(hkask_types::keychain_keys::KEY_MATRIX_PENDING_HOMESERVER)
            .unwrap_or_else(|_| "unknown".to_string());
        matrix_issues.push(format!(
            "Pending Matrix onboarding registration (homeserver: {})",
            homeserver
        ));
    }

    // Check for per-agent failed pod registrations.
    // Keychain doesn't support prefix listing, so we iterate agent dirs.
    for agent_dir in discover_agent_dirs() {
        if let Some(name) = agent_dir.file_name().and_then(|n| n.to_str()) {
            let failed_key = format!("matrix-pod-failed-{}", name);
            if keychain.retrieve_by_key(&failed_key).is_ok() {
                matrix_issues.push(format!(
                    "Failed Matrix registration for agent '{}' (marker: {})",
                    name, failed_key
                ));
            }
            let pending_key = format!(
                "{}-{}",
                hkask_types::keychain_keys::KEY_MATRIX_POD_PENDING_PREFIX,
                name
            );
            if let Ok(url) = keychain.retrieve_by_key(&pending_key) {
                matrix_issues.push(format!(
                    "Pending Matrix registration for agent '{}' (homeserver: {})",
                    name, url
                ));
            }
        }
    }

    if !matrix_issues.is_empty() {
        println!();
        println!("  ── Matrix Registration Issues ──");
        for issue in &matrix_issues {
            println!("  ⚠  {}", issue);
        }
        println!();
        if dry_run {
            println!("  To resolve pending registrations:");
            println!("    Ensure Conduit is running: ./scripts/conduit/conduit-docker.sh start");
            println!("    Then start hKask normally: kask chat");
            println!("    (Pending registrations retry automatically on session start)");
            println!();
            println!("  To clear failed markers (permanent failures):");
            for agent_dir in discover_agent_dirs() {
                if let Some(name) = agent_dir.file_name().and_then(|n| n.to_str()) {
                    let failed_key = format!("matrix-pod-failed-{}", name);
                    if keychain.retrieve_by_key(&failed_key).is_ok() {
                        println!("    kask keystore delete {}", failed_key);
                    }
                }
            }
        }
    }

    if broken.is_empty() && matrix_issues.is_empty() {
        println!("\nAll databases are healthy — nothing to repair.");
        return;
    }

    if broken.is_empty() {
        // Only Matrix issues, no broken databases.
        return;
    }

    println!(
        "\n{} database(s) are unreadable with the current passphrase.",
        broken.len()
    );

    if dry_run {
        println!("Dry run — no changes made. To fix:");
        println!("  kask repair          (interactive prompt)");
        println!("  kask repair --force  (delete all broken databases)");
        return;
    }

    if !force {
        println!("\nThese databases will be DELETED and regenerated on next startup.");
        println!("This is safe — they only contain cache/state, not user data.");
        println!();
        print!("Delete {} broken database(s)? [y/N] ", broken.len());
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            eprintln!("Failed to read input.");
            return;
        }
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled — no databases deleted.");
            println!("To recover old data, set HKASK_DB_PASSPHRASE to your previous passphrase.");
            return;
        }
    }

    let mut deleted = 0;
    for db_path in &broken {
        let path_str = db_path.to_string_lossy();
        let salt_path = format!("{}.salt", path_str);

        if let Err(e) = std::fs::remove_file(db_path) {
            eprintln!("  Failed to delete {}: {}", path_str, e);
        } else {
            println!("  Deleted: {}", path_str);
            deleted += 1;
        }

        // Also remove the salt file so a fresh salt is generated on next open.
        let salt = std::path::Path::new(&salt_path);
        if salt.is_file() {
            let _ = std::fs::remove_file(salt);
        }
    }

    println!(
        "\nRepaired: {} database(s) deleted. They will be regenerated on next startup.",
        deleted
    );
    println!("Run 'kask chat' to start fresh.");
}
