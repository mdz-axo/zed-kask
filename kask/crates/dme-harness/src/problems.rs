//! GSM8K train-split practice problems and the deterministic run order.
//!
//! Source: openai/grade-school-math, grade_school_math/data/train.jsonl
//! (7,473 rows; fetched by plain curl — the canonical upstream JSONL).
//! The DME practice set uses the TRAIN split only — no test-set access.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Problem {
    pub question: String,
    pub answer: String,
}

impl Problem {
    /// GSM8K gold: the integer after the "#### " marker.
    pub fn gold(&self) -> String {
        self.answer
            .rsplit("#### ")
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

pub struct ProblemSet {
    pub problems: Vec<Problem>,
    /// deterministic run order: indices into `problems`
    pub order: Vec<usize>,
}

impl ProblemSet {
    /// Load the train JSONL and compute the run order — the ONE order:
    /// the retired Python harness's CPython `random.Random(seed).shuffle`
    /// order, reproduced by `mt.rs` and pinned against the archived
    /// receipts by `tests/mt_order.rs` (the oracle runs the production
    /// RNG; there is no selectable alternative order).
    pub fn load(path: &Path, seed: u64) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read problems {}", path.display()))?;
        let mut problems = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            problems.push(serde_json::from_str::<Problem>(line).context("parse gsm8k row")?);
        }
        anyhow::ensure!(
            !problems.is_empty(),
            "empty problem set: {}",
            path.display()
        );
        // The archived instrument's dataset guard (ported from the retired
        // gsm8k.rs): the train split is exactly 7,473 rows — a truncated
        // dataset is a typed refusal, never a silently shorter leg.
        anyhow::ensure!(
            problems.len() == 7473,
            "expected 7473 gsm8k train rows, got {} — the dataset is truncated or wrong",
            problems.len()
        );
        let n = problems.len();
        let order = crate::mt::PyRandom::new(seed).shuffle_order(n);
        Ok(Self { problems, order })
    }

    /// The problem the run order assigns to logical row `row`
    /// (row = start + i of the probe loop).
    pub fn at_row(&self, row: usize) -> &Problem {
        let n = self.order.len();
        let idx = self.order[row % n];
        &self.problems[idx]
    }

    pub fn problem_id(&self, row: usize) -> String {
        let n = self.order.len();
        format!("gsm8k-train-{}", self.order[row % n])
    }
}
