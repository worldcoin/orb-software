use super::KeyMaterial;
use async_trait::async_trait;
use color_eyre::Result;
use secrecy::SecretVec;

#[derive(Clone)]
pub struct StaticKey(pub Vec<u8>);

#[async_trait]
impl KeyMaterial for StaticKey {
    async fn fetch(&self) -> Result<SecretVec<u8>> {
        Ok(self.0.clone().into())
    }
}
