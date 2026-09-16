use super::build;
use crate::cmd::{args, cmd};
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

    build::run(build::Args {
        pkg: pkg.clone(),
        target: target.clone(),
    })?;

    let path = format!("./target/deb/{pkg}.deb");
    cmd(&args![
        "cargo",
        "deb",
        "--no-build",
        "--no-strip",
        "-p",
        &pkg,
        "--target",
        &target,
        "-o",
        &path,
    ])?;
    println!("\n{pkg} successfully packaged at {path}");

    Ok(())
}
