#![deny(unsafe_code)]
// FFI boundary crate: the generated bindings carry the raw externs, so
// unsafe is PRODUCTION here — the project gate's accepted pattern
// (deny at the root, scoped allow over the generated module only).
//! See [llama-cpp-4](https://crates.io/crates/llama-cpp-4) for a documented and safe API.
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

#[allow(
    unsafe_code,
    unnecessary_transmutes,
    clippy::missing_safety_doc,
    clippy::ptr_offset_with_cast
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::*;

pub mod common;
