use async_trait::async_trait;
use color_eyre::Result;

pub mod cli;

pub enum Module {
    Modem,
}

#[async_trait]
pub trait McuUtil: 'static + Send + Sync {
    async fn powercycle(&self, module: Module) -> Result<()>;
}
