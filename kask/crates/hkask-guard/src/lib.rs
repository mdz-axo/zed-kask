#![forbid(unsafe_code)]
//! hKask Content Safety Guard — mandatory LLM boundary protection.
//!
//! **P3.1 Social Generativity:** Core content safety controls are mandatory
//! at every LLM boundary. This crate provides the single implementation used
//! by classification pipelines (chat, MCP, and memory pipelines pending integration).
//!
//! # Reference Standards
//!
//! This implementation is aligned with the following frameworks:
//!
//! ## Primary: OWASP Top 10 for LLM Applications
//!
//! <https://owasp.org/www-project-top-10-for-large-language-model-applications>
//!
//! | OWASP LLM Risk | Guard Scanner | Implementation |
//! |---|---|---|
//! | LLM01: Prompt Injection | `BanSubstrings` + `Deobfuscate` | Curated injection patterns with deobfuscation pre-pass (base64, leet, spacing, confusables) |
//! | LLM02: Insecure Output Handling | `Secrets` | Credential leak detection (API keys, JWTs, PEMs), stripped before shared memory storage |
//! | LLM04: Model Denial of Service | `TokenLimit` | 32K token budget gate before model invocation |
//! | LLM06: Sensitive Information Disclosure | `Secrets` (output) | Secrets in model output are redacted before entering any persistent store |
//!
//! Future scanners (available in `llm-guard`, not yet wired):
//! - LLM03: Training Data Poisoning — `ScriptMix` (Unicode look-alike detection)
//! - LLM08: Vector/Embedding Weaknesses — `InvisibleText` (zero-width/bidi character smuggling)
//!
//! ## Secondary References
//!
//! - **NIST AI Risk Management Framework** (AI RMF 1.0, 2023): Categorizes AI risks
//!   into technical, socio-technical, and guiding principles. Our guard implements
//!   the "technical" controls (MAP 1: Validity & Reliability, MAP 3: Security &
//!   Resiliency). <https://www.nist.gov/itl/ai-risk-management-framework>
//!
//! - **ENISA Multilayer Framework for Good Cybersecurity Practices for AI**
//!   (2024): EU agency framework. Our mandatory-by-design controls align with
//!   "security-by-design" requirement. <https://www.enisa.europa.eu>
//!
//! - **Martin et al. (2025) "Few-Shot Is the Dominant Strategy for Structured
//!   Extraction"** (arXiv:2603.29878): Established that few-shot prompting lifts
//!   structured extraction F1 from <30% to 99%+. Justifies our approach of
//!   pattern-based (not ML-based) guard scanning as the primary defense layer.
//!
//! - **Zaratiana et al. (2026) "GLiGuard: Schema-Conditioned Classification for
//!   LLM Safeguard"** (arXiv:2605.07982): Demonstrated that schema-conditioned
//!   classification models can detect prompt injection with high recall without
//!   large-model inference costs. The `llm-guard` rules-tier approach leverages
//!   similar zero-copy, no-ML principles.

mod guarded_inference;
mod pipeline;
mod spotlight;

pub use guarded_inference::GuardedInferencePort;
pub use pipeline::{
    CanaryToken, ContentGuard, GuardConfig, GuardOutput, GuardResult, GuardViolation,
};
pub use spotlight::{SpotlightMode, Spotlighter};
