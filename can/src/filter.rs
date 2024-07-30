use std::cmp::Ordering;

use crate::Id;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Filter {
    pub id: Id,
    pub mask: u32,
}

impl Ord for Filter {
    /// The ordinality of a filter is determined by first the ordinality of the Id, and then
    /// tiebroken by the filter which _guarantees the earliest_ dominance.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cmp::Ordering;
    /// use can_rs::filter::Filter;
    /// use can_rs::Id::Standard;
    ///
    /// let filter_a = Filter {
    ///     id: Standard(0x02),
    ///     mask: 0xFF,
    /// };
    /// let filter_b = Filter {
    ///     id: Standard(0x02),
    ///     mask: 0x0F,
    /// };
    ///
    /// assert_eq!(Ordering::Greater, filter_b.cmp(&filter_a));
    /// ```
    ///
    /// In this example, the ordinality of the IDs is a wash as they are equivalent. When we look
    /// to the masks, we see that `filter_b.mask` is more restrictive _earlier_.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.id.cmp(&other.id) {
            Ordering::Equal => self.mask.cmp(&other.mask).reverse(),
            ord => ord,
        }
    }
}

impl PartialOrd for Filter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct RawFilter {
    pub(crate) id: u32,
    pub(crate) mask: u32,
}

impl RawFilter {
    pub(crate) fn empty() -> Self {
        RawFilter { id: 0, mask: 0 }
    }
}

impl From<Filter> for RawFilter {
    fn from(filter: Filter) -> Self {
        Self {
            id: filter.id.wire_value(),
            mask: filter.mask,
        }
    }
}

impl From<RawFilter> for Filter {
    fn from(filter: RawFilter) -> Self {
        Self {
            id: Id::from(filter.id),
            mask: filter.mask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_basic_filter_ordinality() {
        let greatest = Filter {
            id: Id::Standard(0x02),
            mask: 0x0000,
        };

        let greater = Filter {
            id: Id::Standard(0x02),
            mask: 0x00FF,
        };

        let lesser = Filter {
            id: Id::Standard(0x02),
            mask: 0xFF00,
        };

        assert_eq!(Ordering::Greater, greatest.cmp(&greater));
        assert_eq!(Ordering::Greater, greatest.cmp(&lesser));
        assert_eq!(Ordering::Greater, greater.cmp(&lesser));
    }

    #[test]
    fn compare_mixed_filter_ordinality() {
        let extended_and_lesser = Filter {
            id: Id::Extended(0x02),
            mask: 0x0000,
        };

        let standard_and_greater = Filter {
            id: Id::Standard(0x02),
            mask: 0x0001,
        };

        assert_eq!(
            Ordering::Greater,
            standard_and_greater.cmp(&extended_and_lesser)
        );
    }
}
