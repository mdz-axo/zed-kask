//! Gallery module — active-gallery state and the vision-LLM analysis
//! helpers (face validation/matching, tagging, captioning) shared by the
//! `gallery_*` and `face_*` tools.

pub mod state;
pub mod vision;

pub use state::GalleryState;
