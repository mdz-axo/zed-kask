//! Declarative macros for store boilerplate elimination.
//!
//! `define_driver_store!` generates the struct, `from_driver` constructor,
//! and `driver()` accessor. Each store MUST provide an `init_schema` method
//! that the constructor calls — this ensures idempotent schema initialization
//! for every store without burdening callers.

/// Define a store struct backed by a `DatabaseDriver`.
///
/// Generates `from_driver(driver)` which calls `Self::init_schema(driver)` and
/// propagates any schema-init failure. The store MUST implement
/// `fn init_schema(driver: &Arc<dyn DatabaseDriver>) -> Result<(), E>`
/// in a separate `impl` block, where `E` is the error type passed to the macro
/// (defaulting to `InfrastructureError`). For stores with no tables, return `Ok(())`.
///
/// The optional second argument customizes the error type returned by
/// `from_driver` and expected from `init_schema`. Stores whose domain errors are
/// distinct from `InfrastructureError` (e.g. `Ledger` with `LedgerError`) pass
/// their error type so the macro-generated `from_driver` returns it directly
/// instead of forcing an `InfrastructureError` boundary.
///
/// # Example
/// ```ignore
/// // Default error type (InfrastructureError):
/// define_driver_store!(UserStore);
///
/// impl UserStore {
///     fn init_schema(driver: &Arc<dyn DatabaseDriverTrait>) -> Result<(), hkask_types::InfrastructureError> {
///         driver.execute_batch("CREATE TABLE IF NOT EXISTS users (...);")?;
///         Ok(())
///     }
/// }
///
/// // Custom error type:
/// define_driver_store!(Ledger, LedgerError);
///
/// impl Ledger {
///     fn init_schema(driver: &Arc<dyn DatabaseDriverTrait>) -> Result<(), LedgerError> {
///         schema::init_schema(driver)
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_driver_store {
    ($name:ident) => {
        $crate::define_driver_store!($name, hkask_types::InfrastructureError);
    };
    ($name:ident, $error:ty) => {
        /// Store backed by a provider-agnostic DatabaseDriver.
        #[derive(Clone)]
        pub struct $name {
            driver: std::sync::Arc<dyn $crate::DatabaseDriverTrait>,
        }
        impl $name {
            /// Create a new store backed by the given driver.
            /// Calls `Self::init_schema(driver)` for idempotent schema setup
            /// and propagates any schema-init failure rather than proceeding
            /// with a missing table.
            pub fn from_driver(
                driver: std::sync::Arc<dyn $crate::DatabaseDriverTrait>,
            ) -> Result<Self, $error> {
                $name::init_schema(&driver)?;
                Ok(Self { driver })
            }
            /// Access the underlying driver for direct queries.
            pub fn driver(&self) -> &std::sync::Arc<dyn $crate::DatabaseDriverTrait> {
                &self.driver
            }
        }
    };
}

/// Re-export for macro hygiene — the macro references this path.
pub use crate::database::driver::DatabaseDriver as DatabaseDriverTrait;

/// Implement `From<DbError>` for a store error type, mapping to
/// `XxxError::Infra(InfrastructureError::from(e))`.
#[macro_export]
macro_rules! impl_from_db_error {
    ($error:ident, $infra_variant:ident) => {
        impl From<super::database::types::DbError> for $error {
            fn from(e: super::database::types::DbError) -> Self {
                $error::$infra_variant(hkask_types::InfrastructureError::from(e))
            }
        }
    };
}
