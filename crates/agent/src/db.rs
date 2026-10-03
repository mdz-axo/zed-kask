use crate::{AgentMessage, AgentMessageContent, UserMessage, UserMessageContent};
use acp_thread::ClientUserMessageId;
use agent_client_protocol::schema::v1 as acp;
use agent_settings::AgentProfileId;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use collections::{HashMap, IndexMap};
use futures::{FutureExt, future::Shared};
use gpui::{BackgroundExecutor, Global, Task};
use indoc::indoc;
use language_model::Speed;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sqlez::{
    bindable::{Bind, Column},
    connection::Connection,
    statement::Statement,
};
use std::{io::ErrorKind, path::PathBuf, sync::Arc};
use ui::{App, SharedString};
use util::path_list::PathList;
use zed_env_vars::ZED_STATELESS;

pub type DbMessage = crate::Message;
pub type DbSummary = crate::legacy_thread::DetailedSummaryState;
pub type DbLanguageModel = crate::legacy_thread::SerializedLanguageModel;

#[derive(Debug, Clone)]
pub struct DbThreadMetadata {
    pub id: acp::SessionId,
    pub parent_session_id: Option<acp::SessionId>,
    pub title: SharedString,
    pub updated_at: DateTime<Utc>,
    pub created_at: Option<DateTime<Utc>>,
    /// The workspace folder paths this thread was created against, sorted
    /// lexicographically. Used for grouping threads by project in the sidebar.
    pub folder_paths: PathList,
}

impl From<&DbThreadMetadata> for acp_thread::AgentSessionInfo {
    fn from(meta: &DbThreadMetadata) -> Self {
        Self {
            session_id: meta.id.clone(),
            work_dirs: Some(meta.folder_paths.clone()),
            title: Some(meta.title.clone()),
            updated_at: Some(meta.updated_at),
            created_at: meta.created_at,
            meta: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbThread {
    pub title: SharedString,
    pub messages: Vec<Arc<DbMessage>>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub detailed_summary: Option<SharedString>,
    #[serde(default)]
    pub initial_project_snapshot: Option<Arc<crate::ProjectSnapshot>>,
    #[serde(default)]
    pub cumulative_token_usage: language_model::TokenUsage,
    #[serde(default)]
    pub request_token_usage: HashMap<acp_thread::ClientUserMessageId, language_model::TokenUsage>,
    #[serde(default)]
    pub model: Option<DbLanguageModel>,
    #[serde(default)]
    pub profile: Option<AgentProfileId>,
    #[serde(default)]
    pub delegation_authority: Option<crate::DelegationAuthority>,
    #[serde(default)]
    pub subagent_context: Option<crate::SubagentContext>,
    #[serde(default)]
    pub speed: Option<Speed>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub draft_prompt: Option<Vec<acp::ContentBlock>>,
    #[serde(default)]
    pub ui_scroll_position: Option<SerializedScrollPosition>,
    #[serde(default)]
    pub sandboxed_terminal_temp_dir: Option<PathBuf>,
    /// Sandbox escalations the user approved "for the rest of this thread".
    /// Persisted so reopening a thread keeps its grants. See
    /// [`crate::sandboxing::ThreadSandboxGrants`].
    #[serde(default)]
    pub sandbox_grants: DbSandboxGrants,
}

/// Serialized form of the sandbox permissions the user granted "for the rest of
/// this thread" (the "Allow for this thread" prompt option). Stored inside the
/// thread blob; round-trips with [`crate::sandboxing::ThreadSandboxGrants`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DbSandboxGrants {
    /// Paths granted write access, each paired with the canonical
    /// (symlink-resolved) target established when the grant was approved; each
    /// covers its whole subtree. Legacy rows stored a bare path string per
    /// entry, which still deserializes (as a grant with no resolved canonical)
    /// via [`settings::GrantedWritePath`]'s string-or-object format.
    #[serde(default)]
    pub write_paths: Vec<settings::GrantedWritePath>,
    /// Host patterns granted network access, in canonical string form (e.g.
    /// `github.com`, `*.npmjs.org`). Parsed back into patterns on load.
    #[serde(default)]
    pub network_hosts: Vec<String>,
    /// Whether arbitrary-host network access was granted.
    #[serde(default)]
    pub network_any_host: bool,
    /// Whether unrestricted filesystem writes (the broad escape hatch) were
    /// granted.
    #[serde(default)]
    pub allow_fs_write_all: bool,

    /// Whether the model-requested fully-unsandboxed escape was granted.
    #[serde(default)]
    pub unsandboxed: bool,
    /// Whether running commands unsandboxed was allowed because the OS sandbox
    /// could not be created (the fallback prompt's "for this thread" option).
    #[serde(default)]
    pub sandbox_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SerializedScrollPosition {
    pub item_ix: usize,
    pub offset_in_item: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedThread {
    pub title: SharedString,
    pub messages: Vec<Arc<DbMessage>>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub model: Option<DbLanguageModel>,
    #[serde(default)]
    pub delegation_authority: Option<crate::DelegationAuthority>,
    pub version: String,
}

impl SharedThread {
    pub const VERSION: &'static str = "1.0.0";

    pub fn from_db_thread(thread: &DbThread) -> Self {
        Self {
            title: thread.title.clone(),
            messages: thread.messages.clone(),
            updated_at: thread.updated_at,
            model: thread.model.clone(),
            delegation_authority: thread.delegation_authority.clone(),
            version: Self::VERSION.to_string(),
        }
    }

    pub fn to_db_thread(self) -> DbThread {
        DbThread {
            title: format!("🔗 {}", self.title).into(),
            messages: self.messages,
            updated_at: self.updated_at,
            detailed_summary: None,
            initial_project_snapshot: None,
            cumulative_token_usage: Default::default(),
            request_token_usage: Default::default(),
            model: self.model,
            profile: None,
            subagent_context: None,
            speed: None,
            reasoning_effort: None,
            draft_prompt: None,
            ui_scroll_position: None,
            sandboxed_terminal_temp_dir: None,
            sandbox_grants: DbSandboxGrants::default(),
            delegation_authority: self.delegation_authority,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        const COMPRESSION_LEVEL: i32 = 3;
        let json = serde_json::to_vec(self)?;
        let compressed = zstd::encode_all(json.as_slice(), COMPRESSION_LEVEL)?;
        Ok(compressed)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let decompressed = zstd::decode_all(data)?;
        Ok(serde_json::from_slice(&decompressed)?)
    }
}

impl DbThread {
    pub const VERSION: &'static str = "0.3.0";

    pub fn to_markdown(&self) -> String {
        crate::messages_to_markdown(&self.messages)
    }

    pub fn from_json(json: &[u8]) -> Result<Self> {
        let saved_thread_json = serde_json::from_slice::<serde_json::Value>(json)?;
        match saved_thread_json.get("version") {
            Some(serde_json::Value::String(version)) => match version.as_str() {
                Self::VERSION => Ok(serde_json::from_value(saved_thread_json)?),
                _ => Self::upgrade_from_agent_1(crate::legacy_thread::SerializedThread::from_json(
                    json,
                )?),
            },
            _ => {
                Self::upgrade_from_agent_1(crate::legacy_thread::SerializedThread::from_json(json)?)
            }
        }
    }

    fn upgrade_from_agent_1(thread: crate::legacy_thread::SerializedThread) -> Result<Self> {
        let mut messages = Vec::new();
        let mut request_token_usage = HashMap::default();

        let mut last_user_message_id = None;
        for (ix, msg) in thread.messages.into_iter().enumerate() {
            let message = match msg.role {
                language_model::Role::User => {
                    let mut content = Vec::new();

                    // Convert segments to content
                    for segment in msg.segments {
                        match segment {
                            crate::legacy_thread::SerializedMessageSegment::Text { text } => {
                                content.push(UserMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::Thinking {
                                text,
                                ..
                            } => {
                                // User messages don't have thinking segments, but handle gracefully
                                content.push(UserMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::RedactedThinking {
                                ..
                            } => {
                                // User messages don't have redacted thinking, skip.
                            }
                        }
                    }

                    // If no content was added, add context as text if available
                    if content.is_empty() && !msg.context.is_empty() {
                        content.push(UserMessageContent::Text(msg.context));
                    }

                    let id = ClientUserMessageId::new();
                    last_user_message_id = Some(id.clone());

                    crate::Message::User(UserMessage {
                        // MessageId from old format can't be meaningfully converted, so generate a new one
                        id,
                        content: Arc::from(content),
                    })
                }
                language_model::Role::Assistant => {
                    let mut content = Vec::new();

                    // Convert segments to content
                    for segment in msg.segments {
                        match segment {
                            crate::legacy_thread::SerializedMessageSegment::Text { text } => {
                                content.push(AgentMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::Thinking {
                                text,
                                signature,
                            } => {
                                content.push(AgentMessageContent::Thinking { text, signature });
                            }
                            crate::legacy_thread::SerializedMessageSegment::RedactedThinking {
                                data,
                            } => {
                                content.push(AgentMessageContent::RedactedThinking(data));
                            }
                        }
                    }

                    // Convert tool uses
                    let mut tool_names_by_id = HashMap::default();
                    for tool_use in msg.tool_uses {
                        tool_names_by_id.insert(tool_use.id.clone(), tool_use.name.clone());
                        content.push(AgentMessageContent::ToolUse(
                            language_model::LanguageModelToolUse {
                                id: tool_use.id,
                                name: tool_use.name.into(),
                                raw_input: serde_json::to_string(&tool_use.input)
                                    .unwrap_or_default(),
                                input: language_model::LanguageModelToolUseInput::Json(
                                    tool_use.input,
                                ),
                                is_input_complete: true,
                                thought_signature: None,
                            },
                        ));
                    }

                    // Convert tool results
                    let mut tool_results = IndexMap::default();
                    for tool_result in msg.tool_results {
                        let name = tool_names_by_id
                            .remove(&tool_result.tool_use_id)
                            .unwrap_or_else(|| SharedString::from("unknown"));
                        tool_results.insert(
                            tool_result.tool_use_id.clone(),
                            language_model::LanguageModelToolResult {
                                tool_use_id: tool_result.tool_use_id,
                                tool_name: name.into(),
                                is_error: tool_result.is_error,
                                content: vec![tool_result.content],
                                output: tool_result.output,
                            },
                        );
                    }

                    if let Some(last_user_message_id) = &last_user_message_id
                        && let Some(token_usage) = thread.request_token_usage.get(ix).copied()
                    {
                        request_token_usage.insert(last_user_message_id.clone(), token_usage);
                    }

                    crate::Message::Agent(AgentMessage {
                        content,
                        tool_results,
                        reasoning_details: None,
                    })
                }
                language_model::Role::System => {
                    // Skip system messages as they're not supported in the new format
                    continue;
                }
            };

            messages.push(Arc::new(message));
        }

        Ok(Self {
            title: thread.summary,
            messages,
            updated_at: thread.updated_at,
            detailed_summary: match thread.detailed_summary_state {
                crate::legacy_thread::DetailedSummaryState::NotGenerated
                | crate::legacy_thread::DetailedSummaryState::Generating => None,
                crate::legacy_thread::DetailedSummaryState::Generated { text, .. } => Some(text),
            },
            initial_project_snapshot: thread.initial_project_snapshot,
            cumulative_token_usage: thread.cumulative_token_usage,
            request_token_usage,
            model: thread.model,
            profile: thread.profile,
            subagent_context: None,
            speed: None,
            reasoning_effort: None,
            draft_prompt: None,
            ui_scroll_position: None,
            sandboxed_terminal_temp_dir: None,
            sandbox_grants: DbSandboxGrants::default(),
            delegation_authority: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    #[serde(rename = "zstd")]
    Zstd,
}

impl Bind for DataType {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let value = match self {
            DataType::Zstd => "zstd",
        };
        value.bind(statement, start_index)
    }
}

impl Column for DataType {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (value, next_index) = String::column(statement, start_index)?;
        let data_type = match value.as_str() {
            "zstd" => DataType::Zstd,
            _ => anyhow::bail!("Unknown data type: {}", value),
        };
        Ok((data_type, next_index))
    }
}

pub(crate) struct ThreadsDatabase {
    executor: BackgroundExecutor,
    connection: Arc<Mutex<Connection>>,
    /// In production, saves take real time (serialization, zstd, disk I/O) while
    /// the user keeps typing, so new save requests routinely arrive mid-write.
    /// The test executor completes writes instantly, so tests use this gate to
    /// hold a write in flight and interleave more save requests with it.
    #[cfg(test)]
    write_gate: Mutex<Option<Shared<futures::channel::oneshot::Receiver<()>>>>,
}

struct GlobalThreadsDatabase(Shared<Task<Result<Arc<ThreadsDatabase>, Arc<anyhow::Error>>>>);

impl Global for GlobalThreadsDatabase {}

impl ThreadsDatabase {
    pub fn connect(cx: &mut App) -> Shared<Task<Result<Arc<ThreadsDatabase>, Arc<anyhow::Error>>>> {
        if cx.has_global::<GlobalThreadsDatabase>() {
            return cx.global::<GlobalThreadsDatabase>().0.clone();
        }
        let executor = cx.background_executor().clone();
        let task = executor
            .spawn({
                let executor = executor.clone();
                async move {
                    match ThreadsDatabase::new(executor) {
                        Ok(db) => Ok(Arc::new(db)),
                        Err(err) => Err(Arc::new(err)),
                    }
                }
            })
            .shared();

        cx.set_global(GlobalThreadsDatabase(task.clone()));
        task
    }

    pub fn new(executor: BackgroundExecutor) -> Result<Self> {
        let connection = if *ZED_STATELESS {
            Connection::open_memory(Some("THREAD_FALLBACK_DB"))
        } else if cfg!(any(feature = "test-support", test)) {
            // rust stores the name of the test on the current thread.
            // We use this to automatically create a database that will
            // be shared within the test (for the test_retrieve_old_thread)
            // but not with concurrent tests.
            //
            // zed-kask: D28 — in test builds the override is NOT checked
            // here because the global `Mutex` would leak between concurrent
            // tests (one test setting the override would redirect other
            // tests' `ThreadsDatabase::new` to a file path they don't expect,
            // causing SQLite schema-migration races).
            let thread = std::thread::current();
            let test_name = thread.name();
            Connection::open_memory(Some(&format!(
                "THREAD_FALLBACK_{}",
                test_name.unwrap_or_default()
            )))
        } else {
            // zed-kask: D28 — Standardized Artifact Storage.
            // Archived chat threads live under the kask data root
            // (`{data_dir}/threads/threads.db`), resolved via the kask-side
            // `threads_db_path_override` hook (set by `kask_bridge` at
            // startup), NOT the upstream `paths::data_dir()` (which points at
            // the zed-kask app data root `~/.local/share/zed-kask/`). This
            // keeps all kask artifacts under one rooted tree per the
            // standardized storage layout
            // (kask/docs/architecture/standardized-artifact-storage.md).
            //
            // Pre-release: no back-compat. The override is always wired early
            // in `main.rs` (user-independent), so this branch is the
            // production path.
            let kask_path = crate::threads_db_path_override().with_context(|| {
                "threads_db_path_override not wired — main.rs must call \
                 set_threads_db_path_override before constructing ThreadsDatabase"
            })?;
            if let Some(parent) = kask_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Connection::open_file(&kask_path.to_string_lossy())
        };

        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                folder_paths TEXT,
                folder_paths_order TEXT,
                summary TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                data_type TEXT NOT NULL,
                data BLOB NOT NULL,
                created_at TEXT,
                format INTEGER NOT NULL DEFAULT 0
            )
        "})?()
        .map_err(|e| e.context("Failed to create threads table"))?;

        // zed-kask: D28 — per-message thread storage. Messages live one row
        // each in `thread_messages`; the `threads.data` blob carries only
        // the thread metadata (a DbThread serialized with an empty
        // messages list) for new-format threads. `threads.format` is the
        // format marker: 0 = legacy whole-thread blob, 1 = per-message
        // rows. Legacy threads migrate lazily on first load (idempotent
        // upserts inside one transaction); the legacy blob is preserved
        // until the thread's next save overwrites it with metadata, so a
        // crash never loses data.
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS thread_messages (
                thread_id TEXT NOT NULL,
                ix INTEGER NOT NULL,
                data TEXT NOT NULL,
                PRIMARY KEY (thread_id, ix)
            )
        "})?()
        .map_err(|e| e.context("Failed to create thread_messages table"))?;

        // Additive column migration for databases created before
        // per-message storage (PRAGMA-checked, the D87 pattern). Scoped so
        // the check statement drops before the connection moves into the
        // database handle.
        let has_format_column = {
            let mut format_check = connection.select_bound::<(), i64>(indoc! {"
                SELECT COUNT(*) FROM pragma_table_info('threads')
                WHERE name = 'format'
            "})?;
            format_check(())?.first().copied().unwrap_or(0) > 0
        };
        if !has_format_column {
            connection.exec("ALTER TABLE threads ADD COLUMN format INTEGER NOT NULL DEFAULT 0")?()
                .map_err(|e| e.context("Failed to add format column"))?;
        }

        let db = Self {
            executor,
            connection: Arc::new(Mutex::new(connection)),
            #[cfg(test)]
            write_gate: Mutex::new(None),
        };

        Ok(db)
    }

    fn save_thread_sync(
        connection: &Arc<Mutex<Connection>>,
        id: acp::SessionId,
        thread: DbThread,
        folder_paths: &PathList,
    ) -> Result<()> {
        const COMPRESSION_LEVEL: i32 = 3;

        #[derive(Serialize)]
        struct SerializedThread {
            #[serde(flatten)]
            thread: DbThread,
            version: &'static str,
        }

        let mut thread = thread;
        let title = thread.title.to_string();
        let updated_at = thread.updated_at.to_rfc3339();
        let parent_id = thread
            .subagent_context
            .as_ref()
            .map(|ctx| ctx.parent_thread_id.0.clone());
        let serialized_folder_paths = folder_paths.serialize();
        let (folder_paths_str, folder_paths_order_str): (Option<String>, Option<String>) =
            if folder_paths.is_empty() {
                (None, None)
            } else {
                (
                    Some(serialized_folder_paths.paths),
                    Some(serialized_folder_paths.order),
                )
            };

        // zed-kask: D28 — per-message storage. The metadata blob is the
        // thread serialized with an EMPTY messages list (every field except
        // the messages); the messages are upserted one row each. Upserting
        // all messages on every save is deliberate: messages mutate in
        // place after being saved (tool results fill in mid-turn), so
        // append-only incremental writes would leave stale rows. The
        // dominant legacy save cost — zstd-compressing the whole thread
        // on every save — is gone; the metadata blob is small.
        let messages = std::mem::take(&mut thread.messages);
        let json_data = serde_json::to_string(&SerializedThread {
            thread,
            version: DbThread::VERSION,
        })?;

        let compressed = zstd::encode_all(json_data.as_bytes(), COMPRESSION_LEVEL)?;
        let data_type = DataType::Zstd;
        let data = compressed;

        // Use the thread's updated_at as created_at for new threads.
        // This ensures that the creation time reflects when the thread was conceptually
        // created, not when it was saved to the database.
        let created_at = updated_at.clone();

        let connection = connection.lock();

        // One transaction: the threads row and the message rows move
        // together, so a crash mid-save can never leave a format marker
        // pointing at partially-written rows.
        connection.exec("BEGIN IMMEDIATE")?()
            .map_err(|e| e.context("Failed to begin thread save transaction"))?;
        match Self::save_thread_in_transaction(
            &connection,
            &id,
            &title,
            &updated_at,
            parent_id,
            folder_paths_str,
            folder_paths_order_str,
            data_type,
            data,
            &created_at,
            &messages,
        ) {
            Ok(()) => {
                connection.exec("COMMIT")?()
                    .map_err(|e| e.context("Failed to commit thread save transaction"))?;
                Ok(())
            }
            Err(error) => {
                Self::rollback(&connection, "thread save");
                Err(error)
            }
        }
    }

    /// Best-effort rollback on an already-failing path: the original error
    /// is the one the caller sees; a rollback failure is logged, never
    /// silently discarded (.rules).
    fn rollback(connection: &Connection, operation: &str) {
        match connection.exec("ROLLBACK") {
            Ok(mut rollback) => {
                if let Err(error) = rollback() {
                    log::warn!("failed to roll back {operation} transaction: {error:?}");
                }
            }
            Err(error) => {
                log::warn!("failed to prepare {operation} rollback: {error:?}");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn save_thread_in_transaction(
        connection: &Connection,
        id: &acp::SessionId,
        title: &str,
        updated_at: &str,
        parent_id: Option<Arc<str>>,
        folder_paths: Option<String>,
        folder_paths_order: Option<String>,
        data_type: DataType,
        data: Vec<u8>,
        created_at: &str,
        messages: &[Arc<DbMessage>],
    ) -> Result<()> {
        let mut insert = connection.exec_bound::<(Arc<str>, Option<Arc<str>>, Option<String>, Option<String>, String, String, DataType, Vec<u8>, String, i64)>(indoc! {"
            INSERT INTO threads (id, parent_id, folder_paths, folder_paths_order, summary, updated_at, data_type, data, created_at, format)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                parent_id = excluded.parent_id,
                folder_paths = excluded.folder_paths,
                folder_paths_order = excluded.folder_paths_order,
                summary = excluded.summary,
                updated_at = excluded.updated_at,
                data_type = excluded.data_type,
                data = excluded.data,
                created_at = excluded.created_at,
                format = excluded.format
        "})?;
        insert((
            id.0.clone(),
            parent_id,
            folder_paths,
            folder_paths_order,
            title.to_string(),
            updated_at.to_string(),
            data_type,
            data,
            created_at.to_string(),
            1,
        ))?;

        Self::write_message_rows(connection, id, messages)
    }

    /// zed-kask: D28 — write one row per message (upsert all: messages
    /// mutate in place after being saved, so append-only writes would
    /// leave stale rows) and trim any rows at or beyond the new length.
    fn write_message_rows(
        connection: &Connection,
        id: &acp::SessionId,
        messages: &[Arc<DbMessage>],
    ) -> Result<()> {
        let mut trim = connection.exec_bound::<(Arc<str>, i64)>(indoc! {"
            DELETE FROM thread_messages WHERE thread_id = ? AND ix >= ?
        "})?;
        trim((id.0.clone(), messages.len() as i64))?;

        let mut upsert_message = connection.exec_bound::<(Arc<str>, i64, String)>(indoc! {"
            INSERT INTO thread_messages (thread_id, ix, data)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(thread_id, ix) DO UPDATE SET data = excluded.data
        "})?;
        for (ix, message) in messages.iter().enumerate() {
            let json = serde_json::to_string(message)
                .with_context(|| format!("failed to serialize message {ix} of thread {id:?}"))?;
            upsert_message((id.0.clone(), ix as i64, json))?;
        }
        Ok(())
    }

    pub fn list_threads(&self) -> Task<Result<Vec<DbThreadMetadata>>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let connection = connection.lock();

            let mut select = connection
                .select_bound::<(), (Arc<str>, Option<Arc<str>>, Option<String>, Option<String>, String, String, Option<String>)>(indoc! {"
                SELECT id, parent_id, folder_paths, folder_paths_order, summary, updated_at, created_at FROM threads ORDER BY updated_at DESC, created_at DESC
            "})?;

            let rows = select(())?;
            let mut threads = Vec::new();

            for (id, parent_id, folder_paths, folder_paths_order, summary, updated_at, created_at) in rows {
                let folder_paths = folder_paths
                    .map(|paths| {
                        PathList::deserialize(&util::path_list::SerializedPathList {
                            paths,
                            order: folder_paths_order.unwrap_or_default(),
                        })
                    })
                    .unwrap_or_default();
                let created_at = created_at
                    .as_deref()
                    .map(DateTime::parse_from_rfc3339)
                    .transpose()?
                    .map(|dt| dt.with_timezone(&Utc));

                threads.push(DbThreadMetadata {
                    id: acp::SessionId::new(id),
                    parent_session_id: parent_id.map(acp::SessionId::new),
                    title: summary.into(),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
                    created_at,
                    folder_paths,
                });
            }

            Ok(threads)
        })
    }

    pub fn load_thread(&self, id: acp::SessionId) -> Task<Result<Option<DbThread>>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            // [DIAG-open] — the S1 first-open decomposition probe
            // (two-symptom research doc §4 step 2): lock-wait (S1-H2 — a
            // concurrent turn-end save holds this one shared connection
            // while writing the whole-thread blob) split from
            // read+deserialize (S1-H1 — the whole-thread zstd+JSON
            // round-trip). One line per open; opens are user-initiated and
            // infrequent. Remove with the S1 fix (grep DIAG-open).
            let lock_started = std::time::Instant::now();
            let connection = connection.lock();
            let lock_wait_ms = lock_started.elapsed().as_millis();
            let mut select = connection.select_bound::<Arc<str>, (DataType, Vec<u8>, i64)>(indoc! {"
                SELECT data_type, data, format FROM threads WHERE id = ? LIMIT 1
            "})?;

            let rows = select(id.0.clone())?;
            let read_parse_started = std::time::Instant::now();
            let blob_bytes = rows.iter().next().map(|(_, data, _)| data.len());
            let mut legacy_migrated = false;
            let result = if let Some((data_type, data, format)) = rows.into_iter().next() {
                if format > 0 {
                    // zed-kask: D28 — per-message format: the blob is the
                    // metadata (DbThread with an empty messages list) and
                    // the messages are read row by row.
                    Ok(Some(Self::load_thread_messages(
                        &connection, &id, data_type, data,
                    )?))
                } else {
                    // Legacy whole-thread blob: parse it, then migrate it
                    // to per-message rows in place (idempotent; the blob
                    // itself is preserved until the thread's next save).
                    let thread = Self::deserialize_thread(data_type, data)?;
                    Self::migrate_legacy_thread(&connection, &id, &thread)?;
                    legacy_migrated = true;
                    Ok(Some(thread))
                }
            } else {
                Ok(None)
            };
            log::info!(
                "[DIAG-open] db_load thread_id={} lock_wait_ms={} read_parse_ms={} blob_bytes={} legacy_migrated={}",
                &*id.0,
                lock_wait_ms,
                read_parse_started.elapsed().as_millis(),
                blob_bytes.unwrap_or(0),
                legacy_migrated,
            );
            result
        })
    }

    /// zed-kask: D28 — read a per-message-format thread: the metadata blob
    /// plus one row per message, in index order. A gap in the row indexes
    /// cannot be produced by a save (saves are transactional), so it is
    /// corruption and fails loudly instead of silently misassembling the
    /// thread.
    fn load_thread_messages(
        connection: &Connection,
        id: &acp::SessionId,
        data_type: DataType,
        data: Vec<u8>,
    ) -> Result<DbThread> {
        let mut thread = Self::deserialize_thread(data_type, data)?;
        let mut select = connection.select_bound::<Arc<str>, (i64, String)>(indoc! {"
            SELECT ix, data FROM thread_messages WHERE thread_id = ? ORDER BY ix
        "})?;
        let rows = select(id.0.clone())?;
        let mut messages = Vec::with_capacity(rows.len());
        for (ix, json) in rows {
            if ix != messages.len() as i64 {
                anyhow::bail!(
                    "message row gap at index {ix} for thread {id:?} (expected {})",
                    messages.len()
                );
            }
            let message: Arc<DbMessage> = serde_json::from_str(&json)
                .with_context(|| format!("failed to parse message {ix} of thread {id:?}"))?;
            messages.push(message);
        }
        thread.messages = messages;
        Ok(thread)
    }

    /// zed-kask: D28 — lazy legacy migration on first load: write one row
    /// per message and set the format marker, in one transaction. The
    /// legacy blob in `threads.data` is NOT touched — it is overwritten
    /// with metadata by the thread's next save, and until then it is the
    /// crash-recovery source (a crash mid-migration rolls back; the next
    /// open re-migrates).
    fn migrate_legacy_thread(
        connection: &Connection,
        id: &acp::SessionId,
        thread: &DbThread,
    ) -> Result<()> {
        connection.exec("BEGIN IMMEDIATE")?()
            .map_err(|e| e.context("Failed to begin thread migration transaction"))?;
        let result = (|| -> Result<()> {
            Self::write_message_rows(connection, id, &thread.messages)?;
            let mut set_format = connection.exec_bound::<(i64, Arc<str>)>(indoc! {"
                UPDATE threads SET format = ? WHERE id = ?
            "})?;
            set_format((1, id.0.clone()))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                connection.exec("COMMIT")?()
                    .map_err(|e| e.context("Failed to commit thread migration transaction"))?;
                Ok(())
            }
            Err(error) => {
                Self::rollback(connection, "thread migration");
                Err(error)
            }
        }
    }

    pub fn save_thread(
        &self,
        id: acp::SessionId,
        thread: DbThread,
        folder_paths: PathList,
    ) -> Task<Result<()>> {
        let connection = self.connection.clone();
        #[cfg(test)]
        let write_gate = self.write_gate.lock().clone();

        self.executor.spawn(async move {
            #[cfg(test)]
            if let Some(write_gate) = write_gate {
                write_gate.await.ok();
            }
            Self::save_thread_sync(&connection, id, thread, &folder_paths)
        })
    }

    #[cfg(test)]
    pub fn set_write_gate(&self, gate: futures::channel::oneshot::Receiver<()>) {
        *self.write_gate.lock() = Some(gate.shared());
    }

    fn deserialize_thread(data_type: DataType, data: Vec<u8>) -> Result<DbThread> {
        let json_data = match data_type {
            DataType::Zstd => {
                let decompressed = zstd::decode_all(&data[..])?;
                String::from_utf8(decompressed)?
            }
        };
        DbThread::from_json(json_data.as_bytes())
    }

    fn sandboxed_terminal_temp_dir(data_type: DataType, data: Vec<u8>) -> Option<PathBuf> {
        match Self::deserialize_thread(data_type, data) {
            Ok(thread) => thread.sandboxed_terminal_temp_dir,
            Err(error) => {
                log::warn!("failed to deserialize thread before deleting it: {error:#}");
                None
            }
        }
    }

    fn remove_sandboxed_terminal_temp_dir(temp_dir: PathBuf) {
        match std::fs::remove_dir_all(&temp_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!(
                    "failed to remove sandboxed terminal temp directory {}: {error}",
                    temp_dir.display()
                );
            }
        }
    }

    pub fn delete_thread(&self, id: acp::SessionId) -> Task<Result<()>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let sandboxed_terminal_temp_dirs = {
                let connection = connection.lock();

                let mut select_children =
                    connection.select_bound::<Arc<str>, Arc<str>>(indoc! {"
                    SELECT id FROM threads WHERE parent_id = ?
                "})?;

                // Collect target thread together with all of its transitive
                // subagent threads
                let mut ids_to_delete = vec![id.0.clone()];
                let mut frontier = vec![id.0.clone()];
                while let Some(parent) = frontier.pop() {
                    for child in select_children(parent)? {
                        ids_to_delete.push(child.clone());
                        frontier.push(child);
                    }
                }

                let mut select =
                    connection.select_bound::<Arc<str>, (DataType, Vec<u8>)>(indoc! {"
                    SELECT data_type, data FROM threads WHERE id = ? LIMIT 1
                "})?;

                let mut delete = connection.exec_bound::<Arc<str>>(indoc! {"
                    DELETE FROM threads WHERE id = ?
                "})?;

                // zed-kask: D28 — per-message rows go with their thread.
                let mut delete_messages = connection.exec_bound::<Arc<str>>(indoc! {"
                    DELETE FROM thread_messages WHERE thread_id = ?
                "})?;

                let mut sandboxed_terminal_temp_dirs = Vec::new();
                for thread_id in ids_to_delete {
                    if let Some(temp_dir) = select(thread_id.clone())?.into_iter().next().and_then(
                        |(data_type, data)| Self::sandboxed_terminal_temp_dir(data_type, data),
                    ) {
                        sandboxed_terminal_temp_dirs.push(temp_dir);
                    }
                    delete_messages(thread_id.clone())?;
                    delete(thread_id)?;
                }

                sandboxed_terminal_temp_dirs
            };

            for temp_dir in sandboxed_terminal_temp_dirs {
                Self::remove_sandboxed_terminal_temp_dir(temp_dir);
            }

            Ok(())
        })
    }

    pub fn delete_threads(&self) -> Task<Result<()>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let sandboxed_terminal_temp_dirs = {
                let connection = connection.lock();

                let mut select = connection.select_bound::<(), (DataType, Vec<u8>)>(indoc! {"
                    SELECT data_type, data FROM threads
                "})?;

                let sandboxed_terminal_temp_dirs = select(())?
                    .into_iter()
                    .filter_map(|(data_type, data)| {
                        Self::sandboxed_terminal_temp_dir(data_type, data)
                    })
                    .collect::<Vec<_>>();

                let mut delete = connection.exec_bound::<()>(indoc! {"
                    DELETE FROM threads
                "})?;

                // zed-kask: D28 — per-message rows go with their threads.
                let mut delete_messages = connection.exec_bound::<()>(indoc! {"
                    DELETE FROM thread_messages
                "})?;

                delete_messages(())?;
                delete(())?;

                sandboxed_terminal_temp_dirs
            };

            for temp_dir in sandboxed_terminal_temp_dirs {
                Self::remove_sandboxed_terminal_temp_dir(temp_dir);
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use collections::HashMap;
    use gpui::TestAppContext;
    use std::sync::Arc;

    #[test]
    fn test_shared_thread_roundtrip() {
        let original = SharedThread {
            title: "Test Thread".into(),
            messages: vec![],
            updated_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            model: None,
            delegation_authority: Some(crate::DelegationAuthority::from_mcp_tools(&[
                "a/read".into()
            ])),
            version: SharedThread::VERSION.to_string(),
        };

        let bytes = original.to_bytes().expect("Failed to serialize");
        let restored = SharedThread::from_bytes(&bytes).expect("Failed to deserialize");

        assert_eq!(restored.title, original.title);
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.updated_at, original.updated_at);
        assert_eq!(restored.delegation_authority, original.delegation_authority);
        let imported = restored.to_db_thread();
        let exported = SharedThread::from_db_thread(&imported);
        assert_eq!(exported.delegation_authority, original.delegation_authority);
    }

    fn session_id(value: &str) -> acp::SessionId {
        acp::SessionId::new(Arc::<str>::from(value))
    }

    fn make_thread(title: &str, updated_at: DateTime<Utc>) -> DbThread {
        DbThread {
            title: title.to_string().into(),
            messages: Vec::new(),
            updated_at,
            detailed_summary: None,
            initial_project_snapshot: None,
            cumulative_token_usage: Default::default(),
            request_token_usage: HashMap::default(),
            model: None,
            profile: None,
            subagent_context: None,
            speed: None,
            reasoning_effort: None,
            draft_prompt: None,
            ui_scroll_position: None,
            sandboxed_terminal_temp_dir: None,
            sandbox_grants: DbSandboxGrants::default(),
            delegation_authority: None,
        }
    }

    #[gpui::test]
    async fn test_list_threads_orders_by_created_at(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let older_id = session_id("thread-a");
        let newer_id = session_id("thread-b");

        let older_thread = make_thread(
            "Thread A",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        let newer_thread = make_thread(
            "Thread B",
            Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
        );

        database
            .save_thread(older_id.clone(), older_thread, PathList::default())
            .await
            .unwrap();
        database
            .save_thread(newer_id.clone(), newer_thread, PathList::default())
            .await
            .unwrap();

        let entries = database.list_threads().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, newer_id);
        assert_eq!(entries[1].id, older_id);
    }

    #[gpui::test]
    async fn test_save_thread_replaces_metadata(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let thread_id = session_id("thread-a");
        let original_thread = make_thread(
            "Thread A",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        let updated_thread = make_thread(
            "Thread B",
            Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
        );

        database
            .save_thread(thread_id.clone(), original_thread, PathList::default())
            .await
            .unwrap();
        database
            .save_thread(thread_id.clone(), updated_thread, PathList::default())
            .await
            .unwrap();

        let entries = database.list_threads().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, thread_id);
        assert_eq!(entries[0].title.as_ref(), "Thread B");
        assert_eq!(
            entries[0].updated_at,
            Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap()
        );
        assert!(
            entries[0].created_at.is_some(),
            "created_at should be populated"
        );
    }

    // zed-kask: D28 — per-message storage pins.

    fn resume_messages(count: usize) -> Vec<Arc<DbMessage>> {
        (0..count).map(|_| Arc::new(DbMessage::Resume)).collect()
    }

    fn thread_format(database: &ThreadsDatabase, id: &acp::SessionId) -> i64 {
        let connection = database.connection.lock();
        let mut select = connection
            .select_bound::<Arc<str>, i64>("SELECT format FROM threads WHERE id = ?")
            .unwrap();
        select(id.0.clone()).unwrap().first().copied().unwrap_or(-1)
    }

    fn message_row_count(database: &ThreadsDatabase, id: &acp::SessionId) -> i64 {
        let connection = database.connection.lock();
        let mut select = connection
            .select_bound::<Arc<str>, i64>(
                "SELECT COUNT(*) FROM thread_messages WHERE thread_id = ?",
            )
            .unwrap();
        select(id.0.clone()).unwrap().first().copied().unwrap_or(-1)
    }

    #[gpui::test]
    async fn test_per_message_round_trip(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("per-message");
        let mut thread = make_thread(
            "Per Message",
            Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap(),
        );
        thread.messages = resume_messages(3);

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id.clone())
            .await
            .unwrap()
            .expect("thread should load");
        assert_eq!(loaded.title.as_ref(), "Per Message");
        assert_eq!(loaded.messages.len(), 3);
        assert!(
            loaded
                .messages
                .iter()
                .all(|m| matches!(**m, DbMessage::Resume)),
            "messages should round-trip"
        );
        assert_eq!(thread_format(&database, &thread_id), 1, "format marker set");
        assert_eq!(
            message_row_count(&database, &thread_id),
            3,
            "one row per message"
        );
    }

    #[gpui::test]
    async fn test_save_overwrites_message_rows_in_place(cx: &mut TestAppContext) {
        // Messages mutate after being saved (tool results fill in mid-turn),
        // so a save must overwrite existing rows, not append; and a
        // shrinking message list must not leave orphan rows.
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("upsert-all");
        let mut first = make_thread("Upsert", Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap());
        first.messages = resume_messages(2);
        database
            .save_thread(thread_id.clone(), first, PathList::default())
            .await
            .unwrap();

        let mut second = make_thread("Upsert", Utc.with_ymd_and_hms(2024, 2, 2, 0, 0, 0).unwrap());
        second.messages = resume_messages(3);
        database
            .save_thread(thread_id.clone(), second, PathList::default())
            .await
            .unwrap();
        assert_eq!(message_row_count(&database, &thread_id), 3);

        let mut third = make_thread("Upsert", Utc.with_ymd_and_hms(2024, 2, 3, 0, 0, 0).unwrap());
        third.messages = resume_messages(1);
        database
            .save_thread(thread_id.clone(), third, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id.clone())
            .await
            .unwrap()
            .expect("thread should load");
        assert_eq!(loaded.messages.len(), 1, "shrinking save trims rows");
        assert_eq!(message_row_count(&database, &thread_id), 1);
    }

    #[gpui::test]
    async fn test_legacy_thread_migrates_on_first_load(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("legacy");
        let mut thread = make_thread("Legacy", Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        thread.messages = resume_messages(2);

        // Write a legacy whole-thread blob (format 0) by hand — the
        // pre-D28 storage format.
        #[derive(Serialize)]
        struct LegacySerializedThread<'a> {
            #[serde(flatten)]
            thread: &'a DbThread,
            version: &'static str,
        }
        let json = serde_json::to_string(&LegacySerializedThread {
            thread: &thread,
            version: DbThread::VERSION,
        })
        .unwrap();
        let blob = zstd::encode_all(json.as_bytes(), 3).unwrap();
        {
            let connection = database.connection.lock();
            let mut insert = connection
                .exec_bound::<(Arc<str>, String, String, DataType, Vec<u8>, String)>(indoc! {"
                    INSERT INTO threads (id, summary, updated_at, data_type, data, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "})
                .unwrap();
            insert((
                thread_id.0.clone(),
                "Legacy".to_string(),
                thread.updated_at.to_rfc3339(),
                DataType::Zstd,
                blob,
                thread.updated_at.to_rfc3339(),
            ))
            .unwrap();
        }

        // First load returns the thread AND migrates it to rows.
        let loaded = database
            .load_thread(thread_id.clone())
            .await
            .unwrap()
            .expect("legacy thread should load");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(
            thread_format(&database, &thread_id),
            1,
            "migrated on first load"
        );
        assert_eq!(message_row_count(&database, &thread_id), 2);

        // Second load reads the migrated rows.
        let reloaded = database
            .load_thread(thread_id.clone())
            .await
            .unwrap()
            .expect("migrated thread should load");
        assert_eq!(reloaded.messages.len(), 2);
        assert_eq!(reloaded.title.as_ref(), "Legacy");
    }

    #[gpui::test]
    async fn test_message_row_gap_fails_loudly(cx: &mut TestAppContext) {
        // A gap in the row indexes is corruption (saves are transactional,
        // so they cannot produce one); the load must fail loudly rather
        // than silently misassemble the thread.
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("gap");
        let mut thread = make_thread("Gap", Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap());
        thread.messages = resume_messages(3);
        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        {
            let connection = database.connection.lock();
            let mut delete_middle = connection
                .exec_bound::<(Arc<str>, i64)>(
                    "DELETE FROM thread_messages WHERE thread_id = ? AND ix = ?",
                )
                .unwrap();
            delete_middle((thread_id.0.clone(), 1)).unwrap();
        }

        let error = database
            .load_thread(thread_id.clone())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("message row gap"),
            "a row gap must fail loudly, got: {error}"
        );
    }

    #[gpui::test]
    async fn test_delete_thread_removes_message_rows(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("delete-rows");
        let mut thread = make_thread("Delete", Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap());
        thread.messages = resume_messages(2);
        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();
        assert_eq!(message_row_count(&database, &thread_id), 2);

        database.delete_thread(thread_id.clone()).await.unwrap();

        assert_eq!(
            message_row_count(&database, &thread_id),
            0,
            "message rows go with their thread"
        );
    }

    #[test]
    fn test_subagent_context_defaults_to_none() {
        let json = r#"{
            "title": "Old Thread",
            "messages": [],
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let db_thread: DbThread = serde_json::from_str(json).expect("Failed to deserialize");

        assert!(
            db_thread.subagent_context.is_none(),
            "Legacy threads without subagent_context should default to None"
        );
    }

    #[test]
    fn test_draft_prompt_defaults_to_none() {
        let json = r#"{
            "title": "Old Thread",
            "messages": [],
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let db_thread: DbThread = serde_json::from_str(json).expect("Failed to deserialize");

        assert!(
            db_thread.draft_prompt.is_none(),
            "Legacy threads without draft_prompt field should default to None"
        );
    }

    #[test]
    fn test_sandboxed_terminal_temp_dir_defaults_to_none() {
        let json = r#"{
            "title": "Old Thread",
            "messages": [],
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let db_thread: DbThread = serde_json::from_str(json).expect("Failed to deserialize");

        assert!(
            db_thread.sandboxed_terminal_temp_dir.is_none(),
            "Legacy threads without sandboxed_terminal_temp_dir should default to None"
        );
    }

    #[test]
    fn test_sandbox_grants_default_when_absent() {
        let json = r#"{
            "title": "Old Thread",
            "messages": [],
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let db_thread: DbThread = serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(
            db_thread.sandbox_grants,
            DbSandboxGrants::default(),
            "Legacy threads without sandbox_grants should default to empty grants"
        );
    }

    #[gpui::test]
    async fn test_sandbox_grants_roundtrip_through_save_load(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("sandbox-grants-thread");
        let mut thread = make_thread(
            "Sandbox Grants Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        let grants = DbSandboxGrants {
            write_paths: vec![
                // A legacy bare-string grant (no resolved canonical) and a grant
                // carrying its resolved canonical, to exercise both forms of the
                // string-or-object round-trip.
                settings::GrantedWritePath::from_requested(PathBuf::from("/tmp/build")),
                settings::GrantedWritePath::resolved(
                    PathBuf::from("/tmp/link"),
                    PathBuf::from("/tmp/real"),
                ),
            ],
            network_hosts: vec!["github.com".to_string(), "*.npmjs.org".to_string()],
            network_any_host: false,
            allow_fs_write_all: false,
            unsandboxed: true,
            sandbox_fallback: true,
        };
        thread.sandbox_grants = grants.clone();

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id)
            .await
            .unwrap()
            .expect("thread should exist");
        assert_eq!(loaded.sandbox_grants, grants);
    }

    #[gpui::test]
    async fn test_sandboxed_terminal_temp_dir_roundtrips_through_save_load(
        cx: &mut TestAppContext,
    ) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("sandbox-temp-dir-thread");
        let temp_dir = tempfile::Builder::new()
            .prefix("zed-agent-terminal-test-")
            .tempdir()
            .unwrap()
            .keep();
        let mut thread = make_thread(
            "Sandbox Temp Dir Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        thread.sandboxed_terminal_temp_dir = Some(temp_dir.clone());

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id)
            .await
            .unwrap()
            .expect("thread should exist");
        assert_eq!(loaded.sandboxed_terminal_temp_dir, Some(temp_dir.clone()));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[gpui::test]
    async fn test_delete_thread_removes_sandboxed_terminal_temp_dir(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();
        let thread_id = session_id("sandbox-temp-dir-delete-thread");
        let temp_dir = tempfile::Builder::new()
            .prefix("zed-agent-terminal-test-")
            .tempdir()
            .unwrap()
            .keep();
        std::fs::write(temp_dir.join("sentinel"), b"content").unwrap();
        let mut thread = make_thread(
            "Sandbox Temp Dir Delete Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        thread.sandboxed_terminal_temp_dir = Some(temp_dir.clone());

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();
        database.delete_thread(thread_id).await.unwrap();

        assert!(!temp_dir.exists());
    }

    #[gpui::test]
    async fn test_delete_thread_deletes_subagent_threads(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let parent_id = session_id("parent-thread");
        let child_id = session_id("child-thread");
        let grandchild_id = session_id("grandchild-thread");
        let unrelated_id = session_id("unrelated-thread");

        let parent_thread = make_thread(
            "Parent Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );

        let mut child_thread = make_thread(
            "Child Subagent Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        child_thread.subagent_context = Some(crate::SubagentContext {
            parent_thread_id: parent_id.clone(),
            depth: 1,
        });

        let mut grandchild_thread = make_thread(
            "Grandchild Subagent Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        grandchild_thread.subagent_context = Some(crate::SubagentContext {
            parent_thread_id: child_id.clone(),
            depth: 2,
        });

        let unrelated_thread = make_thread(
            "Unrelated Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );

        for (id, thread) in [
            (parent_id.clone(), parent_thread),
            (child_id.clone(), child_thread),
            (grandchild_id.clone(), grandchild_thread),
            (unrelated_id.clone(), unrelated_thread),
        ] {
            database
                .save_thread(id, thread, PathList::default())
                .await
                .unwrap();
        }

        database.delete_thread(parent_id.clone()).await.unwrap();

        let remaining = database.list_threads().await.unwrap();
        let remaining_ids: Vec<_> = remaining.iter().map(|thread| thread.id.clone()).collect();
        assert_eq!(remaining_ids, vec![unrelated_id]);
    }

    #[gpui::test]
    async fn test_subagent_context_roundtrips_through_save_load(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let parent_id = session_id("parent-thread");
        let child_id = session_id("child-thread");

        let mut child_thread = make_thread(
            "Subagent Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        child_thread.subagent_context = Some(crate::SubagentContext {
            parent_thread_id: parent_id.clone(),
            depth: 2,
        });

        database
            .save_thread(child_id.clone(), child_thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(child_id)
            .await
            .unwrap()
            .expect("thread should exist");

        let context = loaded
            .subagent_context
            .expect("subagent_context should be restored");
        assert_eq!(context.parent_thread_id, parent_id);
        assert_eq!(context.depth, 2);
    }

    #[gpui::test]
    async fn test_non_subagent_thread_has_no_subagent_context(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let thread_id = session_id("regular-thread");
        let thread = make_thread(
            "Regular Thread",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id)
            .await
            .unwrap()
            .expect("thread should exist");

        assert!(
            loaded.subagent_context.is_none(),
            "Regular threads should have no subagent_context"
        );
    }

    #[gpui::test]
    async fn test_folder_paths_roundtrip(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let thread_id = session_id("folder-thread");
        let thread = make_thread(
            "Folder Thread",
            Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap(),
        );

        let folder_paths = PathList::new(&[
            std::path::PathBuf::from("/home/user/project-a"),
            std::path::PathBuf::from("/home/user/project-b"),
        ]);

        database
            .save_thread(thread_id.clone(), thread, folder_paths.clone())
            .await
            .unwrap();

        let threads = database.list_threads().await.unwrap();
        assert_eq!(threads.len(), 1);
    }

    #[gpui::test]
    async fn test_folder_paths_empty_when_not_set(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let thread_id = session_id("no-folder-thread");
        let thread = make_thread(
            "No Folder Thread",
            Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap(),
        );

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let threads = database.list_threads().await.unwrap();
        assert_eq!(threads.len(), 1);
    }

    #[test]
    fn test_scroll_position_defaults_to_none() {
        let json = r#"{
            "title": "Old Thread",
            "messages": [],
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let db_thread: DbThread = serde_json::from_str(json).expect("Failed to deserialize");

        assert!(
            db_thread.ui_scroll_position.is_none(),
            "Legacy threads without scroll_position field should default to None"
        );
    }

    #[gpui::test]
    async fn test_scroll_position_roundtrips_through_save_load(cx: &mut TestAppContext) {
        let database = ThreadsDatabase::new(cx.executor()).unwrap();

        let thread_id = session_id("thread-with-scroll");

        let mut thread = make_thread(
            "Thread With Scroll",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        );
        thread.ui_scroll_position = Some(SerializedScrollPosition {
            item_ix: 42,
            offset_in_item: 13.5,
        });

        database
            .save_thread(thread_id.clone(), thread, PathList::default())
            .await
            .unwrap();

        let loaded = database
            .load_thread(thread_id)
            .await
            .unwrap()
            .expect("thread should exist");

        let scroll = loaded
            .ui_scroll_position
            .expect("scroll_position should be restored");
        assert_eq!(scroll.item_ix, 42);
        assert!((scroll.offset_in_item - 13.5).abs() < f32::EPSILON);
    }

    // zed-kask: D28 — pins the canonical archived-threads DB path.
    // Tests the accessor through a scoped test-thread override. Production
    // continues to use the ProcessGlobal composition-root hook. This does not
    // construct a `ThreadsDatabase`: test builds deliberately use isolated
    // in-memory connections while production requires the wired path.
    #[test]
    fn test_threads_db_override_hook_round_trips() {
        let baseline = crate::threads_db_path_override();
        let sentinel = std::path::PathBuf::from("/tmp/kask-test-threads.db");
        {
            let _override = crate::scoped_threads_db_path_override_for_test(sentinel.clone());
            assert_eq!(
                crate::threads_db_path_override(),
                Some(sentinel),
                "the current test thread must observe its path override"
            );
        }
        assert_eq!(
            crate::threads_db_path_override(),
            baseline,
            "dropping the scoped override must restore prior state"
        );
    }
}
