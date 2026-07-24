//! Layout persistence — save and restore TUI workspace layouts.
//!
//! Saves the split tree, window kinds, tab names, and active tab
//! to a JSON file per-agent. Loaded on TUI startup, saved on quit.
//! Associated with the userpod identity for privacy (P1).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::tui::window::WindowKind;
use crate::tui::window_catalog::window_kind_from_title;

/// Serializable representation of a window layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedLayout {
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub name: String,
    pub root: SavedSplit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SavedSplit {
    Leaf(SavedLeaf),
    Horizontal {
        left: Box<SavedSplit>,
        right: Box<SavedSplit>,
        ratio: f32,
    },
    Vertical {
        top: Box<SavedSplit>,
        bottom: Box<SavedSplit>,
        ratio: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedLeaf {
    /// Serialized WindowKind variant name (e.g., "Chat", "Kanban")
    pub kind: String,
}

impl SavedLayout {
    pub(crate) fn is_valid(&self) -> bool {
        self.version == 1
            && !self.tabs.is_empty()
            && self.active_tab < self.tabs.len()
            && self.tabs.iter().all(|tab| tab.root.is_valid())
    }
}

impl SavedSplit {
    fn is_valid(&self) -> bool {
        match self {
            Self::Leaf(leaf) => window_kind_from_title(&leaf.kind).is_some(),
            Self::Horizontal { left, right, ratio }
            | Self::Vertical {
                top: left,
                bottom: right,
                ratio,
            } => {
                ratio.is_finite()
                    && (0.1..=0.9).contains(ratio)
                    && left.is_valid()
                    && right.is_valid()
            }
        }
    }
}

/// Convert a WindowKind to its serialized string.
pub fn kind_to_string(kind: WindowKind) -> String {
    kind.default_title().to_string()
}

/// Parse a serialized kind string back to WindowKind.
/// Returns Chat as a safe fallback for unknown kinds.
pub fn string_to_kind(s: &str) -> WindowKind {
    window_kind_from_title(s).unwrap_or(WindowKind::Chat)
}

/// Path to the layout file for a given userpod.
pub fn layout_path(userpod_name: &str) -> PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hkask")
        .join(hkask_types::agent_paths::USERPODS_DIR)
        .join(sanitize(userpod_name));
    base.join("tui_layout.json")
}

/// Sanitize a userpod name for use in a directory path.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Load a saved layout from disk. Returns None if no saved layout exists.
pub fn load(path: &PathBuf) -> Option<SavedLayout> {
    let data = fs::read_to_string(path).ok()?;
    let layout: SavedLayout = serde_json::from_str(&data).ok()?;
    layout.is_valid().then_some(layout)
}

/// Save a layout to disk.
pub fn save(path: &PathBuf, layout: &SavedLayout) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(layout)?;
    fs::write(path, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_layout() -> SavedLayout {
        SavedLayout {
            version: 1,
            tabs: vec![SavedTab {
                name: "Chat".into(),
                root: SavedSplit::Leaf(SavedLeaf {
                    kind: "Chat".into(),
                }),
            }],
            active_tab: 0,
        }
    }

    #[test]
    fn rejects_layout_without_tabs() {
        let mut layout = valid_layout();
        layout.tabs.clear();
        assert!(!layout.is_valid());
    }

    #[test]
    fn rejects_out_of_bounds_active_tab() {
        let mut layout = valid_layout();
        layout.active_tab = 1;
        assert!(!layout.is_valid());
    }

    #[test]
    fn rejects_unknown_window_kind() {
        let mut layout = valid_layout();
        layout.tabs[0].root = SavedSplit::Leaf(SavedLeaf {
            kind: "Removed Window".into(),
        });
        assert!(!layout.is_valid());
    }

    #[test]
    fn rejects_invalid_split_ratio() {
        let leaf = || {
            Box::new(SavedSplit::Leaf(SavedLeaf {
                kind: "Chat".into(),
            }))
        };
        let mut layout = valid_layout();
        layout.tabs[0].root = SavedSplit::Horizontal {
            left: leaf(),
            right: leaf(),
            ratio: f32::NAN,
        };
        assert!(!layout.is_valid());
    }

    #[test]
    fn validates_layout_with_new_window_kinds() {
        let layout = SavedLayout {
            version: 1,
            tabs: vec![SavedTab {
                name: "Multi".into(),
                root: SavedSplit::Vertical {
                    top: Box::new(SavedSplit::Leaf(SavedLeaf {
                        kind: "Chat".into(),
                    })),
                    bottom: Box::new(SavedSplit::Horizontal {
                        left: Box::new(SavedSplit::Leaf(SavedLeaf {
                            kind: "Kanban".into(),
                        })),
                        right: Box::new(SavedSplit::Leaf(SavedLeaf {
                            kind: "Companies".into(),
                        })),
                        ratio: 0.5,
                    }),
                    ratio: 0.6,
                },
            }],
            active_tab: 0,
        };
        assert!(layout.is_valid());
    }

    #[test]
    fn validates_layout_with_scenarios_kind() {
        let layout = SavedLayout {
            version: 1,
            tabs: vec![SavedTab {
                name: "Scenarios".into(),
                root: SavedSplit::Leaf(SavedLeaf {
                    kind: "Scenarios".into(),
                }),
            }],
            active_tab: 0,
        };
        assert!(layout.is_valid());
    }

    #[test]
    fn string_to_kind_falls_back_to_chat() {
        assert_eq!(string_to_kind("nonexistent"), WindowKind::Chat);
        assert_eq!(string_to_kind("Kanban"), WindowKind::Kanban);
        assert_eq!(string_to_kind("companies"), WindowKind::Companies);
    }
}
