//! Run configs — the archived, card-anchored sampling configurations
//! (direct-model-edit-archive/configs/config-{A,B,S}.json), loaded verbatim.
//!
//! DME-0's card values come from the vendor card record
//! (vendor-card/card-notes.md in the archive): config B is the vendor-card
//! config (non-thinking default, card temp 0.7, top_p 0.8, presence_penalty
//! 1.5); config A is the anchor-style trap (thinking, temp 0.6); config S is
//! the Ornith-1.5-9B sibling card general-task config.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct RunConfig {
    pub name: String,
    pub label: String,
    pub enable_thinking: bool,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    pub presence_penalty: f32,
    pub repetition_penalty: f32,
    #[serde(default)]
    pub provenance: Option<String>,
}

impl RunConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read config {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse config {}", path.display()))
    }
}
