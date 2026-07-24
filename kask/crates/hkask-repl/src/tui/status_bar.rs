//! Status bar — global system state display.
//!
//! Renders a single-line status bar showing model, energy budget,
//! Regulation health, and context window pressure. This is a cybernetic
//! display surface (P9): it closes the "is the system healthy?"
//! feedback loop.

use std::collections::HashMap;

use ratatui::style::Color;
use ratatui::text::{Line, Span};

use crate::tui::widgets::energy_gauge;
use crate::tui::window::WindowId;

/// Default condensation pressure threshold (matches ReplSettings default).
/// When context pressure exceeds this fraction, the status bar displays red.
const DEFAULT_CONDENSE_THRESHOLD: f64 = 0.875;

/// Warning threshold for context pressure display (percentage).
/// Above this, the status bar displays yellow; below, dark gray.
const CONTEXT_WARNING_PCT: f64 = 60.0;

pub struct StatusBar {
    pub model: String,
    pub gas_remaining: u64,
    pub gas_cap: u64,
    pub reg_status: RegStatus,
    pub context_pressure: f64,
    /// Condensation pressure threshold (0.0–1.0). When context pressure
    /// exceeds this, the display turns red. Defaults to 0.875 (87.5%),
    /// matching `ReplSettings::condense_pressure_threshold`.
    pub condense_threshold: f64,
    pub show_hints: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegStatus {
    Healthy,
    Warning(u32),
    Critical(u32),
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            gas_remaining: 0,
            gas_cap: 10_000,
            reg_status: RegStatus::Healthy,
            context_pressure: 0.0,
            condense_threshold: DEFAULT_CONDENSE_THRESHOLD,
            show_hints: true,
        }
    }

    pub fn render(&self, focused: WindowId, titles: &HashMap<WindowId, String>) -> Line<'static> {
        self.build_line(focused, &|id| titles.get(&id).cloned())
    }

    fn build_line(
        &self,
        focused: WindowId,
        title_lookup: &dyn Fn(WindowId) -> Option<String>,
    ) -> Line<'static> {
        let mut spans: Vec<Span> = Vec::new();

        if !self.model.is_empty() {
            spans.push(Span::styled(
                format!(" Model: {} ", self.model),
                ratatui::style::Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw("│"));
        }

        // Energy gauge — delegates to the shared widget (one source of truth)
        spans.extend(energy_gauge::render_gauge(self.gas_remaining, self.gas_cap).spans);
        spans.push(Span::raw("│"));

        match self.reg_status {
            RegStatus::Healthy => {
                spans.push(Span::styled(
                    " Regulation: ✓ ",
                    ratatui::style::Style::default().fg(Color::Green),
                ));
            }
            RegStatus::Warning(n) => {
                spans.push(Span::styled(
                    format!(" Regulation: ⚠ {} ", n),
                    ratatui::style::Style::default().fg(Color::Yellow),
                ));
            }
            RegStatus::Critical(n) => {
                spans.push(Span::styled(
                    format!(" Regulation: ✗ {} ", n),
                    ratatui::style::Style::default().fg(Color::Red),
                ));
            }
        }
        spans.push(Span::raw("│"));

        let ctx_pct = self.context_pressure * 100.0;
        let condense_pct = self.condense_threshold * 100.0;
        let ctx_style = if ctx_pct > condense_pct {
            Color::Red
        } else if ctx_pct > CONTEXT_WARNING_PCT {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        spans.push(Span::styled(
            format!(" ctx: {:.0}% ", ctx_pct),
            ratatui::style::Style::default().fg(ctx_style),
        ));

        if let Some(title) = title_lookup(focused) {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("[{}]", title),
                ratatui::style::Style::default().fg(Color::White),
            ));
        }

        if self.show_hints {
            spans.push(Span::styled(
                "  ^Q quit  ^W window  ^T tab",
                ratatui::style::Style::default().fg(Color::DarkGray),
            ));
        }

        Line::from(spans)
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}
