use crate::cmd::cmd;
use clap::Args as ClapArgs;
use color_eyre::Result;

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[arg(long, short, default_value = "aarch64-unknown-linux-gnu")]
    pub target: String,
    pub pkg: String,
}

pub fn run(args: Args) -> Result<()> {
    let Args { pkg, target } = args;

    cmd(&[
        "cargo",
        "zigbuild",
        "--target",
        target.as_str(),
        "--release",
        "-p",
        pkg.as_str(),
    ])?;

    Ok(())
}
