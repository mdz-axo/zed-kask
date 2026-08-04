//! GPUI portfolio widget for rendering ```` ```portfolio ```` fenced blocks
//! inline in agent markdown. Replaces the deleted standalone
//! `PortfolioDashboardView` (from the removed `kask_panel` crate) with a
//! passive inline renderer that consumes the combined `companies` MCP tool
//! output the curator agent emits as a fenced block.
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the media and graph renderers. The agent emits a
//! fenced block whose body is the combined tool result, e.g.:
//!
//! ```text
//! ```portfolio
//! { "viz": "portfolio", "portfolio": "main",
//!   "returns": { "total_return": 0.12, "irr": 0.08, ... },
//!   "characteristics": { "pe_ratio": { "value": 15.2, ... }, ... },
//!   "attribution": [ { "symbol": "AAPL", "contribution_bps": 60, ... }, ... ] }
//! ```
//! ```
//!
//! The widget is read-only: no portfolio selector, aggregation/date controls,
//! auto-loader, or comparison mode. The agent picks the portfolio and the
//! block body already contains the data for it.
#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod view;

use gpui::{App, AppContext, Entity};

pub use view::PortfolioWidget;

/// Create a `PortfolioWidget` entity from a block body, without wrapping it in
/// an element. Used by `hkask_viz_core::block_renderer` to cache the entity
/// across renders.
///
/// Self-selects on a JSON body whose `viz` field equals `"portfolio"`. Other
/// bodies (including media blocks, which are claimed first by the media
/// renderer in [`hkask_viz_core::block_renderer`], and graph blocks claimed by
/// the graph renderer) fall through with `None` so the default code-block
/// renderer handles them.
///
/// Returns `None` if the body is not a valid `portfolio` block.
pub fn create_portfolio_widget(body: &str, cx: &mut App) -> Option<Entity<view::PortfolioWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match block::parse_portfolio_body(body) {
        Ok(parsed) if parsed.viz.as_deref() == Some("portfolio") => {
            Some(cx.new(|cx| view::PortfolioWidget::new(parsed, cx)))
        }
        Ok(_) => None,
        Err(error) => {
            log::warn!("hkask-portfolio-widget: malformed portfolio block: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the D18 portfolio block self-selection: a valid portfolio body is
    // claimed; media-shaped, graph-shaped, and plain-text bodies fall through.
    // See DIVERGENCE.md D18 and the .rules "Tests must pin deliberate zed-kask
    // deviations from upstream".
    #[test]
    fn selects_portfolio_body() {
        let body = r#"{"viz":"portfolio","portfolio":"main"}"#;
        let parsed = block::parse_portfolio_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("portfolio"));
        assert_eq!(parsed.portfolio.as_deref(), Some("main"));
    }

    #[test]
    fn falls_through_non_portfolio_bodies() {
        // A media-shaped body has no `viz` field → parsed (viz None) but not
        // claimed by the portfolio renderer.
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = block::parse_portfolio_body(media).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("portfolio"));

        // A graph-shaped body has a different `viz` value → not claimed.
        let graph = r#"{"viz":"event_tree","nodes":[{"id":"e0","name":"root"}]}"#;
        let parsed = block::parse_portfolio_body(graph).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("portfolio"));

        // Plain text is not JSON → parse fails → renderer returns None.
        assert!(block::parse_portfolio_body("not json").is_err());
    }
}
