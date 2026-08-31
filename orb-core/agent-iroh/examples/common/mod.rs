use iroh::{endpoint::Connection, protocol::AcceptError};
use orb_agent_iroh::Alpn;

pub const PHONE_SECRETKEY: [u8; 32] = [69; 32];

pub fn phone_pubkey() -> iroh::PublicKey {
    iroh::SecretKey::from_bytes(&PHONE_SECRETKEY).public()
}

/// Protocol used for talking with the mobile app.
#[derive(Debug, Default)]
pub struct AppProtocol;

impl AppProtocol {
    pub const ALPN: Alpn = Alpn("app-protocol");
}

impl iroh::protocol::ProtocolHandler for AppProtocol {
    async fn accept(&self, _connection: Connection) -> Result<(), AcceptError> {
        Ok(())
    }
}
