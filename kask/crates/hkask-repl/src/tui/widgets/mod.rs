//! Reusable ratatui widgets for the hKask TUI.
//!
//! These are composable rendering components used across multiple
//! window implementations.

/// Energy gauge widget — renders a horizontal bar showing gas usage.
/// Currently rendered inline in the status bar; available for reuse
/// in the Energy window (Tier 2).
pub mod energy_gauge {
    use ratatui::style::Color;
    use ratatui::text::{Line, Span};

    /// Build a styled energy gauge line.
    ///
    /// pre:  gas_remaining ≤ gas_cap
    /// post: returns a Line with filled/empty blocks and percentage
    pub fn render_gauge(gas_remaining: u64, gas_cap: u64) -> Line<'static> {
        let gas_pct = if gas_cap > 0 {
            (gas_remaining as f64 / gas_cap as f64) * 100.0
        } else {
            100.0
        };

        let gas_style = if gas_pct < 20.0 {
            Color::Red
        } else if gas_pct < 50.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        let bar_width = 10;
        let filled = ((gas_pct / 100.0) * bar_width as f64) as usize;
        let empty = bar_width - filled;

        let mut spans = Vec::new();
        spans.push(Span::raw(" Gas: "));
        spans.push(Span::styled(
            "█".repeat(filled),
            ratatui::style::Style::default().fg(gas_style),
        ));
        spans.push(Span::styled(
            "░".repeat(empty),
            ratatui::style::Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            format!(" {:.0}% ", gas_pct),
            ratatui::style::Style::default().fg(gas_style),
        ));

        Line::from(spans)
    }
}

/// Markdown rendering utilities (Tier 2).
pub mod markdown {
    // Placeholder — Tier 2 implementation will convert Markdown
    // strings into ratatui Line/Span with bold, italic, code, and
    // link styling.
}

/// Streaming text display (Tier 2).
pub mod streaming {
    // Placeholder — Tier 2 implementation will render partial
    // inference output as it arrives from the inference port.
}

/// Regulation alert indicator — reusable across windows.
pub mod alert_indicator {
    use ratatui::style::Color;
    use ratatui::text::Span;

    /// Render a Regulation status indicator span.
    pub fn render_reg_status(healthy: bool, alert_count: u32) -> Span<'static> {
        if healthy && alert_count == 0 {
            Span::styled(
                " Regulation: ✓ ",
                ratatui::style::Style::default().fg(Color::Green),
            )
        } else if alert_count < 5 {
            Span::styled(
                format!(" Regulation: ⚠ {} ", alert_count),
                ratatui::style::Style::default().fg(Color::Yellow),
            )
        } else {
            Span::styled(
                format!(" Regulation: ✗ {} ", alert_count),
                ratatui::style::Style::default().fg(Color::Red),
            )
        }
    }
}

/// Header helpers — consistent section title styling.
pub mod headers {
    use ratatui::prelude::Stylize;
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    pub fn section(title: impl Into<String>) -> Line<'static> {
        section_with_color(title, Color::Cyan)
    }

    pub fn section_with_color(title: impl Into<String>, color: Color) -> Line<'static> {
        let title = title.into();
        Line::from(Span::styled(
            format!("── {title} ──"),
            Style::default().fg(color).bold(),
        ))
    }
}
