use std::sync::atomic::{self, AtomicU32};

use can_rs::{addr::CanAddr, Error};
#[cfg(feature = "isotp")]
use can_rs::{isotp::addr::CanIsotpAddr, Id};

mod filters;
mod frame_stream;
#[cfg(feature = "isotp")]
mod isotp_stream;

/// Track the largest Thread ID (keeping it strictly incrementing)
static LARGEST_ID: AtomicU32 = AtomicU32::new(1);
thread_local! {
    /// Use the thread-local ID as a base for any CAN IDs to avoid collisions
    /// **Note: This will not work well for testing filtering.**
    static ID: u32 = LARGEST_ID.fetch_add(1, atomic::Ordering::Relaxed);
}

const CAN_ADDRESS_RAW: &str = "vcan1";
pub fn can_address() -> CanAddr {
    CAN_ADDRESS_RAW
        .parse()
        .expect("failed to parse `{CAN_ADDRESS_RAW}` into CanAddr")
}

const CANFD_ADDRESS_RAW: &str = "vcan0";
pub fn canfd_address() -> CanAddr {
    CANFD_ADDRESS_RAW
        .parse()
        .expect("failed to parse `{CANFD_ADDRESS_RAW}` into CanAddr")
}

#[cfg(feature = "isotp")]
const ISOTP_ADDRESS_RAW: &str = "vcan2";
#[cfg(feature = "isotp")]
const ISOTP_ADDRESS_TX: Id = Id::Standard(0x321);
#[cfg(feature = "isotp")]
const ISOTP_ADDRESS_RX: Id = Id::Standard(0x123);
#[cfg(feature = "isotp")]
pub fn isotp_address() -> CanIsotpAddr {
    CanIsotpAddr::new(ISOTP_ADDRESS_RAW, ISOTP_ADDRESS_TX, ISOTP_ADDRESS_RX).unwrap()
}

#[test]
pub fn parse_addresses() -> Result<(), Error> {
    CAN_ADDRESS_RAW.parse::<CanAddr>()?;
    CANFD_ADDRESS_RAW.parse::<CanAddr>()?;
    Ok(())
}
