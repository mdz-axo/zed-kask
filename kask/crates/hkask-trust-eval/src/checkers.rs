//! Deterministic adherence checkers — the eval harness's hand-label
//! instrument (trust internalization Part V).
//!
//! Each checker is a pure predicate over a run output's text. Checkers
//! verify FORM and pinned ground truth (word counts, structure bans,
//! citation formats, code-block content, claim verdicts); citation
//! TRUTH and judgment-bearing instructions are additionally adjudicated
//! (blinded adjudicator + hand labels) — the deterministic floor never
//! replaces the judge, and the judge never bypasses the floor.

use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

/// One deterministic check over a run output.
#[derive(Clone, Debug)]
pub enum Checker {
    /// Total whitespace-word count at most N.
    MaxTotalWords(u32),
    /// No bullet-list lines (`- `, `* `, `+ `, `1. `).
    NoBulletLists,
    /// No markdown heading lines (`#`).
    NoHeadings,
    /// Exactly N paragraphs (blank-line separated).
    ExactlyParagraphs(u32),
    /// A term appears at least N times (word-boundary).
    TermAtLeast(String, u32),
    /// A term appears exactly N times (word-boundary).
    TermExactly(String, u32),
    /// The output contains the string verbatim.
    Contains(String),
    /// At least N citations in `path:line` form.
    CitationsAtLeast(u32),
    /// Exactly N fenced code blocks.
    CodeBlocksExactly(u32),
    /// The largest fenced block has at most N lines.
    CodeBlockMaxLines(u32),
    /// Some fenced block contains the string.
    CodeBlockContains(String),
    /// No fenced block's non-comment code contains the string.
    CodeBlockNotContains(String),
    /// A string appears at least N times inside fenced blocks.
    CodeBlockTermAtLeast(String, u32),
    /// The terms appear, in the given order.
    TermsInOrder(Vec<String>),
    /// The last non-empty line contains the string.
    LastLineContains(String),
    /// Some backtick span (>= min_chars) appears verbatim in the named
    /// repo file. The path is repo-root-relative.
    VerbatimQuoteFrom { file: String, min_chars: usize },
    /// Every one of N claims has a TRUE or FALSE verdict line.
    ClaimsAllAdjudicated { claims: u32 },
    /// Each claim's verdict matches the pinned ground truth.
    ClaimsMatchGroundTruth { verdicts: Vec<bool> },
    /// Every TRUE verdict line also cites `path:line`.
    TrueLinesCiteFileLine { claims: u32 },
}

static CITATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9_./-]+\.(?:rs|toml|md|sh|lean):[0-9]+").expect("citation regex compiles")
});

/// Extract fenced code blocks (content between ``` fence lines). An
/// unclosed final fence is treated as open-ended.
fn code_blocks(output: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in output.lines() {
        if line.trim_start().starts_with("```") {
            match current.take() {
                Some(block) => blocks.push(block.join("\n")),
                None => current = Some(Vec::new()),
            }
        } else if let Some(block) = current.as_mut() {
            block.push(line);
        }
    }
    blocks
}

fn paragraph_count(output: &str) -> usize {
    output
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .count()
}

fn word_count(output: &str) -> usize {
    output.split_whitespace().count()
}

fn term_count(output: &str, term: &str) -> usize {
    let escaped = regex::escape(term);
    let re = Regex::new(&format!(r"\b{escaped}\b")).expect("term regex compiles");
    re.find_iter(output).count()
}

/// Parse the verdict for claim `n` from an output: Some(true) for TRUE,
/// Some(false) for FALSE, None when the claim has no verdict line. The
/// verdict is read at the verdict position (`CLAIM <n>: <verdict>`),
/// never as any word anywhere in the line — a TRUE line quoting
/// `sidestepped: false` must not flip (the V5 baseline artifact).
fn claim_verdict(output: &str, n: u32) -> Option<bool> {
    let re = Regex::new(&format!(r"(?im)^.*\bclaim\s*#?\s*{n}\s*:\s*(true|false)\b"))
        .expect("claim verdict regex compiles");
    re.captures(output).and_then(|c| c.get(1)).and_then(|m| {
        match m.as_str().to_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

/// Resolve a repo-root-relative path from any working directory: walk up
/// from the cwd until the joined path exists (cargo tests run from the
/// crate dir, three levels below the repo root).
fn resolve_repo_file(path: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(path);
    if direct.is_file() {
        return Some(direct);
    }
    let cwd = std::env::current_dir().ok()?;
    let mut ancestor = cwd.as_path();
    for _ in 0..6 {
        let candidate = ancestor.join(path);
        if candidate.is_file() {
            return Some(candidate);
        }
        ancestor = ancestor.parent()?;
    }
    None
}

/// Apply the checker. Pure over the output text; `VerbatimQuoteFrom`
/// reads the named repo file (deterministic, ground truth at check
/// time).
pub fn check(checker: &Checker, output: &str) -> bool {
    match checker {
        Checker::MaxTotalWords(max) => word_count(output) <= *max as usize,
        Checker::NoBulletLists => !output.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("+ ")
                || Regex::new(r"^[0-9]+[.)]\s")
                    .expect("numbered-list regex compiles")
                    .is_match(t)
        }),
        Checker::NoHeadings => !output.lines().any(|l| l.trim_start().starts_with('#')),
        Checker::ExactlyParagraphs(n) => paragraph_count(output) == *n as usize,
        Checker::TermAtLeast(term, n) => term_count(output, term) >= *n as usize,
        Checker::TermExactly(term, n) => term_count(output, term) == *n as usize,
        Checker::Contains(s) => output.contains(s.as_str()),
        Checker::CitationsAtLeast(n) => CITATION_RE.find_iter(output).count() >= *n as usize,
        Checker::CodeBlocksExactly(n) => code_blocks(output).len() == *n as usize,
        Checker::CodeBlockMaxLines(max) => code_blocks(output)
            .iter()
            .all(|b| b.lines().count() <= *max as usize),
        Checker::CodeBlockContains(s) => code_blocks(output).iter().any(|b| b.contains(s.as_str())),
        Checker::CodeBlockNotContains(s) => code_blocks(output).iter().all(|b| {
            b.lines()
                .all(|l| l.trim_start().starts_with("//") || !l.contains(s.as_str()))
        }),
        Checker::CodeBlockTermAtLeast(s, n) => {
            code_blocks(output)
                .iter()
                .map(|b| b.matches(s.as_str()).count())
                .sum::<usize>()
                >= *n as usize
        }
        Checker::TermsInOrder(terms) => {
            let mut last: Option<usize> = None;
            for term in terms {
                let escaped = regex::escape(term);
                let re = Regex::new(&format!(r"\b{escaped}\b")).expect("term regex compiles");
                match re.find(output) {
                    Some(m) => match last {
                        Some(prev) if m.start() <= prev => return false,
                        _ => last = Some(m.start()),
                    },
                    None => return false,
                }
            }
            true
        }
        Checker::LastLineContains(s) => output
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.contains(s.as_str())),
        Checker::VerbatimQuoteFrom { file, min_chars } => {
            let Some(path) = resolve_repo_file(file) else {
                return false;
            };
            let Ok(source) = std::fs::read_to_string(path) else {
                return false;
            };
            let re = Regex::new(&format!("`([^`\\n]{{{},}})`", min_chars))
                .expect("quote-span regex compiles");
            re.captures_iter(output)
                .any(|c| c.get(1).is_some_and(|m| source.contains(m.as_str())))
        }
        Checker::ClaimsAllAdjudicated { claims } => {
            (1..=*claims).all(|n| claim_verdict(output, n).is_some())
        }
        Checker::ClaimsMatchGroundTruth { verdicts } => verdicts
            .iter()
            .enumerate()
            .all(|(i, expected)| claim_verdict(output, i as u32 + 1) == Some(*expected)),
        Checker::TrueLinesCiteFileLine { claims } => {
            (1..=*claims).all(|n| match claim_verdict(output, n) {
                Some(true) => {
                    CITATION_RE
                        .find_iter(
                            output
                                .lines()
                                .find(|l| {
                                    Regex::new(&format!(r"(?i)\bclaim\s*#?\s*{n}\b"))
                                        .expect("claim regex compiles")
                                        .is_match(l)
                                })
                                .unwrap_or(""),
                        )
                        .count()
                        >= 1
                }
                _ => true,
            })
        }
    }
}
