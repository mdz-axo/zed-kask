use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{AgentTool, ToolCallEventStream, ToolInput, ToolPermissionContext};
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result, anyhow, bail};
use gpui::{App, AppContext as _, Entity, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use project::{Project, WorktreeSettings};
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
#[serde(untagged)]
pub enum LeanCheckToolOutput {
    Check(LeanCheckResult),
    Error { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeanCheckResult {
    pub lean_version: String,
    pub diagnostics: Vec<serde_json::Value>,
    pub messages: Vec<String>,
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

// Resolve the per-user Elan launcher directly: a GUI-launched editor need not
// inherit the shell's PATH. A manually provided Lake on PATH remains supported.
fn lake_executable(elan_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    let explicit_elan_home = elan_home.is_some();
    let elan_home = match elan_home {
        Some(path) if !path.is_absolute() => bail!("ELAN_HOME must be an absolute path"),
        Some(path) => Some(path.to_path_buf()),
        None => home.map(|path| path.join(".elan")),
    };
    if let Some(elan_home) = elan_home {
        let lake = elan_home.join("bin/lake");
        if lake.is_file() {
            return Ok(lake);
        }
        if explicit_elan_home {
            bail!(
                "ELAN_HOME is set but Lake is missing at {}; run the supported zed-kask installer",
                lake.display()
            );
        }
    }
    Ok(PathBuf::from("lake"))
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
    let relative = path
        .strip_prefix(root)
        .context("file is not inside the worktree root")?;
    let root = std::fs::canonicalize(root).context("cannot resolve local worktree root")?;
    let file = std::fs::canonicalize(path).context("saved Lean file is missing or inaccessible")?;
    // Reject even internal symlink aliases: otherwise a non-private alias can
    // expose a private/excluded target to the process after the settings check.
    if file != root.join(relative) || !file.is_file() {
        bail!("Lean file must be a saved regular file inside the local worktree without symlinks")
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

#[cfg(unix)]
struct KillProcessGroupOnDrop(Option<i32>);

#[cfg(unix)]
impl Drop for KillProcessGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            // A timed-out Lake wrapper may have spawned a Lean child. Kill the
            // whole group, not just the wrapper, including on task cancellation.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.as_std_mut().process_group(0);
    }
    if input.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command.spawn().with_context(|| format!("Cannot start Lake; install elan/Lake and the project's lean-toolchain (executable: {})", lake.display()))?;
    #[cfg(unix)]
    let mut group = KillProcessGroupOnDrop(child.id().and_then(|pid| i32::try_from(pid).ok()));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Lake stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Lake stderr unavailable"))?;
    let mut stdin = if input.is_some() {
        Some(
            child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("Lake stdin unavailable"))?,
        )
    } else {
        None
    };
    let execution = async {
        let write = async {
            if let (Some(bytes), Some(mut stdin)) = (input, stdin.take()) {
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
    let result = tokio::time::timeout(RUN_LIMIT, execution)
        .await
        .context("Lake/Lean timed out after 30 seconds")?;
    if result.is_ok() {
        #[cfg(unix)]
        {
            group.0 = None;
        }
    }
    result
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
) -> Result<LeanCheckResult> {
    let file = validate_saved_path(&root, &file)?;
    let root = std::fs::canonicalize(root)?;
    let project = project_root(&root, &file)?;
    let pin = std::fs::read_to_string(project.join("lean-toolchain"))?;
    let pin = pin.trim();
    let expected = pin.strip_prefix("leanprover/lean4:v")
        .filter(|version| !version.is_empty() && version.len() < 64 && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .ok_or_else(|| anyhow!("Unsupported Lean toolchain pin {pin:?}; use a pinned leanprover/lean4:v<version> toolchain"))?;
    let (version_code, version, version_err) =
        invoke_lake(lake, &project, &["--version"], None).await?;
    if version_code != Some(0) {
        bail!("Lake could not select the pinned Lean toolchain: {version_err}")
    }
    let version = version.trim().to_string();
    if !version.contains(&format!("version {expected},")) {
        bail!(
            "Lake selected {version:?}, but lean-toolchain pins v{expected}; install the pinned toolchain via elan"
        )
    }
    let relative = file.strip_prefix(&project)?;
    let relative = relative
        .to_str()
        .ok_or_else(|| anyhow!("Lean file path is not UTF-8"))?;
    let mut audit_line = None;
    let (code, stdout, stderr) = if let Some(name) = theorem.as_deref() {
        validate_name(name)?;
        let mut source = tokio::fs::read(&file).await?;
        audit_line = Some(source.iter().filter(|&&b| b == b'\n').count() + 2);
        source.extend_from_slice(format!("\n#print axioms {name}\n").as_bytes());
        invoke_lake(lake, &project, &["--json", "--stdin"], Some(&source)).await?
    } else {
        invoke_lake(lake, &project, &["--json", relative], None).await?
    };
    let (diagnostics, text) = parse_output(&stdout, &stderr);
    let audit = theorem.as_deref().and_then(|name| {
        diagnostics
            .iter()
            .filter(|v| v["pos"]["line"].as_u64() == audit_line.map(|n| n as u64))
            .filter_map(|v| v.get("data")?.as_str())
            .find(|s| {
                s.contains(&format!("'{name}' does not depend on any axioms"))
                    || s.contains(&format!("'{name}' depends on axioms:"))
            })
    });
    let audit = audit.map(str::to_string);
    let has_warning = diagnostics
        .iter()
        .any(|v| v["severity"] == "warning" || v["severity"] == "error")
        || text
            .iter()
            .any(|s| s.contains("warning:") || s.contains("error:"));
    let completion_status = if code != Some(0) {
        "failed"
    } else if audit
        .as_ref()
        .is_some_and(|s| s.contains("depends on axioms:"))
    {
        "axioms_present"
    } else if has_warning {
        "warnings"
    } else if audit.is_some() {
        "axiom_free"
    } else if theorem.is_some() {
        "audit_unconfirmed"
    } else {
        "checked_not_axiom_audited"
    };
    let goal_text = diagnostics
        .iter()
        .filter_map(|v| v.get("data")?.as_str())
        .filter(|s| s.contains('⊢') || s.contains("unsolved goals"))
        .map(str::to_string)
        .collect();
    Ok(LeanCheckResult {
        lean_version: version,
        diagnostics,
        messages: text,
        goal_text,
        completion_status: completion_status.into(),
        axioms: audit,
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
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let result: anyhow::Result<LeanCheckResult> = async {
                let input = input.recv().await.map_err(|e| anyhow!("{e}"))?;
                validate_relative(&input.path)?;
                if let Some(name) = &input.theorem {
                    validate_name(name)?;
                }
                let fs = project.read_with(cx, |project, _| project.fs().clone());
                let roots = canonicalize_worktree_roots(&project, &fs, cx).await;
                let (root, file) = project.read_with(cx, |project, cx| {
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
                })?;
                // Check the saved file before asking, and again immediately before running.
                let root_check = root.clone();
                let file_check = file.clone();
                let checked = cx
                    .background_spawn(async move { validate_saved_path(&root_check, &file_check) })
                    .await;
                checked?;
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
                prompt.await?;
                let elan_home = std::env::var_os("ELAN_HOME").map(PathBuf::from);
                let home = std::env::var_os("HOME").map(PathBuf::from);
                let lake = lake_executable(elan_home.as_deref(), home.as_deref())?;
                let task = cx.update(|cx| {
                    gpui_tokio::Tokio::spawn(cx, async move {
                        check_saved(root, file, input.theorem, &lake).await
                    })
                });
                task.await.map_err(|e| anyhow!("{e}"))?
            }
            .await;
            result
                .map(LeanCheckToolOutput::Check)
                .map_err(|e| LeanCheckToolOutput::Error {
                    error: format!("{e:#}"),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;
    use gpui::TestAppContext;
    use serde_json::json;
    use settings::SettingsStore;

    #[gpui::test]
    async fn public_tool_rejects_invalid_path_without_prompt_and_requires_approval(
        cx: &mut TestAppContext,
    ) {
        if let Err(error) = run_public_tool_permission_test(cx).await {
            panic!("{error:#}");
        }
    }

    async fn run_public_tool_permission_test(cx: &mut TestAppContext) -> Result<()> {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
        });
        let dir = tempfile::tempdir()?;
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            dir.path(),
            json!({ "Proof.lean": "theorem t : True := by trivial" }),
        )
        .await;
        let project = Project::test(fs, [dir.path()], cx).await;
        let tool = Arc::new(LeanCheckTool::new(project));
        let (stream, mut events) = ToolCallEventStream::test();
        let invalid = cx
            .update(|cx| {
                tool.clone().run(
                    ToolInput::resolved(LeanCheckToolInput {
                        path: "../Proof.lean".into(),
                        theorem: None,
                    }),
                    stream,
                    cx,
                )
            })
            .await;
        assert!(matches!(invalid, Err(LeanCheckToolOutput::Error { .. })));
        assert!(
            events.try_recv().is_err(),
            "invalid paths must not ask for execution permission"
        );

        // A real saved file reaches the mandatory prompt, not the Lean process.
        std::fs::write(
            dir.path().join("Proof.lean"),
            "theorem t : True := by trivial\n",
        )?;
        let root_name = dir
            .path()
            .file_name()
            .context("temp root has no name")?
            .to_string_lossy();
        let (stream, mut events) = ToolCallEventStream::test();
        let task = cx.update(|cx| {
            tool.clone().run(
                ToolInput::resolved(LeanCheckToolInput {
                    path: format!("{root_name}/Proof.lean"),
                    theorem: None,
                }),
                stream,
                cx,
            )
        });
        let request = events.expect_authorization().await;
        assert!(
            request
                .tool_call
                .fields
                .title
                .as_deref()
                .is_some_and(|s| s.contains("may execute project code"))
        );
        request
            .response
            .send(acp_thread::SelectedPermissionOutcome::new(
                acp::PermissionOptionId::new("deny"),
                acp::PermissionOptionKind::RejectOnce,
            ))
            .map_err(|_| anyhow!("permission response channel closed"))?;
        assert!(task.await.is_err());
        Ok(())
    }

    #[test]
    fn uses_installed_elan_launcher_without_shell_path() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let elan = dir.path().join(".elan");
        std::fs::create_dir_all(elan.join("bin"))?;
        std::fs::write(elan.join("bin/lake"), "test launcher")?;
        assert_eq!(
            lake_executable(None, Some(dir.path()))?,
            elan.join("bin/lake")
        );
        assert_eq!(lake_executable(Some(&elan), None)?, elan.join("bin/lake"));
        assert!(lake_executable(Some(Path::new("relative/elan")), None).is_err());
        assert!(lake_executable(Some(&dir.path().join("missing-elan")), None).is_err());
        Ok(())
    }

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

    #[tokio::test]
    async fn saved_project_check_reports_proof_failure_and_axiom_trust() -> Result<()> {
        let lake = std::env::var_os("LEAN_CHECK_TEST_LAKE")
            .context("set LEAN_CHECK_TEST_LAKE to a Lean 4 Lake binary to run this test")?;
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:v4.34.0\n")?;
        std::fs::write(
            root.join("lakefile.lean"),
            "import Lake\nopen Lake DSL\npackage lean_check_test\n",
        )?;
        let file = root.join("Proof.lean");
        let lake = Path::new(&lake);
        std::fs::write(&file, "theorem clean : True := by trivial\n")?;
        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:v0.0.0\n")?;
        let wrong_pin = check_saved(root.into(), file.clone(), None, lake).await;
        let wrong_pin_error = format!("{:#}", wrong_pin.unwrap_err());
        assert!(
            wrong_pin_error.contains("v0.0.0")
                && (wrong_pin_error.contains("Lake could not select the pinned Lean toolchain")
                    || wrong_pin_error.contains("lean-toolchain pins v0.0.0")),
            "{wrong_pin_error}"
        );
        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:v4.34.0\n")?;
        let clean = check_saved(root.into(), file.clone(), Some("clean".into()), lake).await?;
        assert_eq!(
            clean.completion_status, "axiom_free",
            "{:?}",
            clean.diagnostics
        );
        assert!(clean.lean_version.contains("4.34.0"));
        assert!(
            clean
                .axioms
                .as_deref()
                .is_some_and(|a| a.contains("does not depend on any axioms"))
        );

        std::fs::write(&file, "theorem false_claim : 1 = 2 := by decide\n")?;
        let bad = check_saved(root.into(), file.clone(), None, lake).await?;
        assert_eq!(bad.completion_status, "failed");
        assert!(bad.diagnostics.iter().any(|v| v["severity"] == "error"));

        std::fs::write(&file, "theorem hole : False := by sorry\n")?;
        let hole = check_saved(root.into(), file.clone(), Some("hole".into()), lake).await?;
        assert_eq!(hole.completion_status, "axioms_present");
        assert!(
            hole.axioms
                .as_deref()
                .is_some_and(|a| a.contains("sorryAx"))
        );
        let unaudited = check_saved(root.into(), file.clone(), None, lake).await?;
        assert_ne!(unaudited.completion_status, "axiom_free");
        std::fs::write(&file, "theorem compiled : 2 + 2 = 4 := by native_decide\n")?;
        let native = check_saved(root.into(), file, Some("compiled".into()), lake).await?;
        assert_eq!(native.completion_status, "axioms_present");
        assert!(
            native
                .axioms
                .as_deref()
                .is_some_and(|a| a.contains("native_decide"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_toolchain_and_symlink_escape_fail_closed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let file = dir.path().join("Proof.lean");
        std::fs::write(&file, "theorem t : True := by trivial\n")?;
        std::fs::write(
            dir.path().join("lakefile.lean"),
            "import Lake\nopen Lake DSL\npackage missing\n",
        )?;
        let missing = check_saved(dir.path().into(), file.clone(), None, Path::new("lake")).await;
        assert!(format!("{:#}", missing.unwrap_err()).contains("lean-toolchain"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&file, outside.path().join("link.lean"))?;
            assert!(
                validate_saved_path(outside.path(), &outside.path().join("link.lean")).is_err()
            );
        }
        std::fs::write(
            dir.path().join("lean-toolchain"),
            "leanprover/lean4:v4.34.0\n",
        )?;
        let missing_lake =
            check_saved(dir.path().into(), file, None, Path::new("/no/such/lake")).await;
        assert!(format!("{:#}", missing_lake.unwrap_err()).contains("install elan/Lake"));
        Ok(())
    }
}
