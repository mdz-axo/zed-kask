//! Gallery module — image management, analysis, and composition.
//!
//! Tool families:
//! - State: init, scan, info
//! - Vision: detect_objects, detect_faces, caption, tag, classify
//! - Creation: collage, derivative
//! - Search: semantic search

pub mod state;
pub mod vision;

pub use state::GalleryState;
