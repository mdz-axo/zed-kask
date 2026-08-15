//! Re-export of the kanban mermaid module at the crate root.
//!
/// The MCP tool layer (`kanban_board_export`, `kanban_board_import`) references
/// `crate::mermaid::*`; the service-layer tests reference
/// `crate::kanban::mermaid::*`. Both paths resolve to the same implementation
/// in [`crate::kanban::mermaid`].
pub use crate::kanban::mermaid::*;
