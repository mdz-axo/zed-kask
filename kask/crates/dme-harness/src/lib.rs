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

pub mod config;
pub mod mt;
pub mod probe;
pub mod problems;
pub mod receipt;
pub mod serving;

// The capture path (the engine feature): in-process llama-cpp-4 for the
// tensor-transaction hooks, built from the pinned canonical source via
// the in-repo llama-cpp-sys-4 fork (LLAMA_CPP_SRC_DIR at build time).
#[cfg(feature = "engine")]
pub mod capture;
#[cfg(feature = "engine")]
pub mod embed;
#[cfg(feature = "engine")]
pub mod engine;

// The converter (embedding-to-edit bridge), the contrast-u instrument
// leg, and the edit leg need no in-process engine — only the shared GGUF
// component.
pub mod contrast;
pub mod convert;
pub mod edit;

// The shared GGUF component (P3): the private-inference stack's reader
// (local_leg::gguf) + writer (local_leg::gguf_writer) — the hand parsers
// are gone.
pub use hkask_private_inference::local_leg::gguf as pi_gguf;
pub use hkask_private_inference::local_leg::gguf_writer;
