use super::KeyMaterial;
use async_trait::async_trait;
use color_eyre::Result;
use secrecy::SecretVec;

pub struct TrustZone;

#[async_trait]
impl KeyMaterial for TrustZone {
    async fn fetch(&self) -> Result<SecretVec<u8>> {
        todo!("theo, u impl here")
    }
}
