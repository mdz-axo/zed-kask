//! Bridge wiring macros for the TUI workspace.
//!
//! The TUI now exposes only the Chat window, which is wired through the
//! `SettingsBridge` and `SessionBridge` traits defined in `repl_bridge`.
//! The domain-specific data bridges (kanban, wallet, memory, etc.) and
//! their window types have been removed; only the bridge-setter macros
//! remain so the workspace and TUI session can wire the Chat window's
//! optional bridges.

// ── Bridge generation macros ──────────────────────────────────────────
//
// `with_bridges!` takes a sub-macro name and a list of bridge specs, then
// invokes the sub-macro for each. Type names are resolved at the call site
// (where the bridge traits are in scope), avoiding macro-definition-site
// hygiene issues with the callback-pattern approach.

/// Invoke `$sub!` for each bridge spec.
/// Each spec: `$field, $trait, $method`
macro_rules! with_bridges {
    ($sub:ident;
     $($field:ident, $trait:ident, $method:ident);+ $(;)?
    ) => {
        $($sub!($field, $trait, $method);)*
    };
}

/// Generate a `Workspace::with_*` setter method.
macro_rules! workspace_bridge_setter {
    ($field:ident, $trait:ident, $method:ident) => {
        pub fn $method(&mut self, bridge: std::sync::Arc<dyn $trait>) -> &mut Self {
            self.bridges.$field = Some(bridge);
            self
        }
    };
}

/// Generate a `TuiSession::with_*` setter method.
macro_rules! tui_bridge_setter {
    ($field:ident, $trait:ident, $method:ident) => {
        pub fn $method(mut self, bridge: std::sync::Arc<dyn $trait>) -> Self {
            self.workspace.$method(bridge);
            self
        }
    };
}

pub(crate) use {tui_bridge_setter, with_bridges, workspace_bridge_setter};
