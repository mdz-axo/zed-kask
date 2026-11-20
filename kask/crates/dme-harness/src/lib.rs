#![forbid(unsafe_code)]
//! The dme-harness library — the harness's modules, shared between the
//! CLI bin and the instrument's tests.
//!
//! The lib target exists so the order-oracle test (tests/mt_order.rs)
//! pins the PRODUCTION problem order, not a mirrored copy of the
//! algorithm — a mirror can only ever verify itself (a fake oracle;
//! found at consolidation P4 when the mirrored init_by_array never
//! terminated). Tests import `dme_harness::*`; the bin is a thin CLI
//! over the same modules.
//!
//! The harness links no inference engine. The probe leg talks to a
//! llama.cpp server the operator runs (`serving`); the converter, the
//! contrast leg and the edit leg are pure math over files. Activation
//! captures — the input of `contrast` and `convert` — are produced
//! outside this crate.

pub mod config;
pub mod contrast;
pub mod convert;
pub mod edit;
pub mod gguf;
pub mod gguf_writer;
pub mod mt;
pub mod probe;
pub mod problems;
pub mod receipt;
pub mod serving;

#[cfg(test)]
mod test_util;
