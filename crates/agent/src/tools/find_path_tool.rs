use crate::{AgentTool, ToolCallEventStream, ToolInput};
use acp_thread::MentionUri;
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Result, anyhow};
use futures::FutureExt as _;
use gpui::{App, AppContext, Entity, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::{cmp, path::PathBuf, sync::Arc};
use util::paths::PathMatcher;

/// Find file paths that match a given pattern.
///
/// - Supports glob patterns like "**/*.js" or "src/**/*.ts"
/// - Returns matching file paths sorted alphabetically
/// - Prefer the `grep` tool to this tool when searching for symbols unless you have specific information about paths.
/// - Use this tool when you need to find files by name patterns
/// - Results are paginated with 50 matches per page. Use the optional 'offset' parameter to request subsequent pages.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FindPathToolInput {
    /// The glob to match against every path in the project.
    ///
    /// <example>
    /// If the project has the following root directories:
    ///
    /// - directory1/a/something.txt
    /// - directory2/a/things.txt
    /// - directory3/a/other.txt
    ///
    /// You can get back the first two paths by providing a glob of "*thing*.txt"
    /// </example>
    pub glob: String,
    /// Optional starting position for paginated results (0-based).
    /// When not provided, starts from the beginning.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FindPathToolOutput {
    Success {
        offset: usize,
        current_matches_page: Vec<PathBuf>,
        all_matches_len: usize,
        /// Present when a non-empty glob matched zero files and its first
        /// literal segment names no project root (the wrong-root trap) —
        /// distinguishes "pattern cannot match" from a genuinely empty
        /// result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zero_match_note: Option<String>,
    },
    Error {
        error: String,
    },
}

impl From<FindPathToolOutput> for LanguageModelToolResultContent {
    fn from(output: FindPathToolOutput) -> Self {
        match output {
            FindPathToolOutput::Success {
                offset,
                current_matches_page,
                all_matches_len,
                zero_match_note,
            } => {
                if current_matches_page.is_empty() {
                    match zero_match_note {
                        Some(note) => format!("No matches found. {note}").into(),
                        None => "No matches found".into(),
                    }
                } else {
                    let mut llm_output = format!("Found {} total matches.", all_matches_len);
                    if all_matches_len > RESULTS_PER_PAGE {
                        write!(
                            &mut llm_output,
                            "\nShowing results {}-{} (provide 'offset' parameter for more results):",
                            offset + 1,
                            offset + current_matches_page.len()
                        )
                        .ok();
                    }

                    for mat in current_matches_page {
                        write!(&mut llm_output, "\n{}", mat.display()).ok();
                    }

                    llm_output.into()
                }
            }
            FindPathToolOutput::Error { error } => error.into(),
        }
    }
}

const RESULTS_PER_PAGE: usize = 50;

pub struct FindPathTool {
    project: Entity<Project>,
}

impl FindPathTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for FindPathTool {
    type Input = FindPathToolInput;
    type Output = FindPathToolOutput;

    const NAME: &'static str = "find_path";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Search
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        let mut title = "Find paths".to_string();
        if let Ok(input) = input {
            title.push_str(&format!(" matching “`{}`”", input.glob));
        }
        title.into()
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|e| FindPathToolOutput::Error {
                error: e.to_string(),
            })?;

            let search_paths_task = cx.update(|cx| search_paths(&input.glob, project.clone(), cx));

            let matches = futures::select! {
                result = search_paths_task.fuse() => result.map_err(|e| FindPathToolOutput::Error { error: e.to_string() })?,
                _ = event_stream.cancelled_by_user().fuse() => {
                    return Err(FindPathToolOutput::Error { error: "Path search cancelled by user".to_string() });
                }
            };
            let paginated_matches: &[PathBuf] = &matches[cmp::min(input.offset, matches.len())
                ..cmp::min(input.offset + RESULTS_PER_PAGE, matches.len())];

            // A glob whose first literal segment names no project root can
            // only match zero files (globs are matched against full paths
            // starting with a project root) — surface that so a zero
            // result is distinguishable from a genuine empty project.
            let zero_match_note = if matches.is_empty() && !input.glob.trim().is_empty() {
                cx.update(|cx| {
                    let root_names: Vec<String> = project
                        .read(cx)
                        .worktrees(cx)
                        .map(|worktree| worktree.read(cx).root_name_str().to_string())
                        .collect();
                    super::orphaned_path_pattern_note(&input.glob, &root_names)
                })
            } else {
                None
            };

            event_stream.update_fields(
                acp::ToolCallUpdateFields::new()
                    .title(if paginated_matches.is_empty() {
                        "No matches".into()
                    } else if paginated_matches.len() == 1 {
                        "1 match".into()
                    } else {
                        format!("{} matches", paginated_matches.len())
                    })
                    .content(
                        paginated_matches
                            .iter()
                            .map(|path| {
                                let uri = MentionUri::File {
                                    abs_path: path.clone(),
                                };
                                acp::ToolCallContent::Content(acp::Content::new(
                                    acp::ContentBlock::ResourceLink(acp::ResourceLink::new(
                                        path.to_string_lossy(),
                                        uri.to_uri().to_string(),
                                    )),
                                ))
                            })
                            .collect::<Vec<_>>(),
                    ),
            );

            Ok(FindPathToolOutput::Success {
                offset: input.offset,
                current_matches_page: paginated_matches.to_vec(),
                all_matches_len: matches.len(),
                zero_match_note,
            })
        })
    }
}

fn search_paths(glob: &str, project: Entity<Project>, cx: &mut App) -> Task<Result<Vec<PathBuf>>> {
    let path_style = project.read(cx).path_style(cx);
    let path_matcher = match PathMatcher::new(
        [
            // Sometimes models try to search for "". In this case, return all paths in the project.
            if glob.is_empty() { "*" } else { glob },
        ],
        path_style,
    ) {
        Ok(matcher) => matcher,
        Err(err) => return Task::ready(Err(anyhow!("Invalid glob: {err}"))),
    };
    let snapshots: Vec<_> = project
        .read(cx)
        .worktrees(cx)
        .map(|worktree| worktree.read(cx).snapshot())
        .collect();

    cx.background_spawn(async move {
        let mut results = Vec::new();
        for snapshot in snapshots {
            for entry in snapshot.entries(false, 0) {
                if path_matcher.is_match(&snapshot.root_name().join(&entry.path)) {
                    results.push(snapshot.absolutize(&entry.path));
                }
            }
        }

        Ok(results)
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use gpui::TestAppContext;
    use project::{FakeFs, Project};
    use settings::SettingsStore;
    use util::path;

    #[gpui::test]
    async fn test_find_path_tool(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/root",
            serde_json::json!({
                "apple": {
                    "banana": {
                        "carrot": "1",
                    },
                    "bandana": {
                        "carbonara": "2",
                    },
                    "endive": "3"
                }
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

        let matches = cx
            .update(|cx| search_paths("root/**/car*", project.clone(), cx))
            .await
            .unwrap();
        assert_eq!(
            matches,
            &[
                PathBuf::from(path!("/root/apple/banana/carrot")),
                PathBuf::from(path!("/root/apple/bandana/carbonara"))
            ]
        );

        let matches = cx
            .update(|cx| search_paths("**/car*", project.clone(), cx))
            .await
            .unwrap();
        assert_eq!(
            matches,
            &[
                PathBuf::from(path!("/root/apple/banana/carrot")),
                PathBuf::from(path!("/root/apple/bandana/carbonara"))
            ]
        );
    }

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    async fn run_find_path_tool_text(
        input: FindPathToolInput,
        project: Entity<Project>,
        cx: &mut TestAppContext,
    ) -> String {
        let tool = Arc::new(FindPathTool::new(project));
        let task = cx.update(|cx| {
            tool.run(
                ToolInput::resolved(input),
                ToolCallEventStream::test().0,
                cx,
            )
        });
        let output = match task.await {
            Ok(output) => output,
            Err(FindPathToolOutput::Error { error }) => {
                panic!("find_path failed: {error}")
            }
            Err(FindPathToolOutput::Success { .. }) => {
                panic!("find_path returned Err(Success)")
            }
        };
        match LanguageModelToolResultContent::from(output) {
            LanguageModelToolResultContent::Text(text) => text.to_string(),
            _ => panic!("expected text output"),
        }
    }

    #[gpui::test]
    async fn test_find_path_tool_orphaned_glob_warns(cx: &mut TestAppContext) {
        init_test(cx);

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root"),
            serde_json::json!({
                "apple": {
                    "banana": {
                        "carrot": "1",
                    },
                },
            }),
        )
        .await;
        let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

        // Wrong-root glob: matches zero files (globs are matched against
        // full paths starting with the project root), and the output says
        // so with the corrected form instead of a bare "No matches found".
        let output = run_find_path_tool_text(
            FindPathToolInput {
                glob: "apple/**".to_string(),
                offset: 0,
            },
            project.clone(),
            cx,
        )
        .await;
        assert!(output.contains("No matches found"), "got: {output}");
        assert!(output.contains("cannot match any file"), "got: {output}");
        assert!(output.contains("`**/apple/**`"), "got: {output}");

        // Correctly-rooted glob that matches: no note.
        let output = run_find_path_tool_text(
            FindPathToolInput {
                glob: "root/**/car*".to_string(),
                offset: 0,
            },
            project.clone(),
            cx,
        )
        .await;
        assert!(output.contains("Found 1 total matches."), "got: {output}");
        assert!(!output.contains("cannot match any file"), "got: {output}");

        // Correctly-rooted glob, genuinely absent target: plain
        // no-matches — the warning must never fire on correct output.
        let output = run_find_path_tool_text(
            FindPathToolInput {
                glob: "root/**/zebra".to_string(),
                offset: 0,
            },
            project.clone(),
            cx,
        )
        .await;
        assert_eq!(output, "No matches found");
    }
}
