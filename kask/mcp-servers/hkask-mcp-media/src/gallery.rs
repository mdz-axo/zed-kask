//! Gallery module — the SQLite-backed gallery store, active-gallery state,
//! and the vision-LLM analysis helpers (face validation/matching, tagging,
//! captioning) shared by the `gallery_*` and `face_*` tools. The store moved
//! here from hkask-storage; this crate is its sole consumer.

pub mod state;
pub mod store;
pub mod vision;

pub use state::GalleryState;
pub use store::{
    AssetCreationPublication, AssetObservation, FaceRegistryRecord, GalleryMode, GalleryScan,
    GalleryStore, GalleryStoreError, ImageRecord, ReconcileResult, TagRecord,
};
