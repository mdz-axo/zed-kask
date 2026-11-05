//! The served fleet — the shipped model briefs (M9's fleet layer).
//!
//! The fleet is the SET of briefs the private local leg serves: which
//! models ship with the binary, projected into model cards for the M3
//! `openai_compatible` settings entry. It is the model-side data
//! counterpart of the engine manifest's surface pin: the list changes
//! when models join, and a model joins the fleet by a brief TOML under
//! `kask/registry/model-briefs/` plus one entry in
//! [`SHIPPED_FLEET`] — the D1 template-shipping precedent
//! (registry data ships with the binary, so a missing brief is a
//! compile error, not a runtime hole).
//!
//! Each entry also carries the model's LOCAL artifact location relative
//! to the artifacts models root (`{artifacts_dir}/private-inference/
//! models/`) — the supervisor resolves it against the live root at
//! launch, fail-closed (a missing artifact is a typed error naming the
//! path, never a glob guess).

use std::path::{Path, PathBuf};

use super::model_brief::{ModelBrief, ModelBriefError};

/// The local artifact location for one fleet model — relative to the
/// artifacts models root. `artifact` is either a single GGUF file
/// (dense models) or the shard directory (the resolver lists and sorts
/// the `.gguf` files inside; the first is the entry shard — the same
/// convention the live-shard fixture pins).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelLocation {
    pub dir: &'static str,
    pub artifact: &'static str,
}

/// One shipped fleet model: the decoded brief plus its local location.
#[derive(Debug, Clone)]
pub struct ShippedModel {
    pub brief: ModelBrief,
    pub location: LocalModelLocation,
}

/// The shipped fleet — one entry per served model: the registry brief
/// (embedded at compile time) plus its local artifact location. Adding
/// a model is a data edit: the brief TOML plus one entry here; zero
/// changes anywhere else.
pub const SHIPPED_FLEET: &[(&str, &str, LocalModelLocation)] = &[
    (
        "qwen3-8-flash-next",
        include_str!("../../../../registry/model-briefs/qwen3-8-flash-next.toml"),
        LocalModelLocation {
            dir: "qwen38-flash-next",
            artifact: "UD-Q2_K_XL",
        },
    ),
    (
        "gemma-4-31b",
        include_str!("../../../../registry/model-briefs/gemma-4-31b.toml"),
        LocalModelLocation {
            dir: "gemma4",
            artifact: "gemma-4-31B-it-UD-Q4_K_XL.gguf",
        },
    ),
];

/// Fleet projection failures — per-variant, never a blanket string
/// (the string-errors discipline).
#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    /// A shipped brief failed to decode. The whole projection fails
    /// closed — the caller surfaces the error and emits nothing, never
    /// a partial fleet.
    #[error("shipped model brief `{model_id}` failed to decode: {source}")]
    ShippedDecode {
        model_id: String,
        source: ModelBriefError,
    },
    /// The requested model is not in the shipped fleet — the cards
    /// surface only fleet members, so a non-fleet id reaching here is
    /// a divergence between the entry the PI selected and the fleet.
    #[error("`{model_id}` is not in the shipped fleet")]
    UnknownModel { model_id: String },
    /// The model's local artifact is missing at the live root — the
    /// acquisition state and the fleet data disagree. Fail-closed,
    /// naming the exact path.
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

/// Decode every shipped brief through the one decode home
/// (`ModelBrief::decode`).
pub fn shipped_models() -> Result<Vec<ShippedModel>, FleetError> {
    SHIPPED_FLEET
        .iter()
        .map(|(model_id, source, location)| {
            ModelBrief::decode(source)
                .map(|brief| ShippedModel {
                    brief,
                    location: location.clone(),
                })
                .map_err(|source| FleetError::ShippedDecode {
                    model_id: (*model_id).to_string(),
                    source,
                })
        })
        .collect()
}

/// Look up one shipped model by its brief's `model_id` (the card name
/// and the served alias).
pub fn shipped_model(model_id: &str) -> Result<ShippedModel, FleetError> {
    shipped_models()?
        .into_iter()
        .find(|model| model.brief.model_id == model_id)
        .ok_or_else(|| FleetError::UnknownModel {
            model_id: model_id.to_string(),
        })
}

/// Resolve a fleet model's GGUF files under the live artifacts models
/// root (`{artifacts_dir}/private-inference/models/`). A file artifact
/// resolves to itself; a directory artifact resolves to its sorted
/// `.gguf` shard list (the first is the entry shard). Fail-closed: a
/// missing dir or artifact is a typed error naming the exact path.
pub fn resolve_model_files(models_root: &Path, model_id: &str) -> Result<Vec<PathBuf>, FleetError> {
    let model = shipped_model(model_id)?;
    let path = models_root
        .join(model.location.dir)
        .join(model.location.artifact);
    let meta = std::fs::metadata(&path).map_err(|source| FleetError::ArtifactNotFound {
        model_id: model_id.to_string(),
        path: path.to_string_lossy().to_string(),
        source,
    })?;
    if meta.is_file() {
        return Ok(vec![path]);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|source| FleetError::ArtifactNotFound {
            model_id: model_id.to_string(),
            path: path.to_string_lossy().to_string(),
            source,
        })?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "gguf"))
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

    #[test]
    fn shipped_briefs_decode_and_carry_card_data() {
        let fleet = shipped_models().expect("every shipped brief decodes");
        assert_eq!(fleet.len(), SHIPPED_FLEET.len());
        for (model, (model_id, _, _)) in fleet.iter().zip(SHIPPED_FLEET) {
            // The drift pin: the brief's own id must equal the registry
            // entry's name — a renamed file or a copied brief surfaces
            // here, not as a mismatched model card.
            assert_eq!(&model.brief.model_id, model_id);
            // Card data: the model card renders display_name and
            // max_tokens (the recommended context length) — a brief
            // without them cannot produce a card.
            assert!(!model.brief.display_name.is_empty());
            assert!(model.brief.recommendations.context_length > 0);
        }
    }

    #[test]
    fn fleet_ids_are_unique() {
        let fleet = shipped_models().expect("fleet decodes");
        let mut ids: Vec<&str> = fleet.iter().map(|m| m.brief.model_id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate model_id in the shipped fleet");
    }

    #[test]
    fn unknown_model_id_fails_closed() {
        let err = shipped_model("not-a-fleet-model").expect_err("a non-fleet id must refuse");
        assert!(matches!(err, FleetError::UnknownModel { .. }));
        let err = resolve_model_files(Path::new("/nonexistent"), "not-a-fleet-model")
            .expect_err("resolution requires a fleet member first");
        assert!(matches!(err, FleetError::UnknownModel { .. }));
    }

    #[test]
    fn file_artifact_resolves_to_itself_and_missing_artifacts_fail_closed() {
        let root = tempfile::tempdir().expect("tempdir");

        // Missing dir → typed ArtifactNotFound naming the path.
        let err = resolve_model_files(root.path(), "gemma-4-31b").expect_err("missing artifact");
        match &err {
            FleetError::ArtifactNotFound { path, .. } => {
                assert!(path.contains("gemma4"), "the error names the path: {path}");
            }
            other => panic!("missing artifact is ArtifactNotFound, got {other:?}"),
        }

        // Single-file artifact: the file itself, verbatim.
        std::fs::create_dir_all(root.path().join("gemma4")).expect("mkdir");
        std::fs::write(
            root.path().join("gemma4/gemma-4-31B-it-UD-Q4_K_XL.gguf"),
            b"",
        )
        .expect("write");
        let files = resolve_model_files(root.path(), "gemma-4-31b").expect("file resolves");
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0],
            root.path().join("gemma4/gemma-4-31B-it-UD-Q4_K_XL.gguf")
        );
    }

    #[test]
    fn shard_dir_artifact_resolves_to_the_sorted_shard_list() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("qwen38-flash-next/UD-Q2_K_XL")).expect("mkdir");
        for name in [
            "Qwen3.8-Flash-Next-UD-Q2_K_XL-00003-of-00004.gguf",
            "Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf",
            "download.log",
        ] {
            std::fs::write(
                root.path().join("qwen38-flash-next/UD-Q2_K_XL").join(name),
                b"",
            )
            .expect("write");
        }
        let files = resolve_model_files(root.path(), "qwen3-8-flash-next").expect("shards resolve");
        // Only the .gguf files, sorted — the first is the entry shard
        // (the live-shard fixture convention); non-GGUF files in the
        // shard dir are ignored, not guessed at.
        assert_eq!(files.len(), 2);
        assert!(files[0].to_string_lossy().contains("00001-of-00004"));
        assert!(files[1].to_string_lossy().contains("00003-of-00004"));

        // A shard dir with no .gguf files is a broken fixture — typed
        // refusal, never a pass.
        std::fs::remove_file(root.path().join(
            "qwen38-flash-next/UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf",
        ))
        .expect("rm");
        std::fs::remove_file(root.path().join(
            "qwen38-flash-next/UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00003-of-00004.gguf",
        ))
        .expect("rm");
        let err = resolve_model_files(root.path(), "qwen3-8-flash-next").expect_err("no shards");
        assert!(matches!(err, FleetError::NoShards { .. }));
    }
}
