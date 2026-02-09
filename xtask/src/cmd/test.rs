use crate::cmd::cmd;
use clap::Args as ClapArgs;
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

    let mut cmd_args = vec!["cargo", "nextest", "run"];
    cmd_args.extend(pkgs);
    cmd(&cmd_args)?;

    Ok(())
}
