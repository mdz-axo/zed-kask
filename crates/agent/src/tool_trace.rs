//! Opt-in, single-turn native-agent tool trace. No normal turn records raw tool data.
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context as _, Result, bail};
use chrono::{DateTime, Utc};
use language_model::LanguageModelToolUseId;
use serde::Serialize;

use crate::thread::{AgentMessageContent, Message};

// Refuse an oversize export rather than silently truncating evidence or filling disk.
const MAX_TRACE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct ToolTrace {
    pub(crate) run_id: String,
    pub(crate) session_id: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) elapsed_ms: u128,
    pub(crate) completion_error: Option<String>,
    pub(crate) final_answer: Option<String>,
    pub(crate) calls: Vec<ToolTraceCall>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolTraceCall {
    tool_use_id: String,
    name: String,
    raw_input: String,
    input: serde_json::Value,
    input_complete: bool,
    outcome: &'static str,
    result_text: Option<String>,
    raw_output: Option<serde_json::Value>,
    non_text_parts_omitted: usize,
    elapsed_ms: Option<u128>,
}

pub(crate) struct ToolTraceCapture {
    run_id: String,
    session_id: String,
    first_message_ix: usize,
    started_at: DateTime<Utc>,
    started: Instant,
    tool_starts: HashMap<LanguageModelToolUseId, Instant>,
    tool_durations: HashMap<LanguageModelToolUseId, u128>,
}

impl ToolTraceCapture {
    pub(crate) fn new(session_id: String, first_message_ix: usize) -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string(),
            session_id,
            first_message_ix,
            started_at: Utc::now(),
            started: Instant::now(),
            tool_starts: HashMap::new(),
            tool_durations: HashMap::new(),
        }
    }

    pub(crate) fn tool_started(&mut self, id: &LanguageModelToolUseId) {
        self.tool_starts
            .entry(id.clone())
            .or_insert_with(Instant::now);
    }

    pub(crate) fn tool_finished(&mut self, id: &LanguageModelToolUseId) {
        if let Some(start) = self.tool_starts.remove(id) {
            self.tool_durations
                .insert(id.clone(), start.elapsed().as_millis());
        }
    }

    pub(crate) fn finish(self, messages: &[Arc<Message>], error: Option<String>) -> ToolTrace {
        let turn = messages.get(self.first_message_ix..).unwrap_or_default();
        let calls = turn
            .iter()
            .filter_map(|message| match message.as_ref() {
                Message::Agent(message) => Some(message),
                _ => None,
            })
            .flat_map(|message| {
                message
                    .content
                    .iter()
                    .filter_map(|part| {
                        let AgentMessageContent::ToolUse(use_) = part else {
                            return None;
                        };
                        let result = message.tool_results.get(&use_.id);
                        Some(ToolTraceCall {
                            tool_use_id: use_.id.to_string(),
                            name: use_.name.to_string(),
                            raw_input: use_.raw_input.clone(),
                            input: use_.input.to_display_json(),
                            input_complete: use_.is_input_complete,
                            outcome: match result {
                                Some(result) if result.is_error => "failed",
                                Some(_) => "completed",
                                None => "not_completed",
                            },
                            result_text: result.map(|result| result.text_contents()),
                            raw_output: result.and_then(|result| result.output.clone()),
                            non_text_parts_omitted: result.map_or(0, |result| {
                                result
                                    .content
                                    .iter()
                                    .filter(|part| {
                                        matches!(
                                            part,
                                            language_model::LanguageModelToolResultContent::Image(
                                                _
                                            )
                                        )
                                    })
                                    .count()
                            }),
                            elapsed_ms: self.tool_durations.get(&use_.id).copied(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let final_answer = turn.iter().rev().find_map(|message| {
            let Message::Agent(message) = message.as_ref() else {
                return None;
            };
            let text = message
                .content
                .iter()
                .filter_map(|part| match part {
                    AgentMessageContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        });
        ToolTrace {
            run_id: self.run_id,
            session_id: self.session_id,
            started_at: self.started_at,
            elapsed_ms: self.started.elapsed().as_millis(),
            completion_error: error,
            final_answer,
            calls,
        }
    }
}

pub(crate) fn trace_directory() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_TRACE_DIR.with(|dir| dir.borrow().clone()) {
        return path;
    }
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new("agent-traces"))
}

#[cfg(test)]
thread_local! {
    static TEST_TRACE_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestTraceDirectoryGuard(Option<PathBuf>);

#[cfg(test)]
impl TestTraceDirectoryGuard {
    pub(crate) fn scoped(path: PathBuf) -> Self {
        Self(TEST_TRACE_DIR.with(|dir| dir.replace(Some(path))))
    }
}

#[cfg(test)]
impl Drop for TestTraceDirectoryGuard {
    fn drop(&mut self) {
        TEST_TRACE_DIR.with(|dir| {
            dir.replace(self.0.take());
        });
    }
}

/// Store sensitive raw arguments/results only on explicit request. Never overwrite a run.
/// A pre-existing permissive directory is refused rather than silently disclosing contents.
pub(crate) fn write_trace(trace: &ToolTrace, directory: &Path) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .with_context(|| format!("create trace directory {}", directory.display()))?;
    }
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!(
            "trace directory must be a real directory: {}",
            directory.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!(
                "trace directory permits group/other access: {}",
                directory.display()
            );
        }
        let bytes = serde_json::to_vec_pretty(trace)?;
        if bytes.len() > MAX_TRACE_BYTES {
            bail!("trace export exceeds {MAX_TRACE_BYTES} bytes; no trace was written");
        }
        let path = directory.join(format!("{}.json", trace.run_id));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("create trace {}", path.display()))?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            // A partial file must not masquerade as a complete trace.
            drop(file);
            std::fs::remove_file(&path)
                .with_context(|| format!("remove incomplete trace {}", path.display()))?;
            return Err(error.into());
        }
        Ok(path)
    }
    #[cfg(not(unix))]
    bail!("private on-demand traces require Unix file permissions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{AgentMessage, AgentMessageContent};
    use collections::IndexMap;
    use language_model::{
        LanguageModelToolResult, LanguageModelToolResultContent, LanguageModelToolUse,
        LanguageModelToolUseInput,
    };

    #[test]
    fn captured_tool_result_and_missing_result_are_distinct() -> Result<()> {
        let id = LanguageModelToolUseId::from("call-1");
        let use_ = LanguageModelToolUse {
            id: id.clone(),
            name: Arc::from("onto_anchor"),
            raw_input: "{\"term\":\"net margin\"}".into(),
            input: LanguageModelToolUseInput::Json(serde_json::json!({"term":"net margin"})),
            is_input_complete: true,
            thought_signature: None,
        };
        let mut capture = ToolTraceCapture::new("session".into(), 1);
        capture.tool_started(&id);
        capture.tool_finished(&id);
        let result = LanguageModelToolResult {
            tool_use_id: id.clone(),
            tool_name: Arc::from("onto_anchor"),
            is_error: false,
            content: vec![LanguageModelToolResultContent::Text(Arc::from(
                "{\"concept\":\"net_margin\"}",
            ))],
            output: None,
        };
        let message = AgentMessage {
            content: vec![
                AgentMessageContent::ToolUse(use_),
                AgentMessageContent::Text("answer".into()),
            ],
            tool_results: IndexMap::from_iter([(id, result)]),
            reasoning_details: None,
        };
        let messages = vec![Arc::new(Message::Resume), Arc::new(Message::Agent(message))];
        let trace = capture.finish(&messages, None);
        let value = serde_json::to_value(&trace)?;
        assert_eq!(value["calls"][0]["input"]["term"], "net margin");
        assert_eq!(
            value["calls"][0]["result_text"],
            "{\"concept\":\"net_margin\"}"
        );
        assert_eq!(value["calls"][0]["outcome"], "completed");
        assert!(value["calls"][0]["elapsed_ms"].is_number());
        assert_eq!(value["final_answer"], "answer");
        Ok(())
    }

    #[test]
    fn private_export_round_trips_and_refuses_a_permissive_directory() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir()?;
        let directory = root.path().join("agent-traces");
        let trace =
            ToolTraceCapture::new("test-session".into(), 0).finish(&[], Some("interrupted".into()));
        let path = write_trace(&trace, &directory)?;
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(saved["session_id"], "test-session");
        assert_eq!(saved["completion_error"], "interrupted");
        assert_eq!(saved["final_answer"], serde_json::Value::Null);
        assert_eq!(saved["calls"], serde_json::json!([]));
        assert_eq!(
            std::fs::metadata(&directory)?.permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        assert!(
            write_trace(&trace, &directory).is_err(),
            "a run must never overwrite its trace"
        );
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))?;
        let other = ToolTraceCapture::new("other-session".into(), 0).finish(&[], None);
        assert!(
            write_trace(&other, &directory).is_err(),
            "permissive directory must fail closed"
        );
        Ok(())
    }

    #[test]
    fn oversize_export_fails_without_leaving_a_partial_file() -> Result<()> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("agent-traces");
        let mut trace = ToolTraceCapture::new("large-session".into(), 0).finish(&[], None);
        trace.final_answer = Some("x".repeat(MAX_TRACE_BYTES));
        let error = write_trace(&trace, &directory).expect_err("oversize trace must fail");
        assert!(error.to_string().contains("exceeds"), "{error}");
        assert_eq!(std::fs::read_dir(&directory)?.count(), 0);
        Ok(())
    }
}
