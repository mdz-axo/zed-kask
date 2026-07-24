#![forbid(unsafe_code)]
//! hKask Onboarding Service — secret derivation, keychain, A2A registration, sign-in.
//!
//! Matrix account registration during onboarding has been removed (Matrix is
//! deferred out of the onboarding flow). Only the localpart derivation helper
//! remains for display purposes.

mod onboarding_impl;
pub use onboarding_impl::matrix::derive_matrix_localparts;
pub use onboarding_impl::{OnboardingService, ResolvedSecrets, SignInOutcome};
