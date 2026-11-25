#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Shared IPC protocol and domain wire constants/types for hKask MCP servers,
//! GPUI panels, and `kask_bridge`.
//!
//! Extracted from `hkask-types` (Slice S7 Shape A+) so edits to domain wire
//! constants or IPC payloads no longer invalidate the 20+ core crates
//! (`hkask-storage`, `hkask-memory`, `hkask-regulation`, `agent`, etc.) that
//! depend on `hkask-types` without touching IPC/wire shapes.

pub mod inference_ipc;
pub mod kanban_wire;
pub mod media_wire;
pub mod nostr_wire;
pub mod wallet_wire;
