#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask CLI — Command-line interface

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

/// Block on a future using a tokio Runtime, exiting the process on failure.
///
/// # Panics
/// Exits the process with code 1 if the future returns an error.
#[macro_export]
macro_rules! block_on {
    ($rt:expr, $future:expr, $msg:expr $(,)?) => {
        match $rt.block_on($future) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}: {}", $msg, e);
                std::process::exit(1);
            }
        }
    };
}

pub mod archival;
pub mod cli;
pub mod cloud;
pub mod commands;
pub mod error;
pub mod experience;
pub mod onboarding;
pub mod onboarding_session;
pub mod repl_host;
#[cfg(feature = "tui")]
pub mod transcript_viewer;
pub mod verification;
