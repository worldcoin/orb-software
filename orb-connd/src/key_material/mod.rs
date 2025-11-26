use async_trait::async_trait;
use color_eyre::Result;
use secrecy::SecretVec;

pub mod static_key;
pub mod trustzone;

#[async_trait]
pub trait KeyMaterial: Send + Sync + 'static {
    async fn fetch(&self) -> Result<SecretVec<u8>>;
}
