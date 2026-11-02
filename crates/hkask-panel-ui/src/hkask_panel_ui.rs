#![forbid(unsafe_code)]
//! zed-kask: shared UI chrome for the kask GPUI panels — currently the
//! `PanelToggleButton` status-bar button, configured per-panel by the
//! `panel_button` wrappers in `kanban_panel` and `media_panel`.
//!
//! The former marketplace-catalog chrome (card container, bordered search
//! bar, empty-state row) was deleted at the 2026-11-02 RA-01 remediation:
//! its only consumer was the retired swarm panel (D33, 2026-10-28), and the
//! symbol census found zero remaining callers — the shared half that
//! survives is the panel toggle button.

mod panel_button;
pub use panel_button::PanelToggleButton;
