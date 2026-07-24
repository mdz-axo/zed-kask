#![forbid(unsafe_code)]
//! hKask Communication — core Matrix transport, agent registry, and 7R7 listener.
//!
//! This is a core infrastructure crate, not an MCP server. It provides:
//! - `MatrixTransport` — matrix-sdk wrapper for Matrix protocol operations
//! - `AgentRegistry` — WebID→UserId mapping and thread watchlists
//! - `SevenR7Listener` — passive Matrix room observer, emits Regulation spans
//!
//! The daemon owns the Matrix connection. The REPL, pod activation hooks,
//! and MCP tool surface all use this crate through the daemon.
#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

#[cfg(feature = "matrix")]
pub mod agent_registration;
#[cfg(feature = "matrix")]
pub mod listener;
#[cfg(feature = "matrix")]
pub mod matrix;
