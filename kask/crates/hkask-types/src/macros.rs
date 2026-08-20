//! Shared macros for the hKask type system.
//!
//! Centralized here to eliminate the 4× duplication of `enum_str_ops!`
//! across `hkask-types` and `hkask-storage`.

/// Generates `as_str()` and `parse_str()` for a PascalCase enum.
///
/// `as_str()` returns the PascalCase variant name as `&'static str`.
/// `parse_str()` accepts both PascalCase and snake_case strings.
///
/// # Example
///
/// ```ignore
/// enum_str_ops!(MyEnum, {
///     Foo => ("Foo", "foo"),
///     Bar => ("Bar", "bar"),
/// });
/// ```
#[macro_export]
macro_rules! enum_str_ops {
    ($ty:ident, { $($variant:ident => ($pascal:literal, $snake:literal)),+ $(,)? }) => {
        impl $ty {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($ty::$variant => $pascal),+
                }
            }
            pub fn parse_str(s: &str) -> Option<Self> {
                match s {
                    $($pascal | $snake => Some($ty::$variant)),+,
                    _ => None,
                }
            }
        }
    };
}

/// Generates `as_str()` and `FromStr` for an enum with custom string representations.
///
/// Unlike `enum_str_ops!`, this macro returns the specified string from `as_str()`
/// (not necessarily PascalCase), implements `FromStr` (not `parse_str`), and trims
/// input before matching for env-var robustness.
///
/// # Example
///
/// ```ignore
/// enum_snake_str!(SampleMode, {
///     BestOfN => "best-of-n",
///     Synthesis => "synthesis",
/// });
/// ```
#[macro_export]
macro_rules! enum_snake_str {
    ($ty:ident, { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $ty {
            #[must_use]
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($ty::$variant => $s),+
                }
            }
        }

        impl std::str::FromStr for $ty {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.trim() {
                    $($s => Ok($ty::$variant)),+,
                    _ => Err(()),
                }
            }
        }
    };
}
