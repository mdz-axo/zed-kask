//! REPL sub-handler modules — one file per slash command domain

pub mod adapter;
pub mod agent;
pub mod ask;
pub mod bundle;
pub mod consolidation;
pub mod escalation;
pub mod feedback;
pub mod fusion;
pub mod goal;
pub mod improv;
pub mod info;
pub mod kata;
pub mod listen;
#[cfg(feature = "communication")]
pub mod matrix;
pub mod mcp;
pub mod model;
pub mod pod;
pub mod repl_settings;
pub mod skill;
pub mod start;
pub mod status;

pub mod kanban;
pub mod talk;
pub mod thread;

pub use adapter::handle_adapter;
pub use agent::{handle_agent, handle_agents};
pub use ask::handle_ask;
pub use bundle::handle_bundle;
pub use consolidation::handle_consolidate;
pub use escalation::{handle_dismiss, handle_escalations, handle_resolve};
pub use feedback::handle_feedback;
pub use fusion::handle_fusion;
pub use goal::handle_goal;
pub use improv::handle_improv;
pub use info::{handle_history, handle_pods, handle_templates, handle_tools};
pub use kanban::handle_kanban;
pub use kata::handle_kata;
pub use listen::handle_listen;
#[cfg(feature = "communication")]
pub use matrix::{handle_matrix, handle_msg};
pub use mcp::handle_mcp;
pub use model::handle_model;
pub use pod::handle_pod;
pub use repl_settings::{ReplSettings, handle_repl_set, to_llm_params};
pub use skill::handle_skill;
pub use start::handle_start;
pub use status::handle_status;
pub use talk::{handle_talk, speak_response};
pub use thread::handle_thread;
