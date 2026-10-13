//! Service-layer foundation, folded in from the deleted `hkask-services-core`
//! crate (2026-10-13): the canonical `ServiceError` taxonomy and the
//! standalone `HkaskSettings` reader. This server is the sole consumer — the
//! shared crate had exactly one dependent, so it failed the deletion test and
//! was folded rather than kept as a one-consumer abstraction.

pub(crate) mod error;
pub(crate) mod standalone_settings;

pub(crate) use error::{DomainKind, ErrorKind, ServiceError};
pub(crate) use standalone_settings::HkaskSettings;
