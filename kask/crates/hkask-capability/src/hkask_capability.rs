#![forbid(unsafe_code)]
//! hKask Capability — OCAP delegation token system
//!
//! Ed25519-signed delegation tokens with cryptographic attenuation.
//! Two token kinds: **Loop authority** (ZST tokens in `tokens.rs`) prove loop-authorized operations;
//! **Delegation** (`DelegationToken`) are Ed25519-signed tokens for inter-agent delegation.

pub mod auth;
pub mod resources;
pub mod token_types;
pub mod tokens;
pub mod tool_port;
pub mod verification;

pub use auth::{AuthContext, derive_signing_key};
pub use resources::{
    CapabilityParseError, CapabilitySpec, DelegationAction, DelegationResource, capabilities_match,
    capability_from_server_id,
};
pub use token_types::{
    CapabilityError, DelegationToken, DelegationTokenBuilder, NoOpTokenRegistry,
    SYSTEM_MAX_ATTENUATION, SYSTEM_MAX_RECURSION, TokenRegistry, TokenRegistryError,
};
pub use tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
pub use verification::{
    CapabilityChecker, TOKEN_ERR_EXPIRED, TOKEN_ERR_INVALID_SIGNATURE, TOKEN_ERR_NO_CHECKER,
    VerificationOutcome, require_read_access, require_write_access, token_err_insufficient_access,
    token_err_tool_access_denied, verify_delegation_token, verify_delegation_token_now,
};
