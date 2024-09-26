#[path = "../generated/systemd1.rs"]
pub mod proxy;

pub use self::proxy::{ManagerProxy, ManagerProxyBlocking};
