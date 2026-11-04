//! `hkask-private-inference` — the private-inference core shared service.
//!
//! PI ruling (2026-11-04, recorded as the dated amendment on the D-3 plan):
//! private inference is a core kask shared service, NOT an MCP server — the
//! interface is local-only by intent, and a core service carries no
//! agent-callable tool surface at all: the P4 boundary lives in the
//! architecture (settings + the M6 routing hook + a future panel), not in
//! approval tokens. Consumed by the zed-side seams.
//!
//! Committed plan (D-3, 2026-10-30) substance unchanged: a control plane,
//! never a token path — M3 rules the private endpoint is a *configured
//! instance* of the existing `openai_compatible` provider class, so this
//! crate owns placement decisions, lifecycles and verification, never
//! inference traffic.
//!
//! This slice implements the local leg's core (`local_leg`): the
//! drift-absorption stack of the local-leg plan
//! (`kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md`),
//! plus model `acquisition` from HuggingFace through the canonical
//! credential path.

pub mod acquisition;
pub mod local_leg;
