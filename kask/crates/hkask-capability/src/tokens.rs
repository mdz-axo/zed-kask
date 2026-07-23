//! Capability boundary tokens — OCAP authority in the type system
//!
//! Per Mark Miller's OCAP discipline: authority flows downward through the
//! loop hierarchy. These tokens prove that the holder has authority from
//! the correct loop.
//!
//! Each token can only be constructed by the loop that governs it — private
//! fields prevent forgery. The module path IS the loop assignment.

/// Token proving that a consolidation (Episodic → Semantic) operation
/// was authorized. No longer required — consolidation is always permitted.
#[cfg(test)]
mod tests {
    // ConsolidationToken removed — consolidation no longer requires token authorization.
}
