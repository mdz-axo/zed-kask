//! Workspace — split-tree layout, tab management, focus, and event routing.
//!
//! Each tab owns a `SplitNode` binary tree. Windows are stored directly
//! in leaf nodes. The workspace routes render, key events, focus, and
//! tick cycles through the active tab's tree.

use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use uuid::Uuid;

use crate::tui::bridges::{with_bridges, workspace_bridge_setter};
use crate::tui::repl_bridge::{
    ReplBridge, SessionBridge, SettingsBridge, SystemBridge, ToolInvokeBridge,
};
use crate::tui::status_bar::StatusBar;
use crate::tui::tab::Tab;
use crate::tui::window::{SplitDirection, Window, WindowId, WindowKind, WorkspaceAction};

/// Binary split tree. Leaves hold windows directly; internal nodes
/// split the area horizontally or vertically with a ratio.
pub enum SplitNode {
    Leaf(Box<dyn Window>),
    Horizontal {
        left: Box<SplitNode>,
        right: Box<SplitNode>,
        ratio: f32,
    },
    Vertical {
        top: Box<SplitNode>,
        bottom: Box<SplitNode>,
        ratio: f32,
    },
}

impl std::fmt::Debug for SplitNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitNode::Leaf(w) => f.debug_tuple("Leaf").field(&w.id()).finish(),
            SplitNode::Horizontal { ratio, .. } => {
                f.debug_struct("H").field("ratio", ratio).finish()
            }
            SplitNode::Vertical { ratio, .. } => f.debug_struct("V").field("ratio", ratio).finish(),
        }
    }
}

impl SplitNode {
    fn collect_ids(&self, out: &mut Vec<WindowId>) {
        match self {
            SplitNode::Leaf(w) => out.push(w.id()),
            SplitNode::Horizontal { left, right, .. } => {
                left.collect_ids(out);
                right.collect_ids(out);
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.collect_ids(out);
                bottom.collect_ids(out);
            }
        }
    }

    pub fn window_ids(&self) -> Vec<WindowId> {
        let mut ids = Vec::new();
        self.collect_ids(&mut ids);
        ids
    }

    pub fn window_kind(&self, target: WindowId) -> Option<WindowKind> {
        match self {
            SplitNode::Leaf(w) if w.id() == target => Some(w.kind()),
            SplitNode::Leaf(_) => None,
            SplitNode::Horizontal { left, right, .. } => left
                .window_kind(target)
                .or_else(|| right.window_kind(target)),
            SplitNode::Vertical { top, bottom, .. } => top
                .window_kind(target)
                .or_else(|| bottom.window_kind(target)),
        }
    }

    pub fn contains_window(&self, target: WindowId) -> bool {
        match self {
            SplitNode::Leaf(w) => w.id() == target,
            SplitNode::Horizontal { left, right, .. } => {
                left.contains_window(target) || right.contains_window(target)
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.contains_window(target) || bottom.contains_window(target)
            }
        }
    }

    fn find_leaf_mut(&mut self, target: WindowId) -> Option<&mut Box<dyn Window>> {
        match self {
            SplitNode::Leaf(w) if w.id() == target => Some(w),
            SplitNode::Leaf(_) => None,
            SplitNode::Horizontal { left, right, .. } => left
                .find_leaf_mut(target)
                .or_else(|| right.find_leaf_mut(target)),
            SplitNode::Vertical { top, bottom, .. } => top
                .find_leaf_mut(target)
                .or_else(|| bottom.find_leaf_mut(target)),
        }
    }

    /// Replace the leaf matching `target` with a split containing the old
    /// window and `new_win`. If the target is not found, returns the tree
    /// unchanged (with `new_win` dropped).
    fn replace_leaf_with_split(
        self,
        target: WindowId,
        new_win: Box<dyn Window>,
        dir: SplitDirection,
    ) -> SplitNode {
        match self {
            SplitNode::Leaf(w) if w.id() == target => match dir {
                SplitDirection::Vertical => SplitNode::Vertical {
                    top: Box::new(SplitNode::Leaf(w)),
                    bottom: Box::new(SplitNode::Leaf(new_win)),
                    ratio: 0.5,
                },
                SplitDirection::Horizontal => SplitNode::Horizontal {
                    left: Box::new(SplitNode::Leaf(w)),
                    right: Box::new(SplitNode::Leaf(new_win)),
                    ratio: 0.5,
                },
            },
            SplitNode::Leaf(w) => SplitNode::Leaf(w),
            SplitNode::Horizontal { left, right, ratio } => {
                if left.contains_window(target) {
                    SplitNode::Horizontal {
                        left: Box::new((*left).replace_leaf_with_split(target, new_win, dir)),
                        right,
                        ratio,
                    }
                } else {
                    SplitNode::Horizontal {
                        left,
                        right: Box::new((*right).replace_leaf_with_split(target, new_win, dir)),
                        ratio,
                    }
                }
            }
            SplitNode::Vertical { top, bottom, ratio } => {
                if top.contains_window(target) {
                    SplitNode::Vertical {
                        top: Box::new((*top).replace_leaf_with_split(target, new_win, dir)),
                        bottom,
                        ratio,
                    }
                } else {
                    SplitNode::Vertical {
                        top,
                        bottom: Box::new((*bottom).replace_leaf_with_split(target, new_win, dir)),
                        ratio,
                    }
                }
            }
        }
    }

    /// Returns a new tree with the target window removed. Splits collapse
    /// to the surviving sibling. Returns `None` if the target was the
    /// only window in the tree.
    fn remove_window(self, target: WindowId) -> Option<SplitNode> {
        match self {
            SplitNode::Leaf(w) if w.id() == target => None,
            SplitNode::Leaf(w) => Some(SplitNode::Leaf(w)),
            SplitNode::Horizontal { left, right, ratio } => {
                match (left.remove_window(target), right.remove_window(target)) {
                    (None, Some(r)) => Some(r),
                    (Some(l), None) => Some(l),
                    (Some(l), Some(r)) => Some(SplitNode::Horizontal {
                        left: Box::new(l),
                        right: Box::new(r),
                        ratio,
                    }),
                    (None, None) => None,
                }
            }
            SplitNode::Vertical { top, bottom, ratio } => {
                match (top.remove_window(target), bottom.remove_window(target)) {
                    (None, Some(b)) => Some(b),
                    (Some(t), None) => Some(t),
                    (Some(t), Some(b)) => Some(SplitNode::Vertical {
                        top: Box::new(t),
                        bottom: Box::new(b),
                        ratio,
                    }),
                    (None, None) => None,
                }
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, focused_id: Option<WindowId>) {
        match self {
            SplitNode::Leaf(w) => {
                let focused = focused_id == Some(w.id());
                let bs = if focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let block = Block::default()
                    .title(w.title())
                    .borders(Borders::ALL)
                    .border_style(bs);
                let inner = block.inner(area);
                f.render_widget(block, area);
                w.render(f, inner, focused);
            }
            SplitNode::Horizontal { left, right, ratio } => {
                let left_w = ((area.width as f32) * ratio).round() as u16;
                let right_w = area.width.saturating_sub(left_w);
                if left_w < 2 || right_w < 2 {
                    return;
                }
                left.render(
                    f,
                    Rect::new(area.x, area.y, left_w, area.height),
                    focused_id,
                );
                right.render(
                    f,
                    Rect::new(area.x + left_w, area.y, right_w, area.height),
                    focused_id,
                );
            }
            SplitNode::Vertical { top, bottom, ratio } => {
                let top_h = ((area.height as f32) * ratio).round() as u16;
                let bottom_h = area.height.saturating_sub(top_h);
                if top_h < 2 || bottom_h < 2 {
                    return;
                }
                top.render(f, Rect::new(area.x, area.y, area.width, top_h), focused_id);
                bottom.render(
                    f,
                    Rect::new(area.x, area.y + top_h, area.width, bottom_h),
                    focused_id,
                );
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, focused_id: Option<WindowId>) -> bool {
        match self {
            SplitNode::Leaf(w) => focused_id == Some(w.id()) && w.handle_key(key),
            SplitNode::Horizontal { left, right, .. } => {
                left.handle_key(key, focused_id) || right.handle_key(key, focused_id)
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.handle_key(key, focused_id) || bottom.handle_key(key, focused_id)
            }
        }
    }

    fn tick(&mut self) {
        match self {
            SplitNode::Leaf(w) => w.tick(),
            SplitNode::Horizontal { left, right, .. } => {
                left.tick();
                right.tick();
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.tick();
                bottom.tick();
            }
        }
    }

    fn titles(&self, out: &mut HashMap<WindowId, String>) {
        match self {
            SplitNode::Leaf(w) => {
                out.insert(w.id(), w.title().to_string());
            }
            SplitNode::Horizontal { left, right, .. } => {
                left.titles(out);
                right.titles(out);
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.titles(out);
                bottom.titles(out);
            }
        }
    }

    /// Find and refocus a singleton window of `kind` if one exists.
    /// Returns the window's id if found.
    fn find_by_kind(&self, kind: WindowKind) -> Option<WindowId> {
        match self {
            SplitNode::Leaf(w) if w.kind() == kind => Some(w.id()),
            SplitNode::Leaf(_) => None,
            SplitNode::Horizontal { left, right, .. } => {
                left.find_by_kind(kind).or_else(|| right.find_by_kind(kind))
            }
            SplitNode::Vertical { top, bottom, .. } => {
                top.find_by_kind(kind).or_else(|| bottom.find_by_kind(kind))
            }
        }
    }
}

/// Minimal placeholder window used during tree surgery.
/// It is immediately replaced by the real window or sibling subtree.
struct PlaceholderWindow {
    id: WindowId,
}

impl PlaceholderWindow {
    fn new(id: WindowId) -> Self {
        Self { id }
    }
}

impl Window for PlaceholderWindow {
    fn id(&self) -> WindowId {
        self.id
    }
    fn title(&self) -> &str {
        ""
    }
    /// Returns `Chat` — this is safe because the placeholder lives for one
    /// synchronous line during tree surgery and is never observed by
    /// `find_by_kind`, `render`, `tick`, or `extract_layout`. Chat is the
    /// only `allows_multiple` kind, so singleton searches never match it.
    /// If a new `find_by_kind` call is added during surgery, revisit this.
    fn kind(&self) -> WindowKind {
        WindowKind::Chat
    }
    fn render(&self, _f: &mut Frame, _area: Rect, _is_focused: bool) {}
    fn handle_key(&mut self, _key: KeyEvent) -> bool {
        false
    }
}

/// Keymap prefix-mode state for Ctrl-W subcommands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeymapState {
    /// Normal — no prefix active.
    Normal,
    /// Waiting for the next key after Ctrl-W (window operations).
    AwaitWindow,
}

pub struct Workspace {
    tabs: Vec<Tab>,
    active_tab: usize,
    focused_window: Option<WindowId>,
    /// Whether the session should quit (set by Ctrl+Q or /quit).
    pub should_quit: bool,
    bridges: WorkspaceBridges,
    status_bar: StatusBar,
    keymap_state: KeymapState,
    keymap_timeout: u8,
}

struct WorkspaceBridges {
    system_bridge: Arc<dyn SystemBridge>,
    repl_bridge: Arc<dyn ReplBridge>,
    settings_bridge: Option<Arc<dyn SettingsBridge>>,
    session_bridge: Option<Arc<dyn SessionBridge>>,
    tool_invoke_bridge: Option<Arc<dyn ToolInvokeBridge>>,
}

impl WorkspaceBridges {
    fn to_window_bridges(&self) -> crate::tui::window_catalog::WindowBridges {
        crate::tui::window_catalog::WindowBridges {
            system_bridge: self.system_bridge.clone(),
            repl_bridge: self.repl_bridge.clone(),
            settings_bridge: self.settings_bridge.clone(),
            session_bridge: self.session_bridge.clone(),
            tool_invoke_bridge: self.tool_invoke_bridge.clone(),
        }
    }
}

impl Workspace {
    pub fn new(system: Arc<dyn SystemBridge>, repl: Arc<dyn ReplBridge>) -> Self {
        let model = system.model_name().to_string();
        let chat_id = WindowId(Uuid::new_v4());
        let bridges = WorkspaceBridges {
            system_bridge: system.clone(),
            repl_bridge: repl.clone(),
            settings_bridge: None,
            session_bridge: None,
            tool_invoke_bridge: None,
        };
        let factory_ctx = bridges.to_window_bridges();
        let chat =
            crate::tui::window_catalog::create_window(WindowKind::Chat, chat_id, &factory_ctx);

        let root = SplitNode::Leaf(chat);
        let tab = Tab::new("Chat".to_string(), root);

        let mut status_bar = StatusBar::new();
        status_bar.model = model;
        status_bar.gas_remaining = system.gas_remaining();
        status_bar.gas_cap = system.gas_cap();

        Self {
            tabs: vec![tab],
            active_tab: 0,
            focused_window: Some(chat_id),
            should_quit: false,
            bridges,
            status_bar,
            keymap_state: KeymapState::Normal,
            keymap_timeout: 0,
        }
    }

    with_bridges!(workspace_bridge_setter;
        settings_bridge, SettingsBridge, with_settings_bridge;
        session_bridge, SessionBridge, with_session_bridge;
        tool_invoke_bridge, ToolInvokeBridge, with_tool_invoke_bridge
    );

    /// Create a minimal workspace for testing — single Chat window.
    pub fn new_test(system: Arc<dyn SystemBridge>, repl: Arc<dyn ReplBridge>) -> Self {
        let model = system.model_name().to_string();
        let chat_id = WindowId(Uuid::new_v4());
        let bridges = WorkspaceBridges {
            system_bridge: system.clone(),
            repl_bridge: repl.clone(),
            settings_bridge: None,
            session_bridge: None,
            tool_invoke_bridge: None,
        };
        let factory_ctx = bridges.to_window_bridges();
        let chat =
            crate::tui::window_catalog::create_window(WindowKind::Chat, chat_id, &factory_ctx);
        let root = SplitNode::Leaf(chat);
        let tab = Tab::new("Test".to_string(), root);

        let mut status_bar = StatusBar::new();
        status_bar.model = model;
        status_bar.gas_remaining = system.gas_remaining();
        status_bar.gas_cap = system.gas_cap();

        Self {
            tabs: vec![tab],
            active_tab: 0,
            focused_window: Some(chat_id),
            should_quit: false,
            bridges,
            status_bar,
            keymap_state: KeymapState::Normal,
            keymap_timeout: 0,
        }
    }

    // ── Accessors ───────────────────────────────────────────────────

    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused_window
    }

    pub fn window_count(&self) -> usize {
        self.root().window_ids().len()
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab_index(&self) -> usize {
        self.active_tab
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty() || self.tabs[self.active_tab].root.window_ids().is_empty()
    }

    pub fn root(&self) -> &SplitNode {
        &self.tabs[self.active_tab].root
    }
    fn root_mut(&mut self) -> &mut SplitNode {
        &mut self.tabs[self.active_tab].root
    }

    // ── Rendering ────────────────────────────────────────────────────

    pub fn render(&self, f: &mut Frame) {
        let area = f.area();
        let tab_h = if self.tabs.len() > 1 { 1u16 } else { 0u16 };
        let status_h = 1u16;
        let content_h = area.height.saturating_sub(tab_h).saturating_sub(status_h);

        if content_h == 0 {
            return;
        }

        let mut y = area.y;
        if tab_h > 0 {
            self.render_tab_bar(f, Rect::new(area.x, y, area.width, tab_h));
            y += tab_h;
        }
        let content_area = Rect::new(area.x, y, area.width, content_h);
        y += content_h;
        let status_area = Rect::new(area.x, y, area.width, status_h);

        self.root().render(f, content_area, self.focused_window);
        self.render_status(f, status_area);
    }

    fn render_tab_bar(&self, f: &mut Frame, area: Rect) {
        let parts: Vec<String> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                if i == self.active_tab {
                    format!(" [{}] ", tab.name)
                } else {
                    format!("  {}  ", tab.name)
                }
            })
            .collect();
        let bar = Paragraph::new(parts.join(""))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
        f.render_widget(bar, area);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let mut titles = HashMap::new();
        self.root().titles(&mut titles);

        let bar_text = self.status_bar.render(
            self.focused_window.unwrap_or(WindowId(Uuid::nil())),
            &titles,
        );
        let bar = Paragraph::new(bar_text)
            .style(Style::default().fg(Color::White).bg(Color::Rgb(30, 30, 40)));
        f.render_widget(bar, area);
    }

    // ── Events ───────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        let focused = self.focused_window;
        self.root_mut().handle_key(key, focused);
    }

    /// Handle global keybindings. Returns true if the event was consumed.
    ///
    /// Supports a Ctrl-W prefix sub-mode for window operations (vim-style):
    ///   Ctrl-W v = split vertical, Ctrl-W s = split horizontal,
    ///   Ctrl-W c = close, Ctrl-W w = cycle focus next,
    ///   Ctrl-W p = cycle focus prev
    pub fn handle_global_key(&mut self, key: KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // ── Prefix sub-mode: waiting for the key after Ctrl-W ──
        if self.keymap_state == KeymapState::AwaitWindow {
            self.keymap_state = KeymapState::Normal;
            match key.code {
                KeyCode::Char('v') => {
                    self.apply_action(WorkspaceAction::Split(SplitDirection::Vertical));
                    return true;
                }
                KeyCode::Char('s') => {
                    self.apply_action(WorkspaceAction::Split(SplitDirection::Horizontal));
                    return true;
                }
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    self.apply_action(WorkspaceAction::CloseFocused);
                    return true;
                }
                KeyCode::Char('w') | KeyCode::Tab => {
                    self.apply_action(WorkspaceAction::FocusNext);
                    return true;
                }
                KeyCode::Char('p') | KeyCode::Char('W') => {
                    self.apply_action(WorkspaceAction::FocusPrev);
                    return true;
                }
                _ => return false,
            }
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                self.should_quit = true;
                true
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.keymap_state = KeymapState::AwaitWindow;
                self.keymap_timeout = 60;
                true
            }
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                self.apply_action(WorkspaceAction::NewTab(None));
                true
            }
            (KeyModifiers::CONTROL, KeyCode::Tab) => {
                self.apply_action(WorkspaceAction::NextTab);
                true
            }
            (KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.apply_action(WorkspaceAction::PrevTab);
                true
            }
            _ => false,
        }
    }

    // ── Window management ────────────────────────────────────────────

    /// Dispatch a workspace action. Called from `tick()` (drained from
    /// windows) and from `handle_global_key` (keybindings).
    fn apply_action(&mut self, action: WorkspaceAction) {
        match action {
            WorkspaceAction::Quit => self.should_quit = true,
            WorkspaceAction::OpenWindow(kind) => self.open_window(kind),
            WorkspaceAction::CloseFocused => self.close_focused(),
            WorkspaceAction::Split(dir) => self.split_focused(dir),
            WorkspaceAction::FocusNext => self.focus_next(),
            WorkspaceAction::FocusPrev => self.focus_prev(),
            WorkspaceAction::NewTab(name) => self.new_tab(name),
            WorkspaceAction::NextTab => self.next_tab(),
            WorkspaceAction::PrevTab => self.prev_tab(),
        }
    }

    /// Open a window of `kind`. If the kind is a singleton and already
    /// exists in any tab, refocus it. Otherwise, split the focused window
    /// vertically and place the new window below (ratio 0.5).
    fn open_window(&mut self, kind: WindowKind) {
        // Singleton refocus: if this kind already exists, just focus it.
        if !kind.allows_multiple() {
            for (i, tab) in self.tabs.iter().enumerate() {
                if let Some(id) = tab.root.find_by_kind(kind) {
                    self.active_tab = i;
                    self.focus_window(id);
                    return;
                }
            }
        }

        let new_id = WindowId(Uuid::new_v4());
        let new_win = self.create_window_of_kind(kind, new_id);

        let Some(focused) = self.focused_window else {
            // No focused window — wrap root in a split.
            let old_root = std::mem::replace(
                self.root_mut(),
                SplitNode::Leaf(Box::new(PlaceholderWindow::new(WindowId(Uuid::nil())))),
            );
            *self.root_mut() = SplitNode::Vertical {
                top: Box::new(old_root),
                bottom: Box::new(SplitNode::Leaf(new_win)),
                ratio: 0.5,
            };
            self.focus_window(new_id);
            return;
        };

        // Replace the focused leaf with a vertical split (old on top, new on bottom).
        let old_root = std::mem::replace(
            self.root_mut(),
            SplitNode::Leaf(Box::new(PlaceholderWindow::new(WindowId(Uuid::nil())))),
        );
        *self.root_mut() =
            old_root.replace_leaf_with_split(focused, new_win, SplitDirection::Vertical);
        self.focus_window(new_id);
    }

    /// Split the focused window in `dir`. The existing window stays;
    /// a new Chat window fills the other half.
    fn split_focused(&mut self, dir: SplitDirection) {
        let Some(focused) = self.focused_window else {
            return;
        };

        let new_id = WindowId(Uuid::new_v4());
        let new_win = self.create_window_of_kind(WindowKind::Chat, new_id);

        let old_root = std::mem::replace(
            self.root_mut(),
            SplitNode::Leaf(Box::new(PlaceholderWindow::new(WindowId(Uuid::nil())))),
        );
        *self.root_mut() = old_root.replace_leaf_with_split(focused, new_win, dir);
        self.focus_window(new_id);
    }

    /// Close the focused window. The split collapses to the surviving
    /// sibling. If this is the last window in the tab, the tab closes
    /// (or is replaced with a fresh Chat if it's the only tab).
    fn close_focused(&mut self) {
        let Some(focused) = self.focused_window else {
            return;
        };
        // Don't close a window that isn't in the active tab's tree.
        if !self.root().contains_window(focused) {
            return;
        }

        let count = self.root().window_ids().len();
        if count <= 1 {
            // Last window in tab.
            if self.tabs.len() > 1 {
                self.close_tab(self.active_tab);
            } else {
                // Only tab — replace with a fresh Chat.
                let chat_id = WindowId(Uuid::new_v4());
                let chat = self.create_window_of_kind(WindowKind::Chat, chat_id);
                *self.root_mut() = SplitNode::Leaf(chat);
                self.focused_window = Some(chat_id);
            }
            return;
        }

        // Take root out, remove the target window, put it back.
        let old_root = std::mem::replace(
            self.root_mut(),
            SplitNode::Leaf(Box::new(PlaceholderWindow::new(WindowId(Uuid::nil())))),
        );
        if let Some(new_root) = old_root.remove_window(focused) {
            *self.root_mut() = new_root;
            if let Some(&first) = self.root().window_ids().first() {
                self.focus_window(first);
            } else {
                self.focused_window = None;
            }
        }
    }

    /// Create a new tab with a fresh Chat window.
    fn new_tab(&mut self, name: Option<String>) {
        let chat_id = WindowId(Uuid::new_v4());
        let chat = self.create_window_of_kind(WindowKind::Chat, chat_id);
        let tab_name = name.unwrap_or_else(|| format!("Tab {}", self.tabs.len() + 1));
        let tab = Tab::new(tab_name, SplitNode::Leaf(chat));
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.focused_window = Some(chat_id);
    }

    /// Close the tab at `idx`. Refuses the last tab.
    fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        if let Some(&first) = self.root().window_ids().first() {
            self.focused_window = Some(first);
        } else {
            self.focused_window = None;
        }
    }

    fn next_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
        if let Some(&first) = self.root().window_ids().first() {
            self.focused_window = Some(first);
        }
    }

    fn prev_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.active_tab = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
        if let Some(&first) = self.root().window_ids().first() {
            self.focused_window = Some(first);
        }
    }

    // ── Focus ────────────────────────────────────────────────────────

    pub fn focus_next(&mut self) {
        let ids = self.root().window_ids();
        if let Some(ref fw) = self.focused_window {
            if let Some(pos) = ids.iter().position(|id| id == fw) {
                self.focus_window(ids[(pos + 1) % ids.len()]);
            }
        } else if let Some(&first) = ids.first() {
            self.focus_window(first);
        }
    }

    pub fn focus_prev(&mut self) {
        let ids = self.root().window_ids();
        if let Some(ref fw) = self.focused_window {
            if let Some(pos) = ids.iter().position(|id| id == fw) {
                let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
                self.focus_window(ids[prev]);
            }
        } else if let Some(&first) = ids.first() {
            self.focus_window(first);
        }
    }

    fn create_window_of_kind(&self, kind: WindowKind, id: WindowId) -> Box<dyn Window> {
        let ctx = self.bridges.to_window_bridges();
        crate::tui::window_catalog::create_window(kind, id, &ctx)
    }

    fn focus_window(&mut self, id: WindowId) {
        if self.focused_window == Some(id) {
            return;
        }
        // Don't focus a window that isn't in the active tab's tree.
        if !self.root().contains_window(id) {
            return;
        }
        if let Some(prev) = self.focused_window
            && let Some(w) = self.root_mut().find_leaf_mut(prev)
        {
            w.on_blur();
        }
        self.focused_window = Some(id);
        if let Some(w) = self.root_mut().find_leaf_mut(id) {
            w.on_focus();
        }
    }

    // ── Layout persistence ────────────────────────────────────────────

    pub fn extract_layout(&self) -> crate::tui::layout::SavedLayout {
        let tabs: Vec<crate::tui::layout::SavedTab> = self
            .tabs
            .iter()
            .map(|tab| crate::tui::layout::SavedTab {
                name: tab.name.clone(),
                root: Self::extract_split(&tab.root),
            })
            .collect();
        crate::tui::layout::SavedLayout {
            version: 1,
            tabs,
            active_tab: self.active_tab,
        }
    }

    fn extract_split(node: &SplitNode) -> crate::tui::layout::SavedSplit {
        match node {
            SplitNode::Leaf(window) => {
                crate::tui::layout::SavedSplit::Leaf(crate::tui::layout::SavedLeaf {
                    kind: crate::tui::layout::kind_to_string(window.kind()),
                })
            }
            SplitNode::Horizontal { left, right, ratio } => {
                crate::tui::layout::SavedSplit::Horizontal {
                    left: Box::new(Self::extract_split(left)),
                    right: Box::new(Self::extract_split(right)),
                    ratio: *ratio,
                }
            }
            SplitNode::Vertical { top, bottom, ratio } => {
                crate::tui::layout::SavedSplit::Vertical {
                    top: Box::new(Self::extract_split(top)),
                    bottom: Box::new(Self::extract_split(bottom)),
                    ratio: *ratio,
                }
            }
        }
    }

    fn build_window(&self, kind: crate::tui::layout::SavedLeaf) -> Box<dyn Window> {
        let wk = crate::tui::layout::string_to_kind(&kind.kind);
        let new_id = WindowId(uuid::Uuid::new_v4());
        self.create_window_of_kind(wk, new_id)
    }

    fn restore_split(&self, saved: &crate::tui::layout::SavedSplit) -> SplitNode {
        match saved {
            crate::tui::layout::SavedSplit::Leaf(leaf) => {
                SplitNode::Leaf(self.build_window(leaf.clone()))
            }
            crate::tui::layout::SavedSplit::Horizontal { left, right, ratio } => {
                SplitNode::Horizontal {
                    left: Box::new(self.restore_split(left)),
                    right: Box::new(self.restore_split(right)),
                    ratio: *ratio,
                }
            }
            crate::tui::layout::SavedSplit::Vertical { top, bottom, ratio } => {
                SplitNode::Vertical {
                    top: Box::new(self.restore_split(top)),
                    bottom: Box::new(self.restore_split(bottom)),
                    ratio: *ratio,
                }
            }
        }
    }

    pub fn restore_layout(&mut self, layout: &crate::tui::layout::SavedLayout) {
        if !layout.is_valid() {
            return;
        }

        self.tabs.clear();
        for saved_tab in &layout.tabs {
            let root = self.restore_split(&saved_tab.root);
            self.tabs
                .push(crate::tui::tab::Tab::new(saved_tab.name.clone(), root));
        }
        if layout.active_tab < self.tabs.len() {
            self.active_tab = layout.active_tab;
        } else if !self.tabs.is_empty() {
            self.active_tab = 0;
        }
        if let Some(&first) = self.root().window_ids().first() {
            self.focused_window = Some(first);
        }
    }

    // ── Tick ─────────────────────────────────────────────────────────

    pub fn tick(&mut self) {
        if self.keymap_timeout > 0 {
            self.keymap_timeout -= 1;
            if self.keymap_timeout == 0 && self.keymap_state == KeymapState::AwaitWindow {
                self.keymap_state = KeymapState::Normal;
            }
        }
        self.root_mut().tick();
        let actions = collect_actions(self.root_mut());
        for action in actions {
            self.apply_action(action);
        }
        self.status_bar.gas_remaining = self.bridges.system_bridge.gas_remaining();
        self.status_bar.gas_cap = self.bridges.system_bridge.gas_cap();
        let alerts = self.bridges.system_bridge.reg_alert_count();
        self.status_bar.reg_status = if alerts >= 5 {
            crate::tui::status_bar::RegStatus::Critical(alerts)
        } else if alerts > 0 {
            crate::tui::status_bar::RegStatus::Warning(alerts)
        } else {
            crate::tui::status_bar::RegStatus::Healthy
        };
        self.status_bar.context_pressure = self.bridges.system_bridge.context_pressure();
        self.status_bar.model = self.bridges.system_bridge.model_name().to_string();
    }
}

/// Recursively collect `WorkspaceAction`s from all windows in the tree.
fn collect_actions(node: &mut SplitNode) -> Vec<WorkspaceAction> {
    let mut actions = Vec::new();
    match node {
        SplitNode::Leaf(w) => {
            actions.extend(w.drain_actions());
        }
        SplitNode::Horizontal { left, right, .. } => {
            actions.extend(collect_actions(left));
            actions.extend(collect_actions(right));
        }
        SplitNode::Vertical { top, bottom, .. } => {
            actions.extend(collect_actions(top));
            actions.extend(collect_actions(bottom));
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_util::mock_bridges;

    fn make_workspace() -> Workspace {
        let (system, repl) = mock_bridges();
        Workspace::new_test(system, repl)
    }

    #[test]
    fn workspace_starts_with_single_chat_window() {
        let ws = make_workspace();
        assert_eq!(ws.window_count(), 1);
        assert_eq!(ws.tab_count(), 1);
        assert!(ws.focused_window().is_some());
    }

    #[test]
    fn open_window_creates_split() {
        let mut ws = make_workspace();
        assert_eq!(ws.window_count(), 1);
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        assert_eq!(ws.window_count(), 2);
        // New window should be focused
        let focused = ws.focused_window().unwrap();
        assert_eq!(ws.root().window_kind(focused), Some(WindowKind::Kanban));
    }

    #[test]
    fn open_singleton_refocuses_existing() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        assert_eq!(ws.window_count(), 2);
        let kanban_id = ws.focused_window().unwrap();

        // Focus the chat window
        ws.apply_action(WorkspaceAction::FocusNext);
        assert_ne!(ws.focused_window(), Some(kanban_id));

        // Opening Kanban again should refocus, not create a new one
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        assert_eq!(ws.window_count(), 2);
        assert_eq!(ws.focused_window(), Some(kanban_id));
    }

    #[test]
    fn split_focused_doubles_window() {
        let mut ws = make_workspace();
        assert_eq!(ws.window_count(), 1);
        ws.apply_action(WorkspaceAction::Split(SplitDirection::Vertical));
        assert_eq!(ws.window_count(), 2);
    }

    #[test]
    fn close_focused_collapses_split() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        assert_eq!(ws.window_count(), 2);
        ws.apply_action(WorkspaceAction::CloseFocused);
        assert_eq!(ws.window_count(), 1);
    }

    #[test]
    fn close_last_window_replaces_with_chat() {
        let mut ws = make_workspace();
        assert_eq!(ws.window_count(), 1);
        ws.apply_action(WorkspaceAction::CloseFocused);
        assert_eq!(ws.window_count(), 1);
        assert!(ws.focused_window().is_some());
    }

    #[test]
    fn new_tab_and_switch() {
        let mut ws = make_workspace();
        assert_eq!(ws.tab_count(), 1);
        ws.apply_action(WorkspaceAction::NewTab(None));
        assert_eq!(ws.tab_count(), 2);
        assert_eq!(ws.active_tab_index(), 1);
        ws.apply_action(WorkspaceAction::PrevTab);
        assert_eq!(ws.active_tab_index(), 0);
        ws.apply_action(WorkspaceAction::NextTab);
        assert_eq!(ws.active_tab_index(), 1);
    }

    #[test]
    fn focus_next_cycles() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        let first = ws.focused_window().unwrap();
        ws.apply_action(WorkspaceAction::FocusNext);
        assert_ne!(ws.focused_window(), Some(first));
        ws.apply_action(WorkspaceAction::FocusNext);
        assert_eq!(ws.focused_window(), Some(first));
    }

    #[test]
    fn open_multiple_windows_and_close() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Companies));
        assert_eq!(ws.window_count(), 3);

        // Close the focused one (Companies)
        ws.apply_action(WorkspaceAction::CloseFocused);
        assert_eq!(ws.window_count(), 2);
    }

    #[test]
    fn extract_layout_contains_new_kinds() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Scenarios));
        let layout = ws.extract_layout();
        assert!(layout.is_valid());
        // Should have 2 windows in a vertical split
        assert_eq!(layout.tabs.len(), 1);
        // Verify the split contains Chat and Scenarios
        let tab = &layout.tabs[0];
        match &tab.root {
            crate::tui::layout::SavedSplit::Vertical { top, bottom, .. } => {
                if let crate::tui::layout::SavedSplit::Leaf(ref leaf) = **top {
                    assert_eq!(leaf.kind, "Chat");
                } else {
                    panic!("expected Chat leaf");
                }
                if let crate::tui::layout::SavedSplit::Leaf(ref leaf) = **bottom {
                    assert_eq!(leaf.kind, "Scenarios");
                } else {
                    panic!("expected Scenarios leaf");
                }
            }
            other => panic!("expected vertical split, got {:?}", other),
        }
    }

    #[test]
    fn ctrl_w_prefix_mode_consumes_next_key() {
        let mut ws = make_workspace();
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Ctrl-W enters prefix mode
        let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(ws.handle_global_key(ctrl_w));

        // 'v' triggers vertical split
        let v_key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        assert!(ws.handle_global_key(v_key));
        assert_eq!(ws.window_count(), 2);
    }

    #[test]
    fn ctrl_t_creates_new_tab() {
        let mut ws = make_workspace();
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert_eq!(ws.tab_count(), 1);
        let ctrl_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(ws.handle_global_key(ctrl_t));
        assert_eq!(ws.tab_count(), 2);
    }

    #[test]
    fn keymap_timeout_resets_await_window() {
        let mut ws = make_workspace();
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Ctrl-W enters prefix mode
        let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(ws.handle_global_key(ctrl_w));

        // Tick 61 times (timeout is 60) — should auto-reset to Normal
        for _ in 0..61 {
            ws.tick();
        }

        // Now 'v' should NOT trigger a split (prefix mode was reset)
        let v_key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        ws.handle_global_key(v_key);
        assert_eq!(
            ws.window_count(),
            1,
            "split should not have occurred after timeout"
        );
    }

    #[test]
    fn focus_prev_cycles_backward() {
        let mut ws = make_workspace();
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        let kanban_id = ws.focused_window().unwrap();

        // Cycle forward to Chat
        ws.apply_action(WorkspaceAction::FocusNext);
        assert_ne!(ws.focused_window(), Some(kanban_id));

        // Cycle backward should return to Kanban
        ws.apply_action(WorkspaceAction::FocusPrev);
        assert_eq!(ws.focused_window(), Some(kanban_id));
    }

    #[test]
    fn focus_window_rejects_cross_tab_target() {
        let mut ws = make_workspace();

        // Create a second tab with a Kanban window
        ws.apply_action(WorkspaceAction::NewTab(None));
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        let kanban_id = ws.focused_window().unwrap();

        // Switch back to tab 0 (which has only Chat)
        ws.apply_action(WorkspaceAction::PrevTab);
        assert_eq!(ws.active_tab_index(), 0);
        let chat_id = ws.focused_window().unwrap();

        // Try to focus the Kanban window from tab 1 — should be rejected
        ws.focus_window(kanban_id);
        assert_eq!(
            ws.focused_window(),
            Some(chat_id),
            "focus should NOT change to a window in another tab"
        );
    }

    #[test]
    fn close_focused_rejects_cross_tab_target() {
        let mut ws = make_workspace();

        // Create a second tab with Kanban
        ws.apply_action(WorkspaceAction::NewTab(None));
        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        let kanban_id = ws.focused_window().unwrap();
        assert_eq!(ws.window_count(), 2); // Chat + Kanban in tab 1

        // Switch back to tab 0
        ws.apply_action(WorkspaceAction::PrevTab);
        assert_eq!(ws.active_tab_index(), 0);

        // Stash the kanban_id into focused_window (simulating stale state)
        ws.focused_window = Some(kanban_id);

        // close_focused should NOT close anything (kanban_id is not in tab 0)
        ws.apply_action(WorkspaceAction::CloseFocused);
        assert_eq!(ws.window_count(), 1, "tab 0 should still have 1 window");
    }

    #[test]
    fn ctrl_w_p_cycles_focus_prev() {
        let mut ws = make_workspace();
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        ws.apply_action(WorkspaceAction::OpenWindow(WindowKind::Kanban));
        let kanban_id = ws.focused_window().unwrap();

        // Cycle forward to Chat
        ws.apply_action(WorkspaceAction::FocusNext);

        // Ctrl-W p should cycle back to Kanban
        let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(ws.handle_global_key(ctrl_w));
        let p_key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        assert!(ws.handle_global_key(p_key));
        assert_eq!(ws.focused_window(), Some(kanban_id));
    }
}
