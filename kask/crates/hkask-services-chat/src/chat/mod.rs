//! Chat service — unified inference, memory integration, and prompt composition.
//!
//! Module structure:
//! - `types` — Request/response structs and token accounting
//! - `service` — `ChatService` struct and core orchestration methods
//! - `condenser` — Auto-condensation of conversation history
//! - `improv` — Improv mode system prompt generation

mod condenser;
pub mod improv;
pub mod service;
#[cfg(test)]
mod tests;
pub mod types;

pub use service::ChatService;
pub use types::{
    ChatStreamEvent, ChatTurnRequest, ChatTurnResponse, PreparedChat, TokenUsage, TurnRequest,
    TurnResult,
};
