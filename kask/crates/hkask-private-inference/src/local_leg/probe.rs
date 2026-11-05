//! The startup probe — the sense step of the launch gate (plan §10
//! delta 8). `--version` and `--help` are parsed against the manifest's
//! structured pin and flag vocabulary; any mismatch is a typed
//! refusal, never a wrong-flag launch. The manifest is never trusted
//! alone: the probe repairs the model against the installed binary
//! (Good Regulator).
//!
//! Version semantics are exact-commit (slice plan D-S2): commit hashes
//! are not orderable, so "older than" is not computable — any commit
//! other than the pinned one is an unverified surface, newer included.

use std::path::Path;

use crate::local_leg::manifest::EngineSurfaceManifest;

/// A successful probe: what the binary reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub version: String,
    pub commit: String,
}

/// Probe failures — per-variant, never a blanket string.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// The binary could not be executed at all.
    #[error("the engine binary probe failed to run {binary}: {source}")]
    ExecFailed {
        binary: String,
        source: std::io::Error,
    },
    /// The `--version` output does not carry the parsable shape.
    #[error("the engine binary's --version output is unparsable: {output:?}")]
    UnparsableVersion { output: String },
    /// The binary's commit is not the pinned one — exact-commit refusal
    /// (D-S2), naming both.
    #[error(
        "engine commit mismatch: binary {binary_commit} is not the pinned \
         {pinned_commit} — exact-commit refusal (a different commit is an \
         unverified surface, newer included)"
    )]
    CommitMismatch {
        binary_commit: String,
        pinned_commit: String,
    },
    /// The binary lacks flag spellings the pinned surface requires —
    /// naming the missing spellings and the manifest's commit.
    #[error(
        "the engine binary lacks flag spellings the pinned surface \
         requires: {missing:?} (manifest commit {manifest_commit})"
    )]
    MissingFlags {
        missing: Vec<String>,
        manifest_commit: String,
    },
}

/// Parse a `--version` line of the pinned build's shape:
/// `version: 0.9.0-dev (build 1, commit 9e2a04a)` → the version and
/// the commit token. Verified live against the installed build
/// (2026-11-04).
pub fn parse_version_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix("version:")?.trim();
    let (version, tail) = rest.split_once('(')?;
    let version = version.trim().to_string();
    let commit = tail
        .split(',')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("commit "))
        .map(|rest| rest.trim_end_matches(')').to_string())?;
    if version.is_empty() || commit.is_empty() {
        return None;
    }
    Some((version, commit))
}

/// Every flag spelling the plan emitter can emit for this surface
/// (the vocabulary values; the backend-agnostic `binary` name is not a
/// flag spelling). Multi-spelling intents (`kv_cache_type`) contribute
/// each element.
pub fn required_flag_spellings(manifest: &EngineSurfaceManifest) -> Vec<&str> {
    let flags = &manifest.flags;
    let mut wanted: Vec<&str> = vec![
        &flags.model,
        &flags.n_gpu_layers,
        &flags.cpu_moe,
        &flags.override_tensor,
        &flags.ctx_size,
        &flags.flash_attn,
        &flags.host,
        &flags.port,
        &flags.threads,
        &flags.alias,
        &flags.api_key_file,
    ];
    wanted.extend(flags.kv_cache_type.iter().map(String::as_str));
    wanted
}

/// Whether `help` truly contains `spelling` as a flag: the match must
/// end at a non-identifier boundary. A plain substring check passes a
/// rename that EXTENDS the old spelling (`--flash-attn` →
/// `--flash-attention` still contains the old substring) — the exact
/// boundary rule is what makes the probe catch that drift class
/// (found by the probe's own falsifier test).
fn contains_spelling(help: &str, spelling: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = help[from..].find(spelling) {
        let end = from + pos + spelling.len();
        let boundary_ok = help[end..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '-' || c == '_'));
        if boundary_ok {
            return true;
        }
        from = from + pos + 1;
    }
    false
}

/// The spellings absent from a `--help` text — the refusal payload.
pub fn missing_flag_spellings(help_text: &str, manifest: &EngineSurfaceManifest) -> Vec<String> {
    required_flag_spellings(manifest)
        .into_iter()
        .filter(|spelling| !contains_spelling(help_text, spelling))
        .map(str::to_string)
        .collect()
}

/// Probe the installed binary against the manifest: `--version` parsed
/// and compared to the pin (exact-commit), `--help` scanned for every
/// required spelling. The version line is searched in stdout then
/// stderr (builds vary in where they print it).
pub async fn probe_binary(
    binary: &Path,
    manifest: &EngineSurfaceManifest,
) -> Result<ProbeReport, ProbeError> {
    let version_output = tokio::process::Command::new(binary)
        .arg("--version")
        .output()
        .await
        .map_err(|source| ProbeError::ExecFailed {
            binary: binary.to_string_lossy().to_string(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&version_output.stdout);
    let stderr = String::from_utf8_lossy(&version_output.stderr);
    let version_line = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| line.trim().starts_with("version:"))
        .ok_or_else(|| ProbeError::UnparsableVersion {
            output: format!("{stdout}{stderr}").trim().to_string(),
        })?;
    let (version, commit) =
        parse_version_line(version_line).ok_or_else(|| ProbeError::UnparsableVersion {
            output: version_line.trim().to_string(),
        })?;
    if commit != manifest.version_pin.commit {
        return Err(ProbeError::CommitMismatch {
            binary_commit: commit,
            pinned_commit: manifest.version_pin.commit.clone(),
        });
    }

    let help_output = tokio::process::Command::new(binary)
        .arg("--help")
        .output()
        .await
        .map_err(|source| ProbeError::ExecFailed {
            binary: binary.to_string_lossy().to_string(),
            source,
        })?;
    let help = String::from_utf8_lossy(&help_output.stdout);
    let missing = missing_flag_spellings(&help, manifest);
    if !missing.is_empty() {
        return Err(ProbeError::MissingFlags {
            missing,
            manifest_commit: manifest.version_pin.commit.clone(),
        });
    }

    Ok(ProbeReport { version, commit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_leg::manifest::shipped_manifest;

    /// The real observed `--version` line of the pinned build
    /// (verified live 2026-11-04 against build-hip).
    const OBSERVED_VERSION_LINE: &str = "version: 0.9.0-dev (build 1, commit 9e2a04a)";

    #[test]
    fn parses_the_observed_version_line() {
        assert_eq!(
            parse_version_line(OBSERVED_VERSION_LINE),
            Some(("0.9.0-dev".into(), "9e2a04a".into()))
        );
    }

    #[test]
    fn refuses_unparsable_version_lines() {
        assert_eq!(parse_version_line("llama-server, no version here"), None);
        // No commit token → no pin → refuse, never guess.
        assert_eq!(parse_version_line("version: 0.9.0-dev"), None);
        assert_eq!(parse_version_line("version:  (build 1, commit x)"), None);
    }

    #[test]
    fn the_shipped_surface_requires_the_supervisor_flags() {
        let manifest = shipped_manifest().expect("shipped manifest");
        let spellings = required_flag_spellings(&manifest);
        assert!(spellings.contains(&"--alias"));
        assert!(spellings.contains(&"--api-key-file"));
        assert!(spellings.contains(&"--cache-type-k"));
        assert!(spellings.contains(&"--cache-type-v"));
        assert!(
            !spellings.contains(&"llama-server"),
            "the binary name is not a flag"
        );
    }

    #[test]
    fn missing_spellings_are_exactly_the_absent_ones() {
        let manifest = shipped_manifest().expect("shipped manifest");
        // A help text carrying every spelling → nothing missing.
        let full_help = required_flag_spellings(&manifest).join("\n");
        assert!(missing_flag_spellings(&full_help, &manifest).is_empty());

        // A renamed flag (`--flash-attn` → `--flash-attention`) and a
        // dropped one (its line removed) surface exactly themselves —
        // the boundary rule is what catches the extension-rename (the
        // old substring survives inside the new spelling; a plain
        // contains() would pass it).
        let drifted: String = full_help
            .replace("--flash-attn", "--flash-attention")
            .lines()
            .filter(|line| *line != "--cpu-moe")
            .collect::<Vec<_>>()
            .join("\n");
        let drifted = drifted + "\n--cpu-moe-removed";
        let mut missing = missing_flag_spellings(&drifted, &manifest);
        missing.sort();
        assert_eq!(
            missing,
            vec!["--cpu-moe".to_string(), "--flash-attn".to_string()]
        );
    }
}
