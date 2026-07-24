//! Regression tests for TUI rendering boundary guards.
//!
//! Tests that the TUI rendering code does not panic with ratatui buffer
//! overflows when terminals are resized to extreme dimensions.
//!
//! These tests verify the guards added to prevent:
//! - Splash screen overflow on small terminals
//! - Zero-height content area propagation
//! - Split ratio zero-dimension panes
//! - Widget rendering with degenerate areas

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::tui::splash::SplashScreen;

// ── Splash screen clamping ──────────────────────────────────────────

/// The splash screen logo is 60 rows tall (120 pixels / 2).
/// On terminals shorter than 63 rows, the render area must be clamped
/// to prevent a ratatui buffer overflow.
#[test]
fn splash_renders_on_small_terminal_without_panic() {
    let backend = TestBackend::new(80, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    let splash = SplashScreen::new();

    // Must not panic on a 6-row terminal (far smaller than the 63-row logo).
    terminal
        .draw(|f| splash.render(f))
        .expect("splash should render on 80×6 terminal without panic");
}

#[test]
fn splash_renders_on_exact_boundary() {
    // Height 63 = term_rows (60) + wordmark (1) + prompt (1) + spacing (1)
    let backend = TestBackend::new(80, 63);
    let mut terminal = Terminal::new(backend).unwrap();
    let splash = SplashScreen::new();

    terminal
        .draw(|f| splash.render(f))
        .expect("splash should render on 80×63 terminal (exact fit)");
}

#[test]
fn splash_renders_on_one_row_terminal() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let splash = SplashScreen::new();

    terminal
        .draw(|f| splash.render(f))
        .expect("splash should render on 80×1 terminal without panic");
}

#[test]
fn splash_renders_on_zero_dim_terminal() {
    let backend = TestBackend::new(0, 0);
    let mut terminal = Terminal::new(backend).unwrap();
    let splash = SplashScreen::new();

    terminal
        .draw(|f| splash.render(f))
        .expect("splash should render on 0×0 terminal without panic");
}

// ── Workspace rendering ─────────────────────────────────────────────

#[test]
fn workspace_renders_at_terminal_height_zero() {
    // Workspace::render guards content_h == 0 by returning early.
    let backend = TestBackend::new(80, 0);
    let mut terminal = Terminal::new(backend).unwrap();

    // Create a workspace using the test constructor (single Chat window).
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×0 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_one() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×1 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_two() {
    // With height 2 and tabs=0, status_h=1, content_h = 2-0-1 = 1.
    // A 1-row content area with a bordered window: border consumes 2 rows,
    // inner area = 0. Guards in window renderers must handle this.
    let backend = TestBackend::new(80, 2);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×2 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_three() {
    let backend = TestBackend::new(80, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×3 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_four() {
    // content_h = 4-0-1 = 3. Chat window needs 5 minimum → guard fires.
    let backend = TestBackend::new(80, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×4 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_five() {
    // content_h = 5-0-1 = 4. Chat window needs 5 minimum → guard fires.
    // This is the boundary: chat guard fires at area.height < 5.
    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×5 terminal without panic");
}

#[test]
fn workspace_renders_at_terminal_height_six() {
    // content_h = 5. Chat guard: area.height(5) < 5 → false → renders.
    let backend = TestBackend::new(80, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 80×6 terminal without panic");
}

#[test]
fn workspace_renders_at_narrow_terminal() {
    // SplitNode guards fire at left_w < 2 || right_w < 2.
    // At width 3, a 0.65 ratio split: left = round(3*0.65) = 2, right = 3-2 = 1.
    // The < 2 guard fires for the right pane.
    let backend = TestBackend::new(3, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 3×24 terminal without panic");
}

#[test]
fn workspace_renders_at_one_column_terminal() {
    let backend = TestBackend::new(1, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let (system, repl) = crate::tui::test_util::mock_bridges();
    let workspace = crate::tui::workspace::Workspace::new_test(system, repl);

    terminal
        .draw(|f| workspace.render(f))
        .expect("workspace should render on 1×24 terminal without panic");
}
