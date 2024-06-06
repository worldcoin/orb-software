use std::time::Duration;

pub mod canfd;
pub mod isotp;

const RX_TIMEOUT: Duration = Duration::from_millis(1500);
