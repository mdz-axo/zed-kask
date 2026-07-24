#![cfg_attr(not(test), forbid(unsafe_code))]
//! hkask-git-cas — gix-backed Git content-addressed storage adapter.
//!
//! This is the backup/gitcas component: it implements
//! [`hkask_types::git_cas::GitCASPort`] and pod-directory backup for
//! registries and artifacts (files, YAML/templates, databases, logs).
//!
//! It is the **only** crate in the workspace that depends on `gix`. Thin MCP
//! servers depend on `hkask-mcp` (gix-free) and pay no git-engine compile cost;
//! only components that actually instantiate backup/admin operations depend
//! on this crate.

pub mod gix_adapter;
pub use gix_adapter::GixCasAdapter;
