/// This code has been tweaked after being autogenerated. List of manual tweaks:
/// - (1) rename WallMessage setter and getter to not conflict with the
///   `set_wall_message` method.
#[path = "../tweaked/login1.rs"]
pub mod proxy;

pub mod shutdown;

pub use self::proxy::{ManagerProxy, ManagerProxyBlocking};
