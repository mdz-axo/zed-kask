use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{AgentTool, ToolCallEventStream, ToolInput, ToolPermissionContext};
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result, anyhow, bail};
use gpui::{App, Entity, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use project::{Project, project_settings::ProjectSettings as WorktreeSettings};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings as _;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use super::tool_permissions::{
    ResolvedProjectPath, canonicalize_worktree_roots, resolve_project_path,
};

const MAX_OUTPUT: usize = 64 * 1024;
const MAX_SOURCE: u64 = 1024 * 1024;
const RUN_LIMIT: Duration = Duration::from_secs(30);

/// Check a saved local .lean file using the project's pinned Lake toolchain.
/// This runs project code; every invocation requires the user's approval.
/// A successful Lean exit is NOT a claim of axiom freedom. Supply `theorem`
/// to audit that named declaration with `#print axioms`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LeanCheckToolInput {
    /// Project-relative saved .lean path (include the worktree name if needed).
    path: String,
    /// Optional fully qualified theorem name to audit for axioms.
    #[serde(default)]
    theorem: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeanCheckToolOutput {
    pub lean_version: String,
    pub diagnostics: Vec<serde_json::Value>,
    pub goal_text: Vec<String>,
    pub completion_status: String,
    pub axioms: Option<String>,
    pub exit_code: Option<i32>,
}

impl From<LeanCheckToolOutput> for LanguageModelToolResultContent {
    fn from(value: LeanCheckToolOutput) -> Self {
        match serde_json::to_string_pretty(&value) {
            Ok(json) => json.into(),
            Err(error) => format!("Lean check result serialization failed: {error}").into(),
        }
    }
}

pub struct LeanCheckTool {
    project: Entity<Project>,
}

impl LeanCheckTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.split('.').all(|part| {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '\'')
    }) && !name.is_empty()
    {
        Ok(())
    } else {
        bail!("theorem must be a plain qualified Lean identifier")
    }
}

fn validate_relative(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.extension().is_none_or(|ext| ext != "lean")
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        bail!("path must be a project-relative saved .lean file without traversal")
    }
    Ok(())
}

fn validate_saved_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = std::fs::canonicalize(root).context("cannot resolve local worktree root")?;
    let file = std::fs::canonicalize(path).context("saved Lean file is missing or inaccessible")?;
    if !file.starts_with(&root) || !file.is_file() {
        bail!(
            "Lean file must be a saved regular file inside the local worktree (no symlink escape)"
        )
    }
    if file.metadata()?.len() > MAX_SOURCE {
        bail!("Lean file exceeds 1 MiB limit")
    }
    Ok(file)
}

fn project_root(root: &Path, file: &Path) -> Result<PathBuf> {
    let mut dir = file.parent();
    while let Some(current) = dir {
        if !current.starts_with(root) {
            break;
        }
        if current.join("lakefile.lean").is_file() || current.join("lakefile.toml").is_file() {
            if !current.join("lean-toolchain").is_file() {
                bail!(
                    "Lake project at {} has no lean-toolchain; pin Lean before checking",
                    current.display()
                )
            }
            return Ok(current.to_path_buf());
        }
        dir = current.parent();
    }
    bail!(
        "No Lake project with a lean-toolchain found for this file; add lakefile.lean (or lakefile.toml) and lean-toolchain"
    )
}

async fn read_bounded<R: AsyncRead + Unpin>(mut reader: R) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if out.len() + n > MAX_OUTPUT {
            bail!("Lean output exceeded 64 KiB")
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

async fn invoke_lake(
    lake: &Path,
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<(Option<i32>, String, String)> {
    let mut command = Command::new(lake);
    command
        .args(["env", "lean"])
        .args(args)
        .current_dir(root)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if input.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command.spawn().with_context(|| format!("Cannot start Lake; install elan/Lake and the project's lean-toolchain (executable: {})", lake.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Lake stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Lake stderr unavailable"))?;
    let execution = async {
        let write = async {
            if let Some(bytes) = input {
                let mut stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow!("Lake stdin unavailable"))?;
                stdin.write_all(bytes).await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        let ((), out, err, status) =
            tokio::try_join!(write, read_bounded(stdout), read_bounded(stderr), async {
                Ok::<_, anyhow::Error>(child.wait().await?)
            })?;
        Ok::<_, anyhow::Error>((
            status.code(),
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        ))
    };
    tokio::time::timeout(RUN_LIMIT, execution)
        .await
        .context("Lake/Lean timed out after 30 seconds")?
}

// `lake env lean --json` emits one diagnostic per line. Other stdout
// (notably #print axioms and goal text) is retained rather than discarded.
fn parse_output(stdout: &str, stderr: &str) -> (Vec<serde_json::Value>, Vec<String>) {
    let mut diagnostics = Vec::new();
    let mut text = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.is_object() {
                diagnostics.push(value);
                continue;
            }
        }
        if !line.trim().is_empty() {
            text.push(line.to_string());
        }
    }
    (diagnostics, text)
}

// Called only after explicit user approval. The optional audit uses stdin
// containing exactly the saved bytes plus one validated #print command;
// this avoids writing a synthetic .lean file into the worktree.
async fn check_saved(
    root: PathBuf,
    file: PathBuf,
    theorem: Option<String>,
    lake: &Path,
) -> Result<LeanCheckToolOutput> {
    let file = validate_saved_path(&root, &file)?;
    let root = std::fs::canonicalize(root)?;
    let project = project_root(&root, &file)?;
    let (version_code, version, version_err) =
        invoke_lake(lake, &project, &["--version"], None).await?;
    if version_code != Some(0) {
        bail!("Lake could not select the pinned Lean toolchain: {version_err}")
    }
    let version = version.trim().to_string();
    if version.is_empty() {
        bail!("Lake returned no Lean version")
    }
    let relative = file.strip_prefix(&project)?;
    let relative = relative
        .to_str()
        .ok_or_else(|| anyhow!("Lean file path is not UTF-8"))?;
    let (code, stdout, stderr) = if let Some(name) = theorem.as_deref() {
        validate_name(name)?;
        let mut source = tokio::fs::read(&file).await?;
        source.extend_from_slice(format!("\n#print axioms {name}\n").as_bytes());
        invoke_lake(lake, &project, &["--json", "--stdin"], Some(&source)).await?
    } else {
        invoke_lake(lake, &project, &["--json", relative], None).await?
    };
    let (diagnostics, text) = parse_output(&stdout, &stderr);
    let audit = theorem.as_deref().and_then(|name| {
        diagnostics
            .iter()
            .filter_map(|v| v.get("data")?.as_str())
            .chain(text.iter().map(String::as_str))
            .find(|s| {
                s.contains(&format!("'{name}' does not depend on any axioms"))
                    || s.contains(&format!("'{name}' depends on axioms:"))
            })
    });
    let has_warning = diagnostics
        .iter()
        .any(|v| v["severity"] == "warning" || v["severity"] == "error")
        || text
            .iter()
            .any(|s| s.contains("warning:") || s.contains("error:"));
    let completion_status = if code != Some(0) {
        "failed"
    } else if audit.is_some_and(|s| s.contains("depends on axioms:")) {
        "axioms_present"
    } else if has_warning {
        "warnings"
    } else if audit.is_some() {
        "axiom_free"
    } else {
        "checked_not_axiom_audited"
    };
    let goal_text = diagnostics
        .iter()
        .filter_map(|v| v.get("data")?.as_str())
        .filter(|s| s.contains('⊢') || s.contains("unsolved goals"))
        .map(str::to_string)
        .collect();
    Ok(LeanCheckToolOutput {
        lean_version: version,
        diagnostics,
        goal_text,
        completion_status: completion_status.into(),
        axioms: audit.map(str::to_string),
        exit_code: code,
    })
}

impl AgentTool for LeanCheckTool {
    type Input = LeanCheckToolInput;
    type Output = LeanCheckToolOutput;
    const NAME: &'static str = "lean_check";
    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }
    fn allow_in_restricted_mode() -> bool {
        false
    }
    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        input
            .map(|v| format!("Check Lean: {}", v.path).into())
            .unwrap_or_else(|_| "Check Lean".into())
    }
    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, LanguageModelToolResultContent>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|e| e.to_string().into())?;
            validate_relative(&input.path).map_err(|e| e.to_string().into())?;
            if let Some(name) = &input.theorem {
                validate_name(name).map_err(|e| e.to_string().into())?;
            }
            let fs = project.read_with(cx, |project, _| project.fs().clone());
            let roots = canonicalize_worktree_roots(&project, &fs, cx).await;
            let (root, file) = project
                .read_with(cx, |project, cx| {
                    if !project.is_local() {
                        bail!("Lean check requires a local worktree")
                    }
                    let path = match resolve_project_path(project, &input.path, &roots, cx)? {
                        ResolvedProjectPath::Safe(path) => path,
                        ResolvedProjectPath::SymlinkEscape { .. } => {
                            bail!("Lean check refuses symlink escapes")
                        }
                    };
                    for settings in [
                        WorktreeSettings::get_global(cx),
                        WorktreeSettings::get(Some((&path).into()), cx),
                    ] {
                        if settings.is_path_excluded(&path.path)
                            || settings.is_path_private(&path.path)
                        {
                            bail!("Lean check refuses private or file-scan-excluded paths")
                        }
                    }
                    let worktree = project
                        .worktree_for_id(path.worktree_id, cx)
                        .ok_or_else(|| anyhow!("Worktree missing"))?;
                    let root = worktree.read(cx).abs_path().to_path_buf();
                    let file = project
                        .absolute_path(&path, cx)
                        .ok_or_else(|| anyhow!("File path unavailable"))?;
                    Ok::<_, anyhow::Error>((root, file))
                })
                .map_err(|e| e.to_string().into())?;
            // Check the saved file before asking, and again immediately before running.
            let root_check = root.clone();
            let file_check = file.clone();
            let checked = cx
                .background_spawn(async move { validate_saved_path(&root_check, &file_check) })
                .await;
            checked.map_err(|e| e.to_string().into())?;
            let prompt = cx.update(|cx| {
                event_stream.authorize_always_prompt(
                    format!(
                        "Run project Lean on {}? Lake may execute project code",
                        input.path
                    ),
                    ToolPermissionContext::new(Self::NAME, vec![input.path.clone()]),
                    cx,
                )
            });
            prompt.await.map_err(|e| e.to_string().into())?;
            let task = cx.update(|cx| {
                gpui_tokio::Tokio::spawn(cx, async move {
                    check_saved(root, file, input.theorem, Path::new("lake")).await
                })
            });
            task.await
                .map_err(|e| e.to_string().into())?
                .map_err(|e| e.to_string().into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_non_project_paths_and_injected_theorem() -> Result<()> {
        for path in [
            "/tmp/proof.lean",
            "../proof.lean",
            "src/../proof.lean",
            "proof.txt",
        ] {
            assert!(validate_relative(path).is_err(), "{path}");
        }
        for name in ["x\n#eval IO.println \"bad\"", "foo;bar", "", "a..b"] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        Ok(())
    }
}
