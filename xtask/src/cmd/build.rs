use clap::Args as ClapArgs;
use cmd_lib::run_cmd;
use color_eyre::Result;

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[arg(long, short, default_value = "aarch64-unknown-linux-gnu")]
    pub target: String,
    pub pkg: String,
}

pub fn run(args: Args) -> Result<()> {
    let Args { pkg, target } = args;

    run_cmd!(cargo zigbuild --target $target --release -p $pkg)?;

    Ok(())
}
