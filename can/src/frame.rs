use std::cmp::Ordering;

pub const CANFD_FDF_FLAG: u8 = 0x04;

#[derive(Copy, Clone, Debug, PartialEq)]

pub struct Frame<const N: usize> {
    pub id: Id,
    pub len: u8,
    pub flags: u8,
    pub data: [u8; N],
}

impl<const N: usize> Frame<N> {
    pub fn empty() -> Self {
        Self {
            id: Id::Standard(0),
            len: 0,
            flags: 0,
            data: [0u8; N],
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Id {
    Standard(u32),
    Extended(u32),
}

impl Ord for Id {
    /// Ordinality of CAN IDs is determined by what would take precedence on the wire during
    /// arbitration.
    ///
    /// Arbitration on the bus occurs when two devices start sending at the exact same time. At
    /// that point, each transmitting node must keep track of the bit it just sent vs. the bit it
    /// monitored on the bus. If the transmitted is equal to the monitored bit, then transmission
    /// from this node can continue. If the transmitted bit is recessive and the monitored bit is
    /// dominant, then this node has lost the arbitration and must wait to attempt to retransmit.
    ///
    /// Example (`0` is dominant, `1` is recessive):
    /// ```text
    /// Bit #   NODE a  NODE b
    /// 1       1       1
    /// 2       1       1
    /// 3       0       1
    /// 4       1       -
    /// ---
    /// Result: NODE a continues transmission as it didn't detect NODE b.
    ///              NODE b detect that it is not alone transmitting, and stops the transmission.
    /// ```
    /// Here, the transmission between the nodes was equal until `NODE a` sent a dominant 3rd bit
    /// while `NODE b` sent a recessive 3rd bit. Thusly, `NODE a` continues transmission while
    /// `NODE b` waits its turn.
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = self.wire_value();
        let rhs = other.wire_value();
        let lhs_standard = lhs & 0x7FF;
        let rhs_standard = rhs & 0x7FF;

        match lhs_standard.cmp(&rhs_standard) {
            Ordering::Less => Ordering::Greater, // Less is more
            Ordering::Greater => Ordering::Less, // and more is less
            Ordering::Equal => lhs.cmp(&rhs).reverse(),
        }
    }
}

impl PartialOrd for Id {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<u32> for Id {
    fn from(id: u32) -> Id {
        match id & libc::CAN_EFF_FLAG {
            0 => Id::Standard(id & libc::CAN_SFF_MASK),
            _ => Id::Extended(id & libc::CAN_EFF_MASK),
        }
    }
}

impl Id {
    pub(crate) fn wire_value(&self) -> u32 {
        match self {
            Id::Standard(id) => id & libc::CAN_SFF_MASK,
            Id::Extended(id) => (id & libc::CAN_EFF_MASK) | libc::CAN_EFF_FLAG,
        }
    }

    pub fn value(&self) -> u32 {
        match self {
            Id::Standard(id) => id & libc::CAN_SFF_MASK,
            Id::Extended(id) => id & libc::CAN_EFF_MASK,
        }
    }
}

#[derive(Copy, Clone)]
pub enum Length {
    Bytes(u8),
    Dlc(u8),
}

impl From<Length> for u8 {
    fn from(val: Length) -> Self {
        match val {
            Length::Bytes(val) => val,
            Length::Dlc(val) => val,
        }
    }
}

/// Convert CAN DLC (for both FD and 2.0) into real byte length
///
/// If you were curious on what the most efficient way to do this is
/// (like me) and came up with a match and then a lookup table (also like me),
/// then you'll be pleased (well I was) to read this:
/// - [https://kevinlynagh.com/notes/match-vs-lookup/]
pub fn convert_dlc_to_len(dlc: Length) -> Length {
    match dlc {
        Length::Dlc(dlc) => Length::Bytes(match dlc & 0x0F {
            0..=8 => dlc,
            9 => 12,
            10 => 16,
            11 => 20,
            12 => 24,
            13 => 32,
            14 => 48,
            _ => 64,
        }),
        bytes @ Length::Bytes(_) => bytes,
    }
}

/// Convert byte length into CAN DLC (for both FD and 2.0)
///
/// See [`crate::convert_dlc_to_len`]'s notes for interesting
/// performance-related information.
pub fn convert_len_to_dlc(len: Length) -> Length {
    match len {
        Length::Bytes(len) => Length::Dlc(match len {
            0..=8 => len,
            9..=12 => 9,
            13..=16 => 10,
            17..=20 => 11,
            21..=24 => 12,
            25..=32 => 13,
            33..=48 => 14,
            _ => 15,
        }),
        dlc @ Length::Dlc(_) => dlc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_basic_id_ordinality() {
        let super_dominant = Id::Standard(0);
        let super_recessive = Id::Extended(0xFFFFFFFF);

        assert_eq!(
            std::cmp::Ordering::Greater,
            super_dominant.cmp(&super_recessive)
        );

        let mostly_dominant = Id::Extended(0x00);
        let mostly_recessive = Id::Standard(0x7FF);

        assert_eq!(
            std::cmp::Ordering::Greater,
            mostly_dominant.cmp(&mostly_recessive)
        );
    }

    #[test]
    fn compare_edge_id_ordinality() {
        let dominant = Id::Standard(0x02);
        let recessive = Id::Extended(0x02);

        assert_eq!(std::cmp::Ordering::Greater, dominant.cmp(&recessive));

        let dominant = Id::Extended(0x1FFFF801);
        let recessive = Id::Standard(0x02);

        assert_eq!(std::cmp::Ordering::Greater, dominant.cmp(&recessive));
    }
}
