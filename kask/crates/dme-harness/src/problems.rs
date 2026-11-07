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
    /// Load the train JSONL and compute the run order.
    ///
    /// `order` must be "mt19937" (the retired Python harness's CPython
    /// `random.Random(20261106).shuffle` order, reproduced by `mt.rs` and
    /// pinned by `tests/mt_order.rs`) or "declared" (a documented
    /// xorshift64* Fisher-Yates over the same seed, used if the archived
    /// order turns out not to be the CPython one).
    pub fn load(path: &Path, seed: u64, order: &str) -> Result<Self> {
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
        let n = problems.len();
        let order = match order {
            "mt19937" => crate::mt::PyRandom::new(seed).shuffle_order(n),
            "declared" => declared_order(n, seed),
            other => anyhow::bail!("unknown order {other:?} (use mt19937|declared)"),
        };
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

/// Documented fallback order: Fisher-Yates with xorshift64* — deterministic,
/// reproducible from this file alone.
fn declared_order(n: usize, seed: u64) -> Vec<usize> {
    let mut state = seed;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545F4914F6CDD1D)
    };
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}
