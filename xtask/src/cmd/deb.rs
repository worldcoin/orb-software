use super::build;
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

    build::run(build::Args {
        pkg: pkg.clone(),
        target: target.clone(),
    })?;

    let path = format!("./target/deb/{pkg}.deb");
    run_cmd!(cargo deb --no-build --no-strip -p $pkg --target $target -o $path)?;
    println!("\n{pkg} successfully packaged at {path}");

    Ok(())
}
