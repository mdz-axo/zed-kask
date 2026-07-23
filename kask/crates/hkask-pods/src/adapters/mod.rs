//! Adapter implementations for hexagonal ports

pub mod memory_loop_adapter;

pub mod registry_source;

pub use memory_loop_adapter::MemoryLoopForwarder;
pub use registry_source::FilesystemRegistrySource;
