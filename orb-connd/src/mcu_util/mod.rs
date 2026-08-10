use crate::utils::run_cmd;
use color_eyre::eyre::Context;

pub enum Module {
    Modem,
}

#[cfg_attr(feature = "testing", faux::create)]
pub struct McuUtil;

#[cfg_attr(feature = "testing", faux::methods)]
impl McuUtil {
    pub async fn powercycle(&self, module: Module) -> color_eyre::eyre::Result<()> {
        let module = match module {
            Module::Modem => "modem",
        };

        let _ = run_cmd("orb-mcu-util", &["power-cycle", module])
            .await
            .wrap_err_with(|| format!("failed to powercycle {module}"))?;

        Ok(())
    }
}
