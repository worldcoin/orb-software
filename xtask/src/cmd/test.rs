use clap::Args as ClapArgs;
use cmd_lib::run_cmd;
use color_eyre::Result;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Crate names to pass as `-p` arguments to `cargo nextest run`.
    #[arg(required = true)]
    pub packages: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let pkgs: Vec<&str> = args
        .packages
        .iter()
        .flat_map(|p| ["-p", p.as_str()])
        .collect();

    run_cmd!(cargo nextest run $[pkgs])?;

    Ok(())
}
