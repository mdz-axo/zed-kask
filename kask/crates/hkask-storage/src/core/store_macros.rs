//! Declarative macros for store boilerplate elimination.
//!
//! `define_driver_store!` generates the struct, `from_driver` constructor,
//! and `driver()` accessor. Each store MUST provide an `init_schema` method
//! that the constructor calls — this ensures idempotent schema initialization
//! for every store without burdening callers.

/// Define a store struct backed by a `DatabaseDriver`.
///
/// Generates `from_driver(driver)` which calls `Self::init_schema(driver)`.
/// The store MUST implement `fn init_schema(driver: &Arc<dyn DatabaseDriver>)`
/// in a separate `impl` block. For stores with no tables, provide an empty body.
///
/// # Example
/// ```ignore
/// define_driver_store!(UserStore);
///
/// impl UserStore {
///     fn init_schema(driver: &Arc<dyn DatabaseDriverTrait>) {
///         driver.execute_batch("CREATE TABLE IF NOT EXISTS users (...);").ok();
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_driver_store {
    ($name:ident) => {
        /// Store backed by a provider-agnostic DatabaseDriver.
        #[derive(Clone)]
        pub struct $name {
            driver: std::sync::Arc<dyn $crate::DatabaseDriverTrait>,
        }
        impl $name {
            /// Create a new store backed by the given driver.
            /// Calls `Self::init_schema(driver)` for idempotent schema setup.
            pub fn from_driver(driver: std::sync::Arc<dyn $crate::DatabaseDriverTrait>) -> Self {
                $name::init_schema(&driver);
                Self { driver }
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
