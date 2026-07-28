//! Kata practice history — tracks practice frequency, streaks, and automaticity.
//!
//! Cybernetic feedback types for Improvement Kata signal computation.
//! Persisted per agent to enable composition (graduation criteria, habit monitoring).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::error::KataError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KataHistory {
    pub agents: HashMap<String, Vec<PracticeEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticeEntry {
    pub date: String,
    pub kata_type: String,
    pub practice_name: String,
    pub steps_completed: usize,
    pub gas_consumed: u64,
}

impl KataHistory {
    /// \[P9\] Motivating: Homeostatic Self-Regulation — practice history persisted for habit tracking.
    /// pre:  path may or may not exist
    /// post: returns Ok(KataHistory) from file, or default if file missing, or Err on parse failure
    #[must_use = "result must be used"]
    pub fn load(path: &Path) -> Result<Self, KataError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let json = std::fs::read_to_string(path).map_err(|e| {
            KataError::LoadFailed(format!(
                "Failed to read history from {}: {}",
                path.display(),
                e
            ))
        })?;
        serde_json::from_str(&json)
            .map_err(|e| KataError::ParseFailed(format!("Failed to parse history: {}", e)))
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — practice history serialized to disk.
    /// pre:  self is valid; path is a writable filesystem location
    /// post: history serialized as pretty JSON to path, or Err on failure
    #[must_use = "result must be used"]
    pub fn save(&self, path: &Path) -> Result<(), KataError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                KataError::LoadFailed(format!(
                    "Failed to create history directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| KataError::LoadFailed(format!("Failed to serialize history: {}", e)))?;
        std::fs::write(path, &json).map_err(|e| {
            KataError::LoadFailed(format!(
                "Failed to write history to {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok(())
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — each practice session recorded.
    /// pre:  agent is non-empty; entry is a valid PracticeEntry
    /// post: entry appended to agent's practice history list
    pub fn record(&mut self, agent: &str, entry: PracticeEntry) {
        self.agents
            .entry(agent.to_string())
            .or_default()
            .push(entry);
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — streak computation for habit health.
    /// pre:  agent is non-empty; today is a YYYY-MM-DD date string
    /// post: returns consecutive day streak including today, or 0 if today missing
    #[must_use]
    pub fn current_streak(&self, agent: &str, today: &str) -> u32 {
        let entries = match self.agents.get(agent) {
            Some(e) => e,
            None => return 0,
        };
        let mut dates: Vec<&str> = entries.iter().map(|e| e.date.as_str()).collect();
        dates.sort();
        dates.dedup();
        dates.reverse();

        if dates.is_empty() || dates[0] != today {
            return 0;
        }

        let mut streak = 1u32;
        for window in dates.windows(2) {
            let prev = window[0];
            let next = window[1];
            if is_consecutive_day(prev, next) {
                streak += 1;
            } else {
                break;
            }
        }
        streak
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — automaticity score for kata graduation.
    /// pre:  agent is non-empty; today is a YYYY-MM-DD date string
    /// post: returns score 0.0–1.0 based on streak (target 21d) with decay for gaps >3d
    #[must_use]
    pub fn compute_automaticity(&self, agent: &str, today: &str) -> f64 {
        let streak = self.current_streak(agent, today) as f64;
        let days_since = self.days_since_last(agent, today) as f64;

        let mut auto = (streak / 21.0).min(1.0);

        if days_since > 3.0 {
            auto *= 0.8_f64.powf(days_since / 3.0);
        }

        (auto * 100.0).round() / 100.0
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — gap detection for habit decay.
    /// pre:  agent is non-empty; today is a YYYY-MM-DD date string
    /// post: returns days since last practice, or u32::MAX if no history
    #[must_use]
    pub fn days_since_last(&self, agent: &str, today: &str) -> u32 {
        let entries = match self.agents.get(agent) {
            Some(e) => e,
            None => return u32::MAX,
        };
        let last_date = entries.iter().map(|e| e.date.as_str()).max();
        match last_date {
            Some(last) => days_between(last, today).unwrap_or(u32::MAX),
            None => u32::MAX,
        }
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — starter kata graduation gate.
    /// pre:  agent is non-empty; today is a YYYY-MM-DD date string
    /// post: returns true if automaticity > 0.5 (graduation threshold)
    #[must_use]
    pub fn can_graduate_from_starter(&self, agent: &str, today: &str) -> bool {
        self.compute_automaticity(agent, today) > 0.5
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — habit decay intervention trigger.
    /// pre:  agent is non-empty; today is a YYYY-MM-DD date string
    /// post: returns true if 3+ days since last practice (intervention needed)
    #[must_use]
    pub fn needs_habit_intervention(&self, agent: &str, today: &str) -> bool {
        let days = self.days_since_last(agent, today);
        (3..u32::MAX).contains(&days)
    }
}

fn is_consecutive_day(earlier: &str, later: &str) -> bool {
    let parse = |s: &str| -> Option<(i32, u32, u32)> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };
    let (y1, m1, d1) = match parse(earlier) {
        Some(v) => v,
        None => return false,
    };
    let (y2, m2, d2) = match parse(later) {
        Some(v) => v,
        None => return false,
    };
    let doy = |y: i32, m: u32, d: u32| -> u32 {
        let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let mut doy = d;
        for item in days_in_month.iter().take(m as usize - 1) {
            doy += item;
        }
        if leap && m > 2 {
            doy += 1;
        }
        doy
    };
    let doy1 = doy(y1, m1, d1);
    let doy2 = doy(y2, m2, d2);
    y1 == y2 && doy2 == doy1 + 1 || (y1 + 1 == y2 && m1 == 12 && d1 == 31 && m2 == 1 && d2 == 1)
}

fn days_between(from: &str, to: &str) -> Option<u32> {
    let parse = |s: &str| -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
    };
    let from_d = parse(from)?;
    let to_d = parse(to)?;
    let delta = to_d.signed_duration_since(from_d).num_days();
    if delta < 0 { None } else { Some(delta as u32) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementSignal {
    pub metric_before: Option<serde_json::Value>,
    pub metric_after: Option<serde_json::Value>,
    pub delta: Option<f64>,
    pub direction: ImprovementDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImprovementDirection {
    Positive,
    Negative,
    Stalled,
    NotMeasured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExperience {
    pub agent: String,
    pub kata_type: String,
    pub step_label: String,
    pub action: String,
    pub output_summary: String,
    pub gas_used: u64,
    pub timestamp: String,
}
