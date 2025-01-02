pub mod cfg;
pub mod relay;
pub mod shellc;
pub mod shelld;
pub mod sshd;

use derive_more::From;

pub type ClientId = String;
pub const BUFFER_SIZE: usize = 350_000;

#[derive(Debug, From)]
pub enum ShellMsg {
    FromSsh(ClientId, Vec<u8>, u64),
    SshdClosed(ClientId),
}
