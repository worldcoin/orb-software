use crate::{
    mcu_util::{McuUtil, Module},
    utils::run_cmd,
};
use async_trait::async_trait;
use color_eyre::eyre::Context;

pub struct McuUtilCli;

#[async_trait]
impl McuUtil for McuUtilCli {
    async fn powercycle(&self, module: Module) -> color_eyre::eyre::Result<()> {
        let module = match module {
            Module::Modem => "modem",
        };

        let _ = run_cmd("orb-mcu-util", &["power-cycle", module])
            .await
            .wrap_err_with(|| format!("failed to powercycle {module}"))?;

        Ok(())
    }
}
