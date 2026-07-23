//! Curator Agent metacognition: sense→compare→compute→act governance loop.
//! Moved from `curator::metacognition` — persona concern, not regulatory.

mod config;
mod escalation;
mod format;
mod hloop_impl;
mod loop_body;
mod persistence;

pub use config::{HealthSnapshot, MetacognitionConfig};
pub use escalation::{EscalationAlert, EscalationPolicy, EscalationTrigger};
pub use loop_body::MetacognitionLoop;
