#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Capability — in-process capability tokens for inter-agent delegation.
//!
//! A `DelegationToken` declares "holder X may perform action Y on resource Z".
//! Tokens are minted and consumed in-process; the enforced gate is the
//! capability match in `McpRuntime::invoke`, not cryptography.

pub mod auth;
pub mod resources;
pub mod token_types;
pub mod tool_port;
pub mod tool_taint;

pub use auth::panel_default_token;
pub use resources::{
    CapabilityParseError, CapabilitySpec, DelegationAction, DelegationResource, capabilities_match,
    capability_from_server_id,
};
pub use token_types::{DelegationToken, SYSTEM_MAX_RECURSION};
pub use tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
