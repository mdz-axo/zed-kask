#![forbid(unsafe_code)]
//! hkask-acp — ACP (Agent Client Protocol) userpod library.
//!
//! Re-exports the public API for integration testing and external consumers.

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod cloud;
pub mod main_impl;

pub use main_impl::AcpError;
pub use main_impl::HkaskAcpAgent;
pub use main_impl::SessionState;
