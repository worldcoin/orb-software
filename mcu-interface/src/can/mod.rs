use std::time::Duration;

pub mod canfd;
pub mod isotp;

const ACK_RX_TIMEOUT: Duration = Duration::from_millis(1500);
