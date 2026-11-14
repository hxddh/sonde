//! [`Size`] and [`Millis`]: a count of bytes and a time that may be unknown, each in 8 bytes.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

/// A count of bytes, or none when it is not known: what `Option<u64>` says,
/// in 8 bytes instead of 16 (the count is kept one more, so that none is
/// zero; the largest count, never seen, reads one less).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Size(Option<NonZeroU64>);

impl Size {
    pub const NONE: Size = Size(None);

    pub fn get(self) -> Option<u64> {
        self.0.map(|n| n.get() - 1)
    }
}

impl From<Option<u64>> for Size {
    fn from(n: Option<u64>) -> Self {
        Size(n.and_then(|n| NonZeroU64::new(n.saturating_add(1))))
    }
}

impl From<u64> for Size {
    fn from(n: u64) -> Self {
        Size::from(Some(n))
    }
}

impl PartialEq<Option<u64>> for Size {
    fn eq(&self, other: &Option<u64>) -> bool {
        self.get() == *other
    }
}

impl std::fmt::Debug for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

impl Serialize for Size {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Option::<u64>::deserialize(d).map(Size::from)
    }
}

/// A time in milliseconds, or none when it is not known: what
/// `Option<f64>` says, in 8 bytes instead of 16 (none is held as NaN, which
/// no time is).
#[derive(Clone, Copy)]
pub struct Millis(f64);

impl Millis {
    pub const NONE: Millis = Millis(f64::NAN);

    pub fn get(self) -> Option<f64> {
        (!self.0.is_nan()).then_some(self.0)
    }
}

impl Default for Millis {
    fn default() -> Self {
        Millis::NONE
    }
}

impl From<Option<f64>> for Millis {
    fn from(ms: Option<f64>) -> Self {
        Millis(ms.unwrap_or(f64::NAN))
    }
}

impl From<f64> for Millis {
    fn from(ms: f64) -> Self {
        Millis(ms)
    }
}

impl PartialEq for Millis {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

impl PartialEq<Option<f64>> for Millis {
    fn eq(&self, other: &Option<f64>) -> bool {
        self.get() == *other
    }
}

impl std::fmt::Debug for Millis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

impl Serialize for Millis {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Millis {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Option::<f64>::deserialize(d).map(Millis::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_or_a_time_that_may_be_unknown_is_8_bytes() {
        assert_eq!(std::mem::size_of::<Size>(), 8);
        assert_eq!(std::mem::size_of::<Millis>(), 8);
        for n in [None, Some(0), Some(7), Some(u64::MAX - 1)] {
            assert_eq!(Size::from(n).get(), n);
            assert_eq!(
                serde_json::to_string(&Size::from(n)).unwrap(),
                serde_json::to_string(&n).unwrap()
            );
        }
        for ms in [None, Some(0.0), Some(12.345)] {
            assert_eq!(Millis::from(ms).get(), ms);
        }
        assert_eq!(Size::default(), None);
        assert_eq!(Millis::default(), None);
        let m: Millis = serde_json::from_str("null").unwrap();
        assert_eq!(m, None);
    }
}
