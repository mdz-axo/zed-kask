//! Checked satoshi and millisatoshi newtypes (Threat T7).
//!
//! Raw `u64` arithmetic can silently overflow or truncate sub-satoshi amounts.
//! All wallet balance, fee, and cap calculations use these newtypes with
//! checked operations.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Millisatoshis per satoshi.
pub const MSAT_PER_SAT: u64 = 1_000;

/// Maximum supply of Bitcoin in satoshis (21,000,000 BTC).
pub const MAX_BITCOIN_SATS: u64 = 2_100_000_000_000_000;

/// Errors arising from checked amount arithmetic or unit conversion.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AmountError {
    #[error("satoshi/millisatoshi arithmetic overflow")]
    Overflow,
    #[error("satoshi/millisatoshi arithmetic underflow")]
    Underflow,
    #[error("millisatoshi amount is not an exact whole number of satoshis")]
    NonExactSubSatoshi,
}

/// An amount denominated in whole satoshis.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Satoshis(u64);

impl Satoshis {
    pub const ZERO: Self = Self(0);

    pub const fn new(sats: u64) -> Self {
        Self(sats)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(AmountError::Underflow)
    }

    pub fn checked_mul(self, factor: u64) -> Result<Self, AmountError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    pub fn checked_to_msat(self) -> Result<Millisatoshis, AmountError> {
        self.0
            .checked_mul(MSAT_PER_SAT)
            .map(Millisatoshis)
            .ok_or(AmountError::Overflow)
    }
}

impl fmt::Display for Satoshis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} sats", self.0)
    }
}

/// An amount denominated in millisatoshis.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Millisatoshis(u64);

impl Millisatoshis {
    pub const ZERO: Self = Self(0);

    pub const fn new(msat: u64) -> Self {
        Self(msat)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(AmountError::Underflow)
    }

    pub fn checked_mul(self, factor: u64) -> Result<Self, AmountError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    /// Convert to whole satoshis, rejecting any non-zero sub-satoshi remainder.
    pub fn to_satoshis_exact(self) -> Result<Satoshis, AmountError> {
        if !self.0.is_multiple_of(MSAT_PER_SAT) {
            return Err(AmountError::NonExactSubSatoshi);
        }
        Ok(Satoshis(self.0 / MSAT_PER_SAT))
    }

    /// Convert to whole satoshis rounding up conservatively (used for caps and fees).
    pub fn to_satoshis_ceil(self) -> Result<Satoshis, AmountError> {
        let bumped = self
            .0
            .checked_add(MSAT_PER_SAT - 1)
            .ok_or(AmountError::Overflow)?;
        Ok(Satoshis(bumped / MSAT_PER_SAT))
    }
}

impl fmt::Display for Millisatoshis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} msat", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn satoshis_checked_ops_catch_overflow_and_underflow() {
        assert_eq!(
            Satoshis::new(u64::MAX).checked_add(Satoshis::new(1)),
            Err(AmountError::Overflow)
        );
        assert_eq!(
            Satoshis::new(10).checked_sub(Satoshis::new(11)),
            Err(AmountError::Underflow)
        );
        assert_eq!(
            Satoshis::new(u64::MAX).checked_mul(2),
            Err(AmountError::Overflow)
        );
        assert_eq!(
            Satoshis::new(u64::MAX).checked_to_msat(),
            Err(AmountError::Overflow)
        );
    }

    #[test]
    fn millisatoshis_exact_and_ceil_conversions_handle_sub_satoshis() {
        assert_eq!(
            Millisatoshis::new(5_000).to_satoshis_exact(),
            Ok(Satoshis::new(5))
        );
        assert_eq!(
            Millisatoshis::new(5_001).to_satoshis_exact(),
            Err(AmountError::NonExactSubSatoshi)
        );
        assert_eq!(
            Millisatoshis::new(5_001).to_satoshis_ceil(),
            Ok(Satoshis::new(6))
        );
        assert_eq!(Millisatoshis::new(0).to_satoshis_ceil(), Ok(Satoshis::ZERO));
        assert_eq!(
            Millisatoshis::new(u64::MAX).to_satoshis_ceil(),
            Err(AmountError::Overflow)
        );
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn satoshis_checked_add_sub_never_panics_and_round_trips(a in any::<u64>(), b in any::<u64>()) {
                let sa = Satoshis::new(a);
                let sb = Satoshis::new(b);
                match sa.checked_add(sb) {
                    Ok(sum) => {
                        prop_assert_eq!(sum.checked_sub(sb), Ok(sa));
                        prop_assert!(sum >= sa && sum >= sb);
                    }
                    Err(AmountError::Overflow) => {
                        prop_assert!(a.checked_add(b).is_none());
                    }
                    Err(other) => prop_assert!(false, "unexpected error: {other:?}"),
                }
            }

            #[test]
            fn msat_exact_and_ceil_conversions_are_consistent(msat in 0u64..=(u64::MAX - MSAT_PER_SAT)) {
                let m = Millisatoshis::new(msat);
                let ceil = m.to_satoshis_ceil().expect("bounded msat never overflows ceil");
                if msat % MSAT_PER_SAT == 0 {
                    let exact = m.to_satoshis_exact().expect("exact multiple");
                    prop_assert_eq!(exact, ceil);
                    prop_assert_eq!(exact.checked_to_msat(), Ok(m));
                } else {
                    prop_assert_eq!(m.to_satoshis_exact(), Err(AmountError::NonExactSubSatoshi));
                    prop_assert_eq!(ceil.as_u64(), (msat / MSAT_PER_SAT) + 1);
                }
            }
        }
    }
}
