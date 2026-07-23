//! Curator Agent — Persona layer for the Curator (Loop 5)
//!
//! The Curator Agent is the persona/agent half of the Curation separation.
//! It holds metacognition, bot metrics, spec curation, and human-facing
//! reporting — everything that is NOT pure regulatory loop behavior.
//!
//! The Curation Loop (`curation::CurationLoop`) is the pure regulatory half:
//! sense/compute/act with no persona, no chat, no memory. The Curator Agent
//! *uses* the Curation Loop through Communication dispatch and receives
//! `CuratorDirective`s that it formats for human consumption.
//!
//! # Architecture
//!
//! ```text
//! CuratorAgent
//! ├── curation_loop: `Arc<CurationLoop>`   // pure regulatory
//! ├── metacognition: `Arc<MetacognitionLoop>` // persona: observe & adapt
//! └── context: `Arc<CuratorContext>`       // capability-disciplined access
//! ```

pub mod cat;
pub mod metacognition;

use crate::curation::context::CuratorContext;
use crate::curation::curation_loop::CurationLoop;
use crate::pod::CommunicationPosture;
use hkask_memory::ConsolidationBridge;
use hkask_regulation::types::loops::CurationInput;
use std::sync::Arc;

/// Curator Agent — the persona layer of Curation (Loop 5).
///
/// Composes the pure regulatory `CurationLoop` with the persona/agent
/// `MetacognitionLoop`. The agent receives `CuratorDirective`s from the
/// Curation Loop through Communication dispatch and formats human-readable
/// output for `kask chat`.
///
/// **Construction:** Use `CuratorAgent::new()` with a `CuratorContext`,
/// `MetacognitionConfig`, and optional consolidation port. The agent
/// internally creates both the `MetacognitionLoop` and `CurationLoop`.
///
/// **Singleton invariant:** There is exactly one `CuratorAgent` per hKask
/// system, just as there is exactly one `CurationLoop`.
pub struct CuratorAgent {
    curation_loop: Arc<CurationLoop>,
    metacognition: Arc<metacognition::MetacognitionLoop>,
    context: Arc<CuratorContext>,
}

impl CuratorAgent {
    /// Create a new Curator Agent with default configuration.
    ///
    /// The agent internally creates both the `MetacognitionLoop` and
    /// `CurationLoop`, connecting them through the shared `CuratorContext`.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — CuratorAgent composes Curation + Metacognition
    /// pre:  `context` is a valid `Arc<CuratorContext>`.
    /// post: Returns a `CuratorAgent` with default `MetacognitionConfig`
    ///       and a new `CurationLoop`.
    pub fn new(context: Arc<CuratorContext>) -> Self {
        let agent_name = context.handle().curator_id().to_string();
        let metacognition = Arc::new(
            metacognition::MetacognitionLoop::new(
                Arc::clone(&context),
                metacognition::MetacognitionConfig::default(),
            )
            .with_agent_name(agent_name),
        );
        let curator_handle = context.handle().clone();
        let curation_loop = Arc::new(CurationLoop::new(curator_handle, Arc::clone(&context)));

        Self {
            curation_loop,
            metacognition,
            context,
        }
    }

    /// Create a Curator Agent with custom metacognition configuration.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — custom metacognition configuration
    /// \[P7\] Constraining: Evolutionary Architecture — thresholds emerge from real usage
    /// pre:  `context` is a valid `Arc<CuratorContext>`; `config` is a
    ///       valid `MetacognitionConfig`.
    /// post: Returns a `CuratorAgent` with the given config and a new
    ///       `CurationLoop`.
    pub fn with_config(
        context: Arc<CuratorContext>,
        config: metacognition::MetacognitionConfig,
    ) -> Self {
        let agent_name = context.handle().curator_id().to_string();
        let metacognition = Arc::new(
            metacognition::MetacognitionLoop::new(Arc::clone(&context), config)
                .with_agent_name(agent_name),
        );
        let curator_handle = context.handle().clone();
        let curation_loop = Arc::new(CurationLoop::new(curator_handle, Arc::clone(&context)));

        Self {
            curation_loop,
            metacognition,
            context,
        }
    }

    /// Create a Curator Agent with a consolidation port.
    ///
    /// When episodic budget pressure triggers escalation, the consolidation
    /// bridge will fire to migrate episodic h_mems into semantic memory.
    ///
    /// `inbox_rx` — CurationInput channel from Cybernetics.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — consolidation wired into CuratorAgent
    /// \[P2\] Constraining: Affirmative Consent — auto-consolidation is opt-in and consent-gated
    /// pre:  `context` is a valid `Arc<CuratorContext>`; `config` is a
    ///       valid `MetacognitionConfig`; `consolidation` is a valid
    ///       `Arc<ConsolidationBridge>`; `auto_consolidation_enabled` controls
    ///       whether the Curator daemon may auto-run consolidation.
    /// post: Returns a `CuratorAgent` with consolidation and its curation inbox wired.
    pub fn with_consolidation(
        context: Arc<CuratorContext>,
        config: metacognition::MetacognitionConfig,
        consolidation: Arc<ConsolidationBridge>,
        inbox_rx: tokio::sync::mpsc::UnboundedReceiver<CurationInput>,
        auto_consolidation_enabled: bool,
    ) -> Self {
        let agent_name = context.handle().curator_id().to_string();
        let metacognition = Arc::new(
            metacognition::MetacognitionLoop::new(Arc::clone(&context), config)
                .with_agent_name(agent_name),
        );
        let curator_handle = context.handle().clone();
        let curation_loop = Arc::new(
            CurationLoop::with_consolidation(curator_handle, Arc::clone(&context), consolidation)
                .with_auto_consolidation_enabled(auto_consolidation_enabled)
                .with_inbox(inbox_rx),
        );

        Self {
            curation_loop,
            metacognition,
            context,
        }
    }

    /// Access the Curation Loop (pure regulatory).
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — accessor for the pure regulatory loop
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `Arc<CurationLoop>`.
    pub fn curation_loop(&self) -> &Arc<CurationLoop> {
        &self.curation_loop
    }

    /// Access the Metacognition Loop (persona/agent).
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — accessor for the persona/agent loop
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `Arc<MetacognitionLoop>`.
    pub fn metacognition(&self) -> &Arc<metacognition::MetacognitionLoop> {
        &self.metacognition
    }

    /// Access the CuratorContext (capability-disciplined runtime references).
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — accessor for capability-disciplined context
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `Arc<CuratorContext>`.
    pub fn context(&self) -> &Arc<CuratorContext> {
        &self.context
    }

    /// Set communication posture for the metacognition loop (CAT framework).
    ///
    /// Creates a new `MetacognitionLoop` with the given persona name and
    /// convergence bias, using the existing config and context.
    pub fn with_communication_posture(mut self, posture: CommunicationPosture) -> Self {
        let agent_name = self.metacognition.agent_name().to_string();
        self.metacognition = Arc::new(metacognition::MetacognitionLoop::with_posture(
            Arc::clone(&self.context),
            self.metacognition.config().clone(),
            posture,
            agent_name,
        ));
        self
    }
}

// Re-export persona types for convenience
pub use metacognition::{
    EscalationAlert, EscalationPolicy, EscalationTrigger, HealthSnapshot, MetacognitionConfig,
    MetacognitionLoop,
};
