use iroh::{endpoint::Connection, protocol::ProtocolHandler};
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

impl ProtocolHandler for AppProtocol {
    fn accept(
        &self,
        _connection: Connection,
    ) -> impl std::future::Future<Output = Result<(), iroh::protocol::AcceptError>>
           + Send {
        // Accept all peers without auth
        std::future::ready(Ok(()))
    }
}
