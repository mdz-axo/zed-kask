//! The discovered fleet — the local leg's model catalog (operator
//! ruling 2026-11-08: no compile-time fleet list).
//!
//! The fleet is the SET of model briefs the private local leg serves.
//! A model joins the fleet by a brief TOML in the DISCOVERED briefs
//! directory (`{artifacts}/private-inference/model-briefs/`) — one
//! self-contained file carrying the placement policy AND the location
//! of its weights, so joining is a data act (drop the brief beside the
//! GGUF), never a code edit or a rebuild. The compiled-in
//! [`SEED_BRIEFS`] set is the SEED, not the source: the startup path
//! writes each seed brief when absent (write-only-if-absent — an
//! operator's hand-edited or hand-added file always wins, and a
//! deliberately deleted seed file returns at the next seeding; the D1
//! template-shipping precedent).
//!
//! Discovery reads the directory, decodes every brief, and joins each
//! with its artifact state — present models carry their resolved GGUF
//! files; missing ones stay carded and fail closed at launch (the
//! "downloaded = available" ruling). Fail-closed throughout: a brief
//! that fails to decode is a typed error naming the file, and the
//! catalog refuses partial loads — never a silent gap.

use std::path::{Path, PathBuf};

use super::model_brief::{ModelBrief, ModelBriefError};

/// The compiled-in seed — `(file name, brief TOML)`, written to the
/// discovered briefs directory when absent. Every entry must decode
/// (pinned by the seed test) and its file name must equal the brief's
/// `model_id` (pinned by discovery) — the compile-time guarantee of
/// the old `SHIPPED_FLEET` list, held by the test instead of the type.
pub const SEED_BRIEFS: &[(&str, &str)] = &[
    (
        "flint.toml",
        include_str!("../../../../registry/model-briefs/flint.toml"),
    ),
    (
        "gemma-4-31b.toml",
        include_str!("../../../../registry/model-briefs/gemma-4-31b.toml"),
    ),
    (
        "muse-glimmer-30b.toml",
        include_str!("../../../../registry/model-briefs/muse-glimmer-30b.toml"),
    ),
    (
        "qwen3-8-flash-next.toml",
        include_str!("../../../../registry/model-briefs/qwen3-8-flash-next.toml"),
    ),
    (
        "qwen36-35b-a3b.toml",
        include_str!("../../../../registry/model-briefs/qwen36-35b-a3b.toml"),
    ),
    (
        "qwen35-0.8b.toml",
        include_str!("../../../../registry/model-briefs/qwen35-0.8b.toml"),
    ),
    (
        "qwen35-0.8b-edited.toml",
        include_str!("../../../../registry/model-briefs/qwen35-0.8b-edited.toml"),
    ),
    (
        "ornith-1.5-9b.toml",
        include_str!("../../../../registry/model-briefs/ornith-1.5-9b.toml"),
    ),
    (
        "ornith-1.5-9b-q8.toml",
        include_str!("../../../../registry/model-briefs/ornith-1.5-9b-q8.toml"),
    ),
];

/// One discovered fleet model: the decoded brief plus its artifact state.
#[derive(Debug, Clone)]
pub struct FleetModel {
    pub brief: ModelBrief,
    /// The resolved GGUF files — `None` when the artifact is absent
    /// (carded, fails closed at launch; the "downloaded = available"
    /// ruling, operator 2026-11-05).
    pub artifact_files: Option<Vec<PathBuf>>,
}

/// Fleet discovery and resolution failures — per-variant, never a
/// blanket string (the string-errors discipline).
#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    /// The discovered briefs directory does not exist — the seeding
    /// path has not run (the startup task owns this directory; its
    /// absence is a wiring gap, never an empty fleet).
    #[error(
        "the discovered briefs directory {path} does not exist — the compiled-in \
         seed has not been written (seeding owns this directory)"
    )]
    BriefsDirMissing { path: String },
    /// A brief could not be read — the file is named.
    #[error("model brief `{file}` could not be read: {source}")]
    BriefRead {
        file: String,
        source: std::io::Error,
    },
    /// A brief failed to decode — the whole catalog refuses to load,
    /// never a partial fleet. A hand-edited brief surfaces here by
    /// name, not as a mystery model.
    #[error("model brief `{file}` failed to decode: {source}")]
    BriefDecode {
        file: String,
        source: ModelBriefError,
    },
    /// A brief's file name and its declared `model_id` disagree — the
    /// file IS the model's identity slot (two files cannot declare one
    /// id, so this check subsumes duplicate detection).
    #[error(
        "model brief `{file}` declares model_id `{brief_id}` but the file \
         names `{file_id}` — rename the file or fix the id"
    )]
    BriefIdDrift {
        file: String,
        file_id: String,
        brief_id: String,
    },
    /// The requested model is not in the discovered fleet — the briefs
    /// dir is named so the operator can see what IS there.
    #[error("`{model_id}` is not in the discovered fleet ({briefs_dir})")]
    UnknownModel {
        model_id: String,
        briefs_dir: String,
    },
    /// The model's local artifact is missing at the live root — the
    /// acquisition state and the brief disagree. Fail-closed, naming
    /// the exact path.
    #[error("local artifact for `{model_id}` not found at {path}: {source}")]
    ArtifactNotFound {
        model_id: String,
        path: String,
        source: std::io::Error,
    },
    /// A shard directory holds no `.gguf` files — a broken fixture,
    /// never a pass.
    #[error("no .gguf shards in {path}")]
    NoShards { path: String },
}

/// Seeding failures — typed, never a blanket string.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The briefs directory could not be created or written.
    #[error("seeding the briefs dir {path} failed: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// Write each compiled-in seed brief to `dir` when absent —
/// write-only-if-absent: an operator's hand-edited or hand-added file
/// always wins, and a deliberately deleted seed file returns at the
/// next seeding (the directory is system-owned like the seeded
/// templates, D1). Returns the number of briefs written (0 = every
/// seed already present).
pub fn seed_briefs(dir: &Path) -> Result<usize, SeedError> {
    std::fs::create_dir_all(dir).map_err(|source| SeedError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let mut written = 0;
    for (file, toml) in SEED_BRIEFS {
        let path = dir.join(file);
        if !path.exists() {
            std::fs::write(&path, toml).map_err(|source| SeedError::Io {
                path: path.display().to_string(),
                source,
            })?;
            written += 1;
        }
    }
    Ok(written)
}

/// Discover the fleet: decode every `*.toml` under the briefs dir
/// (sorted for determinism), enforce the file-name/model-id identity,
/// and join each brief with its artifact state under the models root.
/// All-or-nothing — any failure refuses the whole catalog, never a
/// partial fleet.
pub fn discover(briefs_dir: &Path, models_root: &Path) -> Result<Vec<FleetModel>, FleetError> {
    let entries = std::fs::read_dir(briefs_dir).map_err(|_| FleetError::BriefsDirMissing {
        path: briefs_dir.display().to_string(),
    })?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();

    let mut models = Vec::with_capacity(files.len());
    for path in files {
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let toml = std::fs::read_to_string(&path).map_err(|source| FleetError::BriefRead {
            file: file.clone(),
            source,
        })?;
        let brief = ModelBrief::decode(&toml).map_err(|source| FleetError::BriefDecode {
            file: file.clone(),
            source,
        })?;
        let file_id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_id != brief.model_id {
            return Err(FleetError::BriefIdDrift {
                file,
                file_id,
                brief_id: brief.model_id,
            });
        }
        // The artifact join: a missing artifact (or an empty shard dir)
        // is the carded-but-missing state, never a discovery failure —
        // launch re-resolves and fails closed there. Any other
        // resolution error is a real failure and refuses the catalog.
        let artifact_files = match resolve_model_files(models_root, &brief) {
            Ok(files) => Some(files),
            Err(FleetError::ArtifactNotFound { .. }) | Err(FleetError::NoShards { .. }) => None,
            Err(other) => return Err(other),
        };
        models.push(FleetModel {
            brief,
            artifact_files,
        });
    }
    Ok(models)
}

/// Discover one fleet model by id — the launch path's resolution
/// (the full [`FleetModel`]: the brief plus the artifact state, so the
/// launch's missing-artifact refusal stays typed).
pub fn discover_model(
    briefs_dir: &Path,
    models_root: &Path,
    model_id: &str,
) -> Result<FleetModel, FleetError> {
    discover(briefs_dir, models_root)?
        .into_iter()
        .find(|model| model.brief.model_id == model_id)
        .ok_or_else(|| FleetError::UnknownModel {
            model_id: model_id.to_string(),
            briefs_dir: briefs_dir.display().to_string(),
        })
}

/// Resolve a brief's GGUF files under the live artifacts models root.
/// A file artifact resolves to itself; a directory artifact resolves
/// to its sorted `.gguf` shard list (the first is the entry shard — the
/// live-shard fixture convention). Fail-closed: a missing dir or
/// artifact is a typed error naming the exact path.
pub fn resolve_model_files(
    models_root: &Path,
    brief: &ModelBrief,
) -> Result<Vec<PathBuf>, FleetError> {
    let path = models_root
        .join(&brief.location.dir)
        .join(&brief.location.artifact);
    let meta = std::fs::metadata(&path).map_err(|source| FleetError::ArtifactNotFound {
        model_id: brief.model_id.clone(),
        path: path.to_string_lossy().to_string(),
        source,
    })?;
    if meta.is_file() {
        return Ok(vec![path]);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|source| FleetError::ArtifactNotFound {
            model_id: brief.model_id.clone(),
            path: path.to_string_lossy().to_string(),
            source,
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "gguf"))
        .collect();
    if files.is_empty() {
        return Err(FleetError::NoShards {
            path: path.to_string_lossy().to_string(),
        });
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_briefs_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        seed_briefs(dir.path()).expect("the seed set writes");
        dir
    }

    fn hand_brief(model_id: &str) -> String {
        format!(
            "model_id = \"{model_id}\"\ndisplay_name = \"{model_id}\"\n\n\
             [location]\ndir = \"{model_id}\"\nartifact = \"weights.gguf\"\n\n\
             [model]\ntotal_params = 1_000_000_000\nactive_params = 1_000_000_000\n\
             expert_count = 0\nexpert_used_count = 0\n\n\
             [placement]\nple_pin_pattern = \"\"\n\n\
             [recommendations]\nkv_cache_type = \"q8_0\"\nflash_attn = true\n\
             context_length = 65_536\n"
        )
    }

    #[test]
    fn every_seed_brief_decodes() {
        for (file, toml) in SEED_BRIEFS {
            let brief = ModelBrief::decode(toml)
                .unwrap_or_else(|err| panic!("seed brief {file} decodes: {err}"));
            assert_eq!(
                &brief.model_id,
                file.trim_end_matches(".toml"),
                "the seed file name IS the model id"
            );
            assert!(
                !brief.display_name.is_empty() && brief.recommendations.context_length > 0,
                "{file} carries card data"
            );
        }
    }

    #[test]
    fn seeding_writes_only_if_absent_and_operator_files_win() {
        let dir = tempfile::tempdir().expect("tempdir");

        // First seeding writes the whole seed set; re-seeding writes
        // nothing — write-only-if-absent.
        assert_eq!(
            seed_briefs(dir.path()).expect("first seed"),
            SEED_BRIEFS.len(),
            "an empty dir takes the whole seed set"
        );
        assert_eq!(
            seed_briefs(dir.path()).expect("re-seed"),
            0,
            "a present seed file is never overwritten"
        );

        // An operator's hand edit always wins.
        let edited = dir.path().join("flint.toml");
        let operator_text = "# the operator's own brief\n";
        std::fs::write(&edited, operator_text).expect("operator edit");
        assert_eq!(seed_briefs(dir.path()).expect("seed over the edit"), 0);
        assert_eq!(
            std::fs::read_to_string(&edited).expect("read"),
            operator_text,
            "the operator's hand edit survives seeding"
        );

        // A deliberately deleted seed file returns at the next
        // seeding — the directory is system-owned like the templates.
        std::fs::remove_file(&edited).expect("delete");
        assert_eq!(seed_briefs(dir.path()).expect("seed after delete"), 1);
        assert_eq!(
            std::fs::read_to_string(&edited).expect("read"),
            *SEED_BRIEFS
                .iter()
                .find(|(file, _)| *file == "flint.toml")
                .map(|(_, toml)| toml)
                .expect("flint is a seed brief"),
            "the deleted seed brief returns byte-identical"
        );
    }

    #[test]
    fn seeded_briefs_discover_to_the_seed_fleet() {
        let briefs = seeded_briefs_dir();
        let models_root = tempfile::tempdir().expect("tempdir");
        let fleet = discover(briefs.path(), models_root.path()).expect("the seeded dir discovers");

        let mut discovered: Vec<&str> = fleet.iter().map(|m| m.brief.model_id.as_str()).collect();
        let mut seeded: Vec<&str> = SEED_BRIEFS
            .iter()
            .map(|(file, _)| file.trim_end_matches(".toml"))
            .collect();
        discovered.sort_unstable();
        seeded.sort_unstable();
        assert_eq!(discovered, seeded, "discovery mirrors the seed set");

        // No weights under the temp models root: every model is carded
        // but missing — the "downloaded = available" state.
        for model in &fleet {
            assert!(
                model.artifact_files.is_none(),
                "{} is carded with a missing artifact",
                model.brief.model_id
            );
        }
    }

    #[test]
    fn an_operator_added_brief_joins_the_catalog() {
        let briefs = seeded_briefs_dir();
        std::fs::write(
            briefs.path().join("hand-added.toml"),
            hand_brief("hand-added"),
        )
        .expect("operator brief");
        let models_root = tempfile::tempdir().expect("tempdir");
        let fleet = discover(briefs.path(), models_root.path()).expect("discovery");
        assert!(
            fleet
                .iter()
                .any(|model| model.brief.model_id == "hand-added"),
            "a hand-added brief joins the fleet with no code change"
        );
    }

    #[test]
    fn discovery_fails_closed_naming_the_corrupt_file() {
        let briefs = seeded_briefs_dir();
        std::fs::write(briefs.path().join("broken.toml"), "not a brief").expect("corrupt");
        let models_root = tempfile::tempdir().expect("tempdir");
        let err = discover(briefs.path(), models_root.path())
            .expect_err("a corrupt brief refuses the whole catalog");
        match err {
            FleetError::BriefDecode { file, .. } => {
                assert_eq!(file, "broken.toml", "the error names the file")
            }
            other => panic!("a corrupt brief is BriefDecode, got {other:?}"),
        }
    }

    #[test]
    fn brief_file_name_must_match_the_model_id() {
        let briefs = seeded_briefs_dir();
        std::fs::write(
            briefs.path().join("renamed.toml"),
            hand_brief("not-renamed"),
        )
        .expect("drift");
        let models_root = tempfile::tempdir().expect("tempdir");
        let err =
            discover(briefs.path(), models_root.path()).expect_err("id drift refuses the catalog");
        match err {
            FleetError::BriefIdDrift {
                file,
                file_id,
                brief_id,
                ..
            } => {
                assert_eq!(file, "renamed.toml");
                assert_eq!(file_id, "renamed");
                assert_eq!(brief_id, "not-renamed");
            }
            other => panic!("id drift is BriefIdDrift, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_briefs_dir_fails_typed_and_an_empty_dir_discovers_empty() {
        let models_root = tempfile::tempdir().expect("tempdir");

        let err = discover(Path::new("/nonexistent-briefs"), models_root.path())
            .expect_err("a missing briefs dir is a wiring gap, not an empty fleet");
        assert!(matches!(err, FleetError::BriefsDirMissing { .. }));

        let empty = tempfile::tempdir().expect("tempdir");
        let fleet = discover(empty.path(), models_root.path()).expect("empty dir discovers");
        assert!(fleet.is_empty(), "an explicitly emptied dir serves nothing");

        let err = discover_model(empty.path(), models_root.path(), "any").expect_err("no members");
        assert!(matches!(err, FleetError::UnknownModel { .. }));
    }

    #[test]
    fn file_artifact_resolves_to_itself_and_missing_artifacts_fail_closed() {
        let root = tempfile::tempdir().expect("tempdir");
        let brief = ModelBrief::decode(&hand_brief("gemma-4-31b")).expect("brief");

        // Missing dir → typed ArtifactNotFound naming the path.
        let err = resolve_model_files(root.path(), &brief).expect_err("missing artifact");
        match &err {
            FleetError::ArtifactNotFound { path, .. } => {
                assert!(
                    path.contains("gemma-4-31b"),
                    "the error names the path: {path}"
                );
            }
            other => panic!("missing artifact is ArtifactNotFound, got {other:?}"),
        }

        // Single-file artifact: the file itself, verbatim.
        std::fs::create_dir_all(root.path().join("gemma-4-31b")).expect("mkdir");
        std::fs::write(root.path().join("gemma-4-31b/weights.gguf"), b"").expect("write");
        let files = resolve_model_files(root.path(), &brief).expect("file resolves");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], root.path().join("gemma-4-31b/weights.gguf"));
    }

    #[test]
    fn shard_dir_artifact_resolves_to_the_sorted_shard_list() {
        let root = tempfile::tempdir().expect("tempdir");
        let toml = hand_brief("qwen3-8-flash-next").replace("weights.gguf", "UD-Q2_K_XL");
        let brief = ModelBrief::decode(&toml).expect("brief");
        std::fs::create_dir_all(root.path().join("qwen3-8-flash-next/UD-Q2_K_XL")).expect("mkdir");
        for name in [
            "Qwen3.8-Flash-Next-UD-Q2_K_XL-00003-of-00004.gguf",
            "Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf",
            "download.log",
        ] {
            std::fs::write(
                root.path().join("qwen3-8-flash-next/UD-Q2_K_XL").join(name),
                b"",
            )
            .expect("write");
        }
        let files = resolve_model_files(root.path(), &brief).expect("shards resolve");
        // Only the .gguf files, sorted — the first is the entry shard;
        // non-GGUF files in the shard dir are ignored, not guessed at.
        assert_eq!(files.len(), 2);
        assert!(files[0].to_string_lossy().contains("00001-of-00004"));
        assert!(files[1].to_string_lossy().contains("00003-of-00004"));

        // A shard dir with no .gguf files is a broken fixture — typed
        // refusal, never a pass.
        std::fs::remove_file(root.path().join(
            "qwen3-8-flash-next/UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf",
        ))
        .expect("rm");
        std::fs::remove_file(root.path().join(
            "qwen3-8-flash-next/UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00003-of-00004.gguf",
        ))
        .expect("rm");
        let err = resolve_model_files(root.path(), &brief).expect_err("no shards");
        assert!(matches!(err, FleetError::NoShards { .. }));
    }
}
