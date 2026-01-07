mod read_section;

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use clap::ValueEnum;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{ensure, Context as _, ContextCompat, OptionExt},
    Result,
};
use derive_more::Display;
use uuid::Uuid;

pub mod reexports {
    pub use ::clap;
    pub use ::color_eyre;
}

const AARCH64: &str = "aarch64-unknown-linux-gnu";
const ENV_OPTEE_OS_PATH: &str = "OPTEE_OS_PATH";

const STAGE_KEY_ID: &str =
    "arn:aws:kms:eu-central-1:510867353226:key/fff09fa9-1363-4588-ab71-a3a0c5b63d7d";

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

#[derive(Debug, clap::Subcommand)]
pub enum SignSubcommands {
    Crate(BuildArgs),
    File {
        #[arg(long)]
        path: PathBuf,
    },
}

/// Sign a TA
#[derive(Debug, clap::Args)]
pub struct SignArgs {
    /// Use production signing keys instead of staging
    #[arg(long)]
    prod_key_id: Option<String>,
    #[arg(long)]
    out_dir: Option<PathBuf>,
    #[command(subcommand)]
    subcommands: SignSubcommands,
}

impl SignArgs {
    pub fn run(self) -> Result<()> {
        let key_id = self.prod_key_id.unwrap_or(STAGE_KEY_ID.to_string());
        let (file_to_sign, out_dir, expected_uuid) = match self.subcommands {
            SignSubcommands::Crate(build_args) => {
                let CrateInfo {
                    uuid,
                    out_dir: cargo_out_dir,
                } = get_crate_info(&build_args)?;
                let input_file = cargo_out_dir.join(&build_args.package);
                build_args.run()?;

                (
                    input_file,
                    self.out_dir.unwrap_or(cargo_out_dir),
                    Some(uuid),
                )
            }
            SignSubcommands::File { path } => {
                (path, self.out_dir.unwrap_or(PathBuf::from(".")), None)
            }
        };

        let binary_contents =
            std::fs::read(&file_to_sign).wrap_err("failed to read elf file")?;
        let inspected_uuid = crate::read_section::read_uuid_from_elf(&binary_contents)
            .wrap_err("failed to determine TA UUID from ELF file")?;

        if let Some(expected_uuid) = expected_uuid {
            ensure!(expected_uuid == inspected_uuid);
        }

        let optee_os_path = std::env::var(ENV_OPTEE_OS_PATH).wrap_err_with(|| {
            format!("failed to read requried arg: {ENV_OPTEE_OS_PATH}")
        })?;

        run_cmd!(uv run --all-packages $optee_os_path/scripts/sign_encrypt.py sign-enc --uuid $inspected_uuid --in $file_to_sign --out $out_dir/$inspected_uuid.ta --key $key_id)?;

        Ok(())
    }
}

#[derive(Debug, ValueEnum, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display(rename_all = "lowercase")]
enum CargoProfile {
    Dev,
    Release,
    Artifact,
}

impl CargoProfile {
    const fn as_dirname(&self) -> &'static str {
        match self {
            CargoProfile::Dev => "debug",
            CargoProfile::Release => "release",
            CargoProfile::Artifact => "artifact",
        }
    }
}

/// Build a TA
#[derive(Debug, clap::Args, Clone)]
pub struct BuildArgs {
    /// The cargo package to build
    #[arg(long, short)]
    package: String,
    #[arg(long, value_enum, default_value_t = CargoProfile::Dev)]
    profile: CargoProfile,
    #[arg(long)]
    optee_workspace: Option<PathBuf>,
}

impl BuildArgs {
    pub fn run(self) -> Result<()> {
        let BuildArgs {
            package,
            profile,
            optee_workspace,
        } = self;
        let optee_workspace = optee_workspace
            .as_deref()
            .unwrap_or_else(|| optee_manifest_dir());
        run_cmd!(cd $optee_workspace; RUSTC_BOOTSTRAP=1 cargo build --target aarch64-unknown-linux-gnu --profile $profile -p $package)?;

        Ok(())
    }
}

#[derive(Debug)]
struct CrateInfo {
    uuid: Uuid,
    out_dir: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct OrbOpteeMetadata {
    uuid_path: String,
}

fn optee_manifest_dir() -> &'static Path {
    static LAZY: LazyLock<PathBuf> = LazyLock::new(|| {
        let md = cargo_metadata::MetadataCommand::new()
            .current_dir(std::env::current_exe().unwrap().parent().unwrap())
            .exec()
            .expect("must be called from a cargo workspace");
        md.workspace_root.join("optee").into()
    });

    &LAZY
}

fn get_crate_info(build_args: &BuildArgs) -> Result<CrateInfo> {
    let BuildArgs {
        package,
        profile,
        optee_workspace,
    } = build_args;
    let optee_workspace = optee_workspace
        .as_deref()
        .unwrap_or_else(|| optee_manifest_dir());
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(optee_workspace.join("Cargo.toml"))
        .exec()?;

    let out_dir = metadata
        .target_directory
        .join(AARCH64)
        .join(profile.as_dirname());
    let package = metadata
        .workspace_packages()
        .into_iter()
        .find(|p| p.name.as_str() == build_args.package)
        .wrap_err_with(|| format!("failed to find metadata for package {package}"))?;
    let optee_metadata = package
        .metadata
        .get("orb-optee")
        .ok_or_eyre("missing [package.metadata.orb-optee]")?
        .to_owned();
    let optee_metadata: OrbOpteeMetadata = serde_json::from_value(optee_metadata)
        .wrap_err(
            "failed to deserialize package's [package.metadata.orb-optee] metadata",
        )?;

    let uuid_path = package
        .manifest_path
        .parent()
        .expect("infallible")
        .join(optee_metadata.uuid_path);
    let uuid = std::fs::read_to_string(&uuid_path)
        .wrap_err_with(|| format!("failed to read {uuid_path:?}"))?
        .parse()
        .wrap_err("failed to parse uuid")?;

    Ok(CrateInfo {
        uuid,
        out_dir: out_dir.into(),
    })
}
