use clap::ValueEnum;
use cmd_lib::run_cmd;
use color_eyre::Result;
use derive_more::Display;

/// OP-TEE related commands
#[derive(Debug, clap::Subcommand)]
pub enum Subcommands {
    #[command(subcommand)]
    Ta(TaSubcommands),
}

impl Subcommands {
    pub fn run(self) -> Result<()> {
        match self {
            Subcommands::Ta(inner) => inner.run(),
        }
    }
}

/// Trusted-Application (TA) related subcommands
#[derive(Debug, clap::Subcommand)]
pub enum TaSubcommands {
    Sign(SignArgs),
    Build(BuildArgs),
}

impl TaSubcommands {
    pub fn run(self) -> Result<()> {
        match self {
            TaSubcommands::Sign(args) => args.run(),
            TaSubcommands::Build(args) => args.run(),
        }
    }
}

/// Sign a TA
#[derive(Debug, clap::Args)]
pub struct SignArgs {
    /// Use production signing keys
    #[arg(long)]
    prod: bool,
    #[command(flatten)]
    build_args: BuildArgs,
}

impl SignArgs {
    pub fn run(self) -> Result<()> {
        if self.prod {
            todo!("prod signing not implemented yet");
        }

        self.build_args.run()?;

        todo!("signing not implemented yet")
    }
}

#[derive(Debug, ValueEnum, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display(rename_all = "lowercase")]
enum CargoProfile {
    Dev,
    Release,
    Artifact,
}

/// Build a TA
#[derive(Debug, clap::Args)]
pub struct BuildArgs {
    /// The cargo package to build
    #[arg(long, short)]
    package: String,
    #[arg(long, value_enum, default_value_t = CargoProfile::Dev)]
    profile: CargoProfile,
}

impl BuildArgs {
    pub fn run(self) -> Result<()> {
        let BuildArgs { package, profile } = self;
        run_cmd!(cd optee; RUSTC_BOOTSTRAP=1 cargo build --target aarch64-unknown-linux-gnu --profile $profile -p $package)?;

        Ok(())
    }
}
