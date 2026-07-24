#![forbid(unsafe_code)]
//! hKask Skill Service — skill discovery, publishing, hashing, auditing, and bundle composition.
//!
//! Extracted from `hkask-services`.
mod skill_impl;
pub use skill_impl::{
    SkillInfo, SkillPublishResult, compute_file_hash, discover_skills, find_public_skill,
    publish_skill, read_skill_namespace, read_skill_visibility, resolve_userpod_name,
};

pub mod audit;
pub mod bundle;

pub use audit::{
    SkillAuditError, SkillAuditReport, SkillAuditor, SkillHealthScore, SkillStatus, TemplateSummary,
};
pub use bundle::{BundleComposeResult, BundleService};
