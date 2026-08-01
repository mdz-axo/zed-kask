#![forbid(unsafe_code)]
//! hKask Capability — in-process capability tokens for inter-agent delegation.
//!
//! A `DelegationToken` declares "holder X may perform action Y on resource Z".
//! Tokens are minted and consumed in-process; the enforced gate is the
//! capability match in `McpRuntime::invoke`, not cryptography.

pub mod auth;
pub mod resources;
pub mod token_types;
pub mod tool_port;
pub mod verification;

pub use auth::panel_default_token;
pub use resources::{
    CapabilityParseError, CapabilitySpec, DelegationAction, DelegationResource, capabilities_match,
    capability_from_server_id,
};
pub use token_types::{
    CapabilityError, DelegationToken, DelegationTokenBuilder, SYSTEM_MAX_ATTENUATION,
    SYSTEM_MAX_RECURSION, TokenRegistry, TokenRegistryError,
};
pub use tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
pub use verification::{
    TOKEN_ERR_EXPIRED, TOKEN_ERR_INVALID_SIGNATURE, TOKEN_ERR_NO_CHECKER, VerificationOutcome,
    token_err_insufficient_access, token_err_tool_access_denied,
};
