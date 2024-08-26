use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};

#[derive(Debug, Parser)]
pub struct Reboot {
    #[arg(short)]
    recovery: bool,
}

impl Reboot {
    pub async fn run(self) -> Result<()> {
        crate::boot::reboot(self.recovery).await.wrap_err_with(|| {
            format!(
                "failed to reboot into {} mode",
                if self.recovery { "recovery" } else { "normal" }
            )
        })
    }
}
