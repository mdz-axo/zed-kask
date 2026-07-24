#![forbid(unsafe_code)]
//! hkask-mcp-cloud-gateway — mTLS + DelegationToken transport adapter.
//!
//! Provides remote access to the hKask daemon for MCP servers running
//! outside the local machine (cloud deployments, IDE integrations).
//!
//! # Security Model (Three Layers)
//!
//! 1. **Transport:** mTLS 1.3 — client and server present X.509 certificates.
//!    The client cert's Common Name maps to the userpod WebID.
//! 2. **Authorization:** Ed25519-signed `DelegationToken` per request.
//!    Token `delegated_to` must match the mTLS CN.
//! 3. **Capability:** Per-tool gating — `token.resource_id` must match
//!    the requested tool name.
//!
//! # Architecture
//!
//! ```text
//! Remote MCP Client ──[mTLS]──▶ Gateway ──[Unix socket]──▶ DaemonHandler
//!                                    │
//!                                    ├── Verify client cert CN
//!                                    ├── Verify DelegationToken signature
//!                                    ├── Verify resource_id matches tool
//!                                    └── Forward to DaemonHandler::dispatch_tool
//! ```

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod auth;
pub mod server;
