#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Service-Layer Foundation — shared error types and model settings.
//!
//! Consumed by the corpus and curator MCP servers: `ServiceError` (with
//! `DomainKind`/`ErrorKind`) and `HkaskSettings` (env > settings.json >
//! default model resolution). The former `ServiceConfig` was removed —
//! zero production callers (the deletion test); storage drivers are opened
//! directly by each server via `hkask-storage`.
//!
//! # Modules
//!
//! - `error` — `ServiceError` enum composing all domain error types
//! - `settings` — `HkaskSettings` and the canonical settings path

pub mod error;
pub mod settings;

pub use error::{DomainKind, ErrorKind, ServiceError};
pub use settings::HkaskSettings;
