use clap::Args as ClapArgs;
use cmd_lib::run_cmd;
use color_eyre::Result;

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[arg(long, short, default_value = "aarch64-unknown-linux-gnu")]
    pub target: String,
    pub pkg: String,
}

impl Args {
    pub fn run(self) -> Result<()> {
        let Args { pkg, target } = self;

        run_cmd!(cargo zigbuild --target $target --release -p $pkg)?;

        Ok(())
    }
}
