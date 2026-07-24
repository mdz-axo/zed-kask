#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Wallet Service — gas budgeting, price feeds, and Regulation integration.
//!
//! Extracted from `hkask-services`.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]

mod wallet_impl;
pub use wallet_impl::WalletService;
