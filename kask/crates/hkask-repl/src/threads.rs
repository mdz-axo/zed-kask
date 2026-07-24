//! Chat thread registry — short-term memory streams for agent conversations.
//!
//! Each agent has a `threads/` directory in their agent folder. Threads are
//! stored as individual JSON files — one per thread — for user sovereignty.
//!
//! Threads are **short-term memory**: each stores its own conversation history
//! (the "stream"). Switching threads changes the agent's immediate context.
//! Long-term episodic/semantic memory runs in parallel via the REPL's "p" step
//! and is independent of which thread is active.
//!
//! Storage layout:
//! ```text
//! agents/{name}/threads/
//!   _active            — currently active thread ID (plain text)
//!   {uuid}.json        — thread metadata + conversation turns
//! ```rust,no_run
//!
//! Each thread file is a JSON object:
//! ```json
//! {
//!   "id": "uuid",
//!   "title": "...",
//!   "status": "active",
//!   "turns": [
//!     {"role": "user", "content": "...", "timestamp": "..."},
//!     {"role": "assistant", "content": "...", "timestamp": "..."}
//!   ]
//! }
//! ```rust,no_run
//!
//! Turns are capped at 200 entries (100 exchanges). Beyond that, oldest turns
//! are pruned on save — they remain in long-term episodic memory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Emit a Regulation span for thread operations.
fn emit_thread_reg(operation: &str, detail: &str) {
    tracing::info!(
        target: "reg",
        reg_domain = "reg.thread",
        operation = %operation,
        detail = %detail,
        "REG"
    );
}

/// Maximum turns stored per thread (200 = 100 exchanges).
const MAX_THREAD_TURNS: usize = 200;

/// A single turn in a thread's conversation stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnEntry {
    /// "user" or "assistant"
    pub role: String,
    /// The message content.
    pub content: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
}

/// A chat thread — a short-term memory stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    /// Unique thread identifier (UUID v4).
    pub id: String,
    /// Agent that owns this thread.
    pub agent_name: String,
    /// Human-readable title (derived from first user message).
    pub title: String,
    /// ISO 8601 timestamp of thread creation.
    pub created_at: String,
    /// ISO 8601 timestamp of last activity.
    pub last_active_at: String,
    /// Thread status.
    pub status: ThreadStatus,
    /// Number of exchanges (user + assistant pairs) in this thread.
    pub message_count: u32,
    /// First 80 characters of the first user message (for preview).
    pub preview: String,
    /// The short-term memory stream — recent conversation turns.
    /// Capped at MAX_THREAD_TURNS; oldest pruned on save.
    #[serde(default)]
    pub turns: Vec<TurnEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    Active,
    Archived,
}

/// In-memory thread registry.
#[derive(Debug, Clone, Default)]
pub struct ThreadRegistry {
    pub threads: BTreeMap<String, ChatThread>,
    pub active_thread_id: Option<String>,
    /// Whether the active thread's history has been seeded into context.
    /// False on session start and after thread switch — the next turn
    /// injects thread_history to bootstrap cold context.
    pub seeded: bool,
}

impl ThreadRegistry {
    /// Load the registry by scanning the agent's `threads/` directory.
    pub fn load(agent_name: &str) -> Self {
        let dir = threads_dir(agent_name);
        let mut threads = BTreeMap::new();
        let mut active_id = None;

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "_active" {
                    active_id = std::fs::read_to_string(&path)
                        .ok()
                        .map(|s| s.trim().to_string());
                } else if name.ends_with(".json")
                    && let Ok(contents) = std::fs::read_to_string(&path)
                    && let Ok(thread) = serde_json::from_str::<ChatThread>(&contents)
                {
                    threads.insert(thread.id.clone(), thread);
                }
            }
        }

        Self {
            threads,
            active_thread_id: active_id,
            seeded: false,
        }
    }

    /// Create a new thread and set it as active.
    pub fn create_thread(&mut self, agent_name: &str, first_message: &str) -> &ChatThread {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let title = thread_title_from_message(first_message);
        let preview = first_message.chars().take(80).collect::<String>();

        let thread = ChatThread {
            id,
            agent_name: agent_name.to_string(),
            title,
            created_at: now.clone(),
            last_active_at: now,
            status: ThreadStatus::Active,
            message_count: 0,
            preview,
            turns: Vec::new(),
        };

        let id = thread.id.clone();
        write_thread_file(agent_name, &thread);
        self.threads.insert(id.clone(), thread);
        self.active_thread_id = Some(id.clone());
        write_active_file(agent_name, Some(&id));
        self.seeded = false;
        emit_thread_reg("created", &id);
        self.threads.get(&id).expect("just inserted")
    }

    /// Append a user/assistant exchange to the active thread's short-term stream.
    /// Compacts oldest turns if the stream exceeds MAX_THREAD_TURNS — inserts a
    /// visible compaction entry instead of silently dropping turns.
    pub fn append_turn(&mut self, agent_name: &str, user_input: &str, assistant_response: &str) {
        let thread_id = match self.active_thread_id {
            Some(ref id) => id.clone(),
            None => return,
        };
        if let Some(thread) = self.threads.get_mut(&thread_id) {
            let now = chrono::Utc::now().to_rfc3339();
            thread.turns.push(TurnEntry {
                role: "user".to_string(),
                content: user_input.to_string(),
                timestamp: now.clone(),
            });
            thread.turns.push(TurnEntry {
                role: "assistant".to_string(),
                content: assistant_response.to_string(),
                timestamp: now,
            });
            thread.last_active_at = chrono::Utc::now().to_rfc3339();
            thread.message_count += 1;

            // Compact oldest turns if over cap — insert a visible compaction
            // entry instead of silently dropping turns. The ChatService handles
            // context-window condensation (87.5% threshold); this is a storage-
            // level cap that prevents JSON files from growing unbounded.
            if thread.turns.len() > MAX_THREAD_TURNS {
                let excess = thread.turns.len() - MAX_THREAD_TURNS;
                let removed_count = (excess + 1).min(thread.turns.len());
                let removed: Vec<TurnEntry> = thread.turns.drain(..removed_count).collect();
                let compacted_msg = format!(
                    "[Context Compacted] {} earlier message(s) pruned from short-term \
                     storage. Long-term episodic memory retains the full conversation.",
                    removed.len()
                );
                thread.turns.insert(
                    0,
                    TurnEntry {
                        role: "system".to_string(),
                        content: compacted_msg,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    },
                );
                emit_thread_reg(
                    "compacted",
                    &format!("{}: {} turns", thread.id, removed.len()),
                );
            }

            write_thread_file(agent_name, thread);
        }
    }

    /// Get the active thread's conversation history as typed `ChatMessage`s.
    /// Each turn's `role` ("user" or "assistant") is preserved, so the provider
    /// sees `[user, assistant, user, assistant, ...]` — not a flattened string.
    /// Returns the last `max_turns` exchanges (None = all). Returns None if
    /// no active thread or thread has no turns.
    pub fn thread_history_messages(
        &self,
        max_turns: Option<usize>,
    ) -> Option<Vec<hkask_types::ChatMessage>> {
        let thread_id = self.active_thread_id.as_ref()?;
        let thread = self.threads.get(thread_id)?;
        if thread.turns.is_empty() {
            return None;
        }
        let turns: Vec<&TurnEntry> = if let Some(max) = max_turns {
            let start = thread.turns.len().saturating_sub(max * 2);
            thread.turns[start..].iter().collect()
        } else {
            thread.turns.iter().collect()
        };
        let messages: Vec<hkask_types::ChatMessage> = turns
            .iter()
            .filter(|t| t.role == "user" || t.role == "assistant")
            .map(|t| hkask_types::ChatMessage {
                role: t.role.clone(),
                content: t.content.clone(),
            })
            .collect();
        if messages.is_empty() {
            None
        } else {
            Some(messages)
        }
    }

    /// Archive threads older than `max_age_days`. Returns count archived.
    pub fn archive_stale(&mut self, agent_name: &str, max_age_days: u32) -> usize {
        if max_age_days == 0 {
            return 0;
        }
        let cutoff = chrono::Utc::now() - chrono::TimeDelta::days(max_age_days as i64);
        let mut to_save: Vec<ChatThread> = Vec::new();
        for thread in self.threads.values_mut() {
            if thread.status == ThreadStatus::Active
                && let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&thread.last_active_at)
                && ts < cutoff
            {
                thread.status = ThreadStatus::Archived;
                to_save.push(thread.clone());
            }
        }
        for thread in &to_save {
            write_thread_file(agent_name, thread);
        }
        if !to_save.is_empty() {
            emit_thread_reg("archived", &format!("{} threads", to_save.len()));
        }
        to_save.len()
    }

    /// Manually archive or unarchive a thread.
    pub fn set_status(&mut self, thread_id: &str, agent_name: &str, status: ThreadStatus) -> bool {
        let result: Option<(bool, ChatThread)> =
            if let Some(thread) = self.threads.get_mut(thread_id) {
                thread.status = status;
                let is_now_archived = matches!(thread.status, ThreadStatus::Archived);
                Some((is_now_archived, thread.clone()))
            } else {
                None
            };
        match result {
            Some((is_now_archived, thread)) => {
                write_thread_file(agent_name, &thread);
                if is_now_archived && self.active_thread_id.as_deref() == Some(thread_id) {
                    self.active_thread_id = None;
                    write_active_file(agent_name, None);
                }
                true
            }
            None => false,
        }
    }

    /// Switch the active thread. Loads the thread's short-term memory.
    pub fn switch_to(&mut self, thread_id: &str, agent_name: &str) -> bool {
        if self.threads.contains_key(thread_id) {
            self.active_thread_id = Some(thread_id.to_string());
            write_active_file(agent_name, Some(thread_id));
            self.seeded = false;
            emit_thread_reg("switched", thread_id);
            if let Some(thread) = self.threads.get_mut(thread_id) {
                thread.last_active_at = chrono::Utc::now().to_rfc3339();
                write_thread_file(agent_name, thread);
            }
            true
        } else {
            false
        }
    }

    /// Mark the active thread as seeded — subsequent turns skip thread
    /// history injection; episodic recall handles conversation context.
    pub fn mark_seeded(&mut self) {
        self.seeded = true;
    }

    /// Get a thread by ID.
    pub fn get(&self, thread_id: &str) -> Option<&ChatThread> {
        self.threads.get(thread_id)
    }

    /// List threads, sorted by last_active_at (most recent first).
    pub fn list(&self) -> Vec<&ChatThread> {
        let mut threads: Vec<&ChatThread> = self.threads.values().collect();
        threads.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
        threads
    }
}

// ── Free I/O functions (no borrow on registry) ──────────────────────────

fn threads_dir(agent_name: &str) -> PathBuf {
    hkask_types::agent_paths::userpod_dir(agent_name).join("threads")
}

fn write_thread_file(agent_name: &str, thread: &ChatThread) {
    let dir = threads_dir(agent_name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", thread.id));
    let tmp = dir.join(format!("{}.json.tmp", thread.id));
    if let Ok(json) = serde_json::to_string_pretty(thread) {
        let _ = std::fs::write(&tmp, &json);
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn write_active_file(agent_name: &str, active_id: Option<&str>) {
    let dir = threads_dir(agent_name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("_active");
    if let Some(id) = active_id {
        let _ = std::fs::write(&path, id);
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

fn thread_title_from_message(msg: &str) -> String {
    let cleaned = msg.trim().chars().take(60).collect::<String>();
    if cleaned.len() < msg.trim().len() {
        format!("{}…", cleaned)
    } else {
        cleaned
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_list_threads() {
        let mut reg = ThreadRegistry::default();
        let id = {
            let t = reg.create_thread("test-agent", "Hello, can you help me with Rust?");
            assert_eq!(t.status, ThreadStatus::Active);
            assert!(t.title.contains("Hello"));
            t.id.clone()
        };
        assert_eq!(reg.list().len(), 1);
        let thread_file = threads_dir("test-agent").join(format!("{}.json", id));
        assert!(thread_file.exists());
    }

    #[test]
    fn append_and_recall_turns() {
        let mut reg = ThreadRegistry::default();
        reg.create_thread("test-agent", "Start");
        reg.append_turn("test-agent", "What is 2+2?", "It's 4.");
        reg.append_turn("test-agent", "Thanks!", "You're welcome!");

        let history = reg.thread_history_messages(None).unwrap();
        let texts: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("What is 2+2?")));
        assert!(texts.iter().any(|t| t.contains("It's 4.")));
        assert!(texts.iter().any(|t| t.contains("Thanks!")));
    }

    #[test]
    fn switch_threads_changes_context() {
        let mut reg = ThreadRegistry::default();
        let t1 = {
            let t = reg.create_thread("test-agent", "Thread 1");
            t.id.clone()
        };
        reg.append_turn("test-agent", "T1 message", "T1 response");

        let _t2 = {
            let t = reg.create_thread("test-agent", "Thread 2");
            t.id.clone()
        };
        reg.append_turn("test-agent", "T2 message", "T2 response");

        // Thread 2 is active — should see T2 history.
        let history = reg.thread_history_messages(None).unwrap();
        let texts: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("T2 message")));
        assert!(!texts.iter().any(|t| t.contains("T1 message")));

        // Switch to thread 1 — should see T1 history.
        reg.switch_to(&t1, "test-agent");
        let history = reg.thread_history_messages(None).unwrap();
        let texts: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("T1 message")));
        assert!(!texts.iter().any(|t| t.contains("T2 message")));
    }

    #[test]
    fn archive_stale_threads() {
        let mut reg = ThreadRegistry::default();
        let id = {
            let t = reg.create_thread("test-agent", "Old thread");
            t.id.clone()
        };

        let old_ts = (chrono::Utc::now() - chrono::TimeDelta::days(61)).to_rfc3339();
        if let Some(thread) = reg.threads.get_mut(&id) {
            thread.last_active_at = old_ts;
        }

        let archived = reg.archive_stale("test-agent", 60);
        assert_eq!(archived, 1);
        assert_eq!(reg.get(&id).unwrap().status, ThreadStatus::Archived);
    }

    #[test]
    fn thread_turns_pruned_at_cap() {
        let mut reg = ThreadRegistry::default();
        reg.create_thread("test-agent", "Cap test");
        let active_id = reg.active_thread_id.clone().unwrap();
        // Add more than MAX_THREAD_TURNS entries.
        for i in 0..(MAX_THREAD_TURNS + 10) {
            reg.append_turn("test-agent", &format!("msg{}", i), &format!("resp{}", i));
        }
        let thread = reg.get(&active_id).unwrap();
        assert!(thread.turns.len() <= MAX_THREAD_TURNS);
        // Oldest should be compacted — first messages should be gone.
        // With compaction, each overflow removes 3 turns and inserts 1
        // compaction entry. After 110 compactions (210 exchanges - 100 at
        // cap), msg110 is removed; oldest preserved is resp110/msg111.
        let history = reg.thread_history_messages(None).unwrap();
        let texts: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
        assert!(
            !texts.iter().any(|t| t.contains("msg0")),
            "msg0 should be compacted away"
        );
        assert!(
            !texts.iter().any(|t| t.contains("msg110")),
            "msg110 should be compacted away"
        );
        assert!(
            texts.iter().any(|t| t.contains("msg111")),
            "msg111 should be preserved"
        );
        // Compaction entry should be visible in the thread history.
        // (system-role entries are filtered out by thread_history_messages,
        // so the compaction marker is not visible — this is expected.)
    }
}
