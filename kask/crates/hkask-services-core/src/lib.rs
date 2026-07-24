#![forbid(unsafe_code)]
//! hKask Service-Layer Foundation — shared error types, configuration, and settings.
//!
//! This crate is the foundation for most service-layer modules: `ServiceError`,
//! `ServiceConfig`, and `HkaskSettings` are consumed by every service crate
//! except the research module (now in hkask-mcp-research), which intentionally keeps its own
//! provider-shaped `WebError` (see ADR-054). Extracted from `hkask-services` to
//! enable parallel compilation and clear architectural boundaries.
//!
//! # Modules
//!
//! - `error` — `ServiceError` enum composing all domain error types
//! - `config` — `ServiceConfig` resolved once at startup
//! - `settings` — `HkaskSettings` and canonical settings path

pub mod config;
pub mod error;
pub mod settings;

pub use config::{DEFAULT_DB_PATH, ServiceConfig};
pub use error::{DomainKind, ErrorKind, ServiceError};
pub use settings::{HkaskSettings, load_settings, save_settings, settings_path};
