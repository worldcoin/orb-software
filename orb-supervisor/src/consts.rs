use tokio::time::Duration;

pub const WORLDCOIN_CORE_UNIT_NAME: &str = "worldcoin-core.service";
pub const DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP: Duration = Duration::from_secs(20 * 60);
