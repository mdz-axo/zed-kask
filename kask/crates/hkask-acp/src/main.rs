//! hkask-acp binary entry point.
//!
//! The library crate re-exports the public API; this binary just calls main().

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

use hkask_acp::main_impl;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Ok(main_impl::run().await?)
}
