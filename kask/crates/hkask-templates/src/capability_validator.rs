//! Capability-aware validator — OCAP enforcement point for template execution.
//!
//! At template registration time, capability validation checks that the registering
//! agent holds the required OCAP tokens for the template's declared capability
//! requirements. At runtime, OCAP enforcement is handled by `GovernedTool` in
//! `hkask-regulation::governed_tool`. This validator covers the registration-time concern:
//! ensuring template capability declarations are consistent with the agent's
//! granted capabilities.

use crate::ports::{Result, TemplateError};
use hkask_capability::{CapabilitySpec, DelegationToken, capabilities_match};

/// Validates that an agent's capability tokens satisfy a template's requirements.
///
/// Each required capability is a string like `"tool:execute"` or `"memory:read"`.
/// The validator parses each requirement and checks it against the held tokens
/// using `capabilities_match` (which respects the action hierarchy: Execute ≥ Write ≥ Read).
///
/// This is a registration-time gate. Runtime enforcement is delegated to
/// `GovernedTool` in `hkask-regulation`.
pub struct CapabilityAwareValidator;

impl CapabilityAwareValidator {
    /// Create a new validator.
    ///
    /// expect: "The system validates template capability requirements against held tokens"
    /// \[P3\] Motivating: Generative Space — registration-time OCAP gate for template capabilities
    /// \[P4\] Constraining: Clear Boundaries — validator establishes capability boundary
    /// post: returns CapabilityAwareValidator
    pub fn new() -> Self {
        Self
    }

    /// Validate that the given tokens satisfy the template's capability requirements.
    ///
    /// Returns `Ok(())` if all required capabilities are satisfied by at least one
    /// held token. Returns `Err(TemplateError::CapabilityDenied)` with details about
    /// the first unsatisfied requirement.
    ///
    /// expect: "The system validates template capability requirements against held tokens"
    /// \[P3\] Motivating: Generative Space — checks template capability requirements against held tokens
    /// \[P4\] Constraining: Clear Boundaries — action hierarchy enforcement (Execute ≥ Write ≥ Read)
    /// pre:  template_id is non-empty
    /// post: returns Ok(()) if all required capabilities are satisfied
    /// post: returns Ok(()) if required_capabilities is empty
    /// post: returns Err(CapabilityDenied) for first unsatisfied requirement
    pub fn validate_capabilities(
        &self,
        template_id: &str,
        required_capabilities: &[String],
        held_tokens: &[DelegationToken],
    ) -> Result<()> {
        // No requirements → always valid
        if required_capabilities.is_empty() {
            return Ok(());
        }

        for required in required_capabilities {
            let required_spec = CapabilitySpec::parse(required).map_err(|e| {
                TemplateError::CapabilityDenied(format!(
                    "Template '{}' has malformed capability requirement '{}': {}",
                    template_id, required, e
                ))
            })?;

            let satisfied = held_tokens.iter().any(|token| {
                let token_capability = format!(
                    "{}:{}:{}",
                    token.resource.as_str(),
                    token.resource_id,
                    token.action.as_str()
                );
                capabilities_match(&token_capability, required)
            });

            if !satisfied {
                return Err(TemplateError::CapabilityDenied(format!(
                    "Template '{}' requires capability '{}' ({}:{}:{}) but no held token satisfies it",
                    template_id,
                    required,
                    required_spec.resource.as_str(),
                    required_spec.resource_id,
                    required_spec.action.as_str()
                )));
            }
        }

        Ok(())
    }
}

impl Default for CapabilityAwareValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::{DelegationAction, DelegationResource, derive_signing_key};
    use hkask_types::WebID;

    fn make_token(
        resource: DelegationResource,
        resource_id: &str,
        action: DelegationAction,
    ) -> DelegationToken {
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"holder");
        DelegationToken::new(
            resource,
            resource_id.into(),
            action,
            from,
            to,
            &derive_signing_key(b"test-secret-32-bytes-long!!"),
        )
    }

    // [P3] Motivating: Generative Space — validates empty capability requirement set
    #[test]
    fn empty_requirements_always_pass() {
        let validator = CapabilityAwareValidator::new();
        let result = validator.validate_capabilities("t1", &[], &[]);
        assert!(result.is_ok());
    }

    // [P3] Motivating: Generative Space — validates held token satisfies requirement
    #[test]
    fn satisfied_requirement_passes() {
        let validator = CapabilityAwareValidator::new();
        let token = make_token(
            DelegationResource::Tool,
            "search",
            DelegationAction::Execute,
        );
        let result =
            validator.validate_capabilities("t1", &["tool:search:execute".into()], &[token]);
        assert!(result.is_ok());
    }

    // [P3] Motivating: Generative Space — validates insufficient capability is rejected
    #[test]
    fn unsatisfied_requirement_fails() {
        let validator = CapabilityAwareValidator::new();
        let token = make_token(DelegationResource::Tool, "search", DelegationAction::Read);
        let result =
            validator.validate_capabilities("t1", &["tool:search:execute".into()], &[token]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("t1"));
        assert!(err.to_string().contains("tool:search:execute"));
    }

    // [P3] Motivating: Generative Space — validates action hierarchy
    //Constraining: Clear Boundaries — Execute token satisfies Read requirement
    #[test]
    fn execute_token_satisfies_read_requirement() {
        let validator = CapabilityAwareValidator::new();
        let token = make_token(
            DelegationResource::Tool,
            "search",
            DelegationAction::Execute,
        );
        let result = validator.validate_capabilities("t1", &["tool:search:read".into()], &[token]);
        assert!(result.is_ok());
    }

    // [P3] Motivating: Generative Space — validates action hierarchy
    //Constraining: Clear Boundaries — Write token satisfies Read requirement
    #[test]
    fn write_token_satisfies_read_requirement() {
        let validator = CapabilityAwareValidator::new();
        let token = make_token(DelegationResource::Tool, "search", DelegationAction::Write);
        let result = validator.validate_capabilities("t1", &["tool:search:read".into()], &[token]);
        assert!(result.is_ok());
    }

    // [P3] Motivating: Generative Space — validates action hierarchy
    //Constraining: Clear Boundaries — Read token does not satisfy Write requirement
    #[test]
    fn read_token_does_not_satisfy_write_requirement() {
        let validator = CapabilityAwareValidator::new();
        let token = make_token(DelegationResource::Tool, "search", DelegationAction::Read);
        let result = validator.validate_capabilities("t1", &["tool:search:write".into()], &[token]);
        assert!(result.is_err());
    }

    // [P3] Motivating: Generative Space — validates malformed capability syntax is rejected
    #[test]
    fn malformed_requirement_returns_error() {
        let validator = CapabilityAwareValidator::new();
        let result = validator.validate_capabilities("t1", &["not-a-valid-capability".into()], &[]);
        assert!(result.is_err());
    }

    // [P3] Motivating: Generative Space — validates all required capabilities must be held
    #[test]
    fn multiple_requirements_all_must_be_satisfied() {
        let validator = CapabilityAwareValidator::new();
        let token1 = make_token(
            DelegationResource::Tool,
            "search",
            DelegationAction::Execute,
        );
        let token2 = make_token(
            DelegationResource::Registry,
            "templates",
            DelegationAction::Read,
        );
        let result = validator.validate_capabilities(
            "t1",
            &[
                "tool:search:execute".into(),
                "registry:templates:read".into(),
            ],
            &[token1, token2],
        );
        assert!(result.is_ok());
    }

    // [P3] Motivating: Generative Space — validates missing tokens cause rejection
    #[test]
    fn no_held_tokens_with_requirements_fails() {
        let validator = CapabilityAwareValidator::new();
        let result = validator.validate_capabilities("t1", &["tool:search:execute".into()], &[]);
        assert!(result.is_err());
    }
}
