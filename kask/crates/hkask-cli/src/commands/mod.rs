//! CLI commands implementation
//!
//! Admin, config, startup, shutdown, and the single `tui` runtime launch.
//! Runtime operations (skills, bundles, templates, kata, kanban, goals,
//! adapters, Regulation queries, curator escalations, consolidation, style,
//! web search) live in the TUI's REPL slash commands or are invoked via
//! MCP tools from within the runtime.

pub mod backup_cmd;
pub mod chat; // chat_with_agent_streaming used by tui.rs for non-interactive mode
pub mod daemon;
pub mod deploy;
pub mod doctor;
pub mod export_cmd;
pub mod git_cmd;
pub mod helpers;
pub mod init;
pub mod keystore;
pub mod matrix;
pub mod mcp;
pub mod pod;
pub mod repair;
pub mod serve;
pub mod settings;
pub mod skill;
pub mod sovereignty;
pub mod token;
pub mod tui;
pub mod user;
pub mod wallet;

// Re-exports from chat (used by tui.rs non-interactive mode)
pub use chat::{ChatTurnResponse, TokenUsage, chat_with_agent_streaming};
