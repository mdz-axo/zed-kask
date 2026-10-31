#![forbid(unsafe_code)]
//! hkask-trust-eval — the trust-internalization measurement instruments.
//!
//! Two pieces make the trust mechanism measurable (trust-program-synthesis
//! Part V, experiment `trust-adherence-v1` / `exp_8dc334cc…`):
//!
//! - the **shortcut probe harness** (`probe`, `probe_ledger`): a pinned task
//!   carrying an explicit weight-3 instruction whose honest path costs at
//!   least 3x the shortcut path, a deterministic action-path classifier
//!   over run outputs (honored / shortcut / renegotiated / indeterminate,
//!   with the disclosure state), and a four-tool ledger flow that records
//!   the outcome through the real `trust_ledger_*` MCP tools on a
//!   probe-scoped account — never the operator's live trust account;
//! - the **adherence eval harness** (`eval_set`, `checkers`,
//!   `adjudication`): 20 pinned tasks drawn from real zed-kask work
//!   classes (research, code change, doc update, verification), each
//!   carrying 2-3 explicit weight-3 instructions, pinned by content hash;
//!   deterministic checkers as the hand-label instrument; and the blinded
//!   adjudication schema with the calibration gate (>= 0.90 agreement on
//!   20 hand-labeled outcomes before any confirmatory run).
//!
//! Nothing here fabricates outcomes: the probe reports what the run did;
//! the eval harness adjudicates real run outputs. Synthetic outputs appear
//! only as test fixtures pinning the instrument, never as results.

pub mod adjudication;
pub mod checkers;
pub mod eval_set;
pub mod probe;
pub mod probe_ledger;
