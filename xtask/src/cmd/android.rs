use crate::cmd::target::unsupported_packages;
use crate::cmd::{args, cmd};
use cargo_metadata::{semver::Version, Metadata, MetadataCommand, Package};
use clap::Args as ClapArgs;
use color_eyre::Result;
use std::path::{Path, PathBuf};

pub(crate) const TARGET: &str = "aarch64-linux-android";
pub(crate) const DEFAULT_APEX_OUT_DIR: &str = "target/android-apex";
pub(crate) const ZENOHD_VERSION: &str = "1.7.2";

#[derive(Debug, Clone)]
pub enum BuildPackage {
    Workspace(Box<Package>),
    Zenohd,
}

pub(crate) struct BuiltPackage {
    pub name: String,
    pub version: Version,
    pub binaries: Vec<(String, PathBuf)>,
}

fn parse_build_package(name: &str) -> std::result::Result<BuildPackage, String> {
    match name {
        "zenohd" => Ok(BuildPackage::Zenohd),
        _ => parse_package(name).map(|pkg| BuildPackage::Workspace(Box::new(pkg))),
    }
}

/// Shared by `android-build`/`android-apex`/`android-deploy`
#[derive(ClapArgs, Debug, Clone)]
pub struct BuildArgs {
    /// Workspace crate or upstream `zenohd` to build/package/install.
    /// If omitted, applies only to Android-supported workspace crates.
    #[arg(value_parser = parse_build_package)]
    pub pkg: Option<BuildPackage>,
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
    /// Directory to write the resulting `<apex-manifest-name>.apex` files
    /// into. Only used when packaging/installing an APEX.
    #[arg(long, default_value = DEFAULT_APEX_OUT_DIR)]
    pub out_dir: PathBuf,
}

/// Resolves and validates a `pkg` CLI argument at parse time: it must name a
/// real workspace package that's actually supported on `TARGET`, so
/// `build_with` below never has to re-check either condition.
fn parse_package(name: &str) -> std::result::Result<Package, String> {
    let md = MetadataCommand::new()
        .no_deps()
        .exec()
        .map_err(|e| format!("failed to run `cargo metadata`: {e}"))?;

    let pkg = md
        .workspace_packages()
        .into_iter()
        .find(|p| p.name.as_str() == name)
        .cloned()
        .ok_or_else(|| format!("no such package `{name}` in the workspace"))?;

    if unsupported_packages(&md, TARGET).contains(pkg.name.as_str()) {
        return Err(format!("`{name}` is unsupported on {TARGET}"));
    }

    Ok(pkg)
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: any failure among the non-excluded crates is a hard error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    run_build_with(&md, args).map(|_| ())
}

/// Like [`run_build`], but reuses metadata the caller already fetched
/// instead of shelling out to `cargo metadata` again - shared with
/// [`crate::cmd::apex::run_apex`], which needs a fresh build of the same
/// target before staging APEX payloads.
pub(crate) fn run_build_with(
    md: &Metadata,
    args: BuildArgs,
) -> Result<Vec<BuiltPackage>> {
    let pkg = match args.pkg {
        Some(BuildPackage::Zenohd) => {
            return build_zenohd(md.target_directory.as_std_path(), args.release)
                .map(|pkg| vec![pkg]);
        }
        Some(BuildPackage::Workspace(pkg)) => Some(*pkg),
        None => None,
    };
    let profile = if args.release { "release" } else { "debug" };
    let binary_dir = md.target_directory.join(TARGET).join(profile);
    Ok(build_with(md, pkg, args.release, &["build"], &[])?
        .into_iter()
        .map(|pkg| BuiltPackage {
            name: pkg.name.to_string(),
            version: pkg.version,
            binaries: pkg
                .targets
                .iter()
                .filter(|target| target.is_bin())
                .map(|target| {
                    (target.name.clone(), binary_dir.join(&target.name).into())
                })
                .collect(),
        })
        .collect())
}

fn zenohd_install_root(target_dir: &Path, release: bool) -> PathBuf {
    target_dir
        .join("android-tools/zenohd")
        .join(ZENOHD_VERSION)
        .join(if release { "release" } else { "debug" })
}

fn build_zenohd(target_dir: &Path, release: bool) -> Result<BuiltPackage> {
    let install_root = zenohd_install_root(target_dir, release);
    let build_dir = target_dir.join("android-tools/zenohd/build");
    let mut command = args![
        "cargo",
        "install",
        "zenohd",
        "--version",
        ZENOHD_VERSION,
        "--locked",
        "--no-default-features",
        "--features",
        "zenoh/transport_unixsock-stream",
        "--target",
        TARGET,
        "--target-dir",
        &build_dir,
        "--root",
        &install_root,
        "--force",
    ]
    .to_vec();
    if !release {
        command.push("--debug".as_ref());
    }
    cmd(&command)?;
    let binary = install_root.join("bin/zenohd");
    println!("Android zenohd: {}", binary.display());
    Ok(BuiltPackage {
        name: "zenohd".to_owned(),
        version: ZENOHD_VERSION.parse()?,
        binaries: vec![("zenohd".to_owned(), binary)],
    })
}

/// CLI args for `android-test` - same as [`BuildArgs`] minus `out_dir`,
/// which only matters when packaging an APEX.
#[derive(ClapArgs, Debug, Clone)]
pub struct TestArgs {
    /// Crate to build. If omitted, applies to every Android-supported crate
    /// in the workspace.
    #[arg(value_parser = parse_package)]
    pub pkg: Option<Package>,
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Compiles (but doesn't run - there's no Android device/emulator here)
/// each crate's test binaries for Android via `cargo build --tests`. Catches
/// API mismatches between target-specific modules (e.g.
/// `orb_id_linux.rs`/`orb_id_android.rs`) that only show up in test code,
/// which `android-build` alone can't see.
pub fn run_build_test(args: TestArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    build_with(&md, args.pkg, args.release, &["build", "--tests"], &[]).map(|_| ())
}

/// Lints for `aarch64-linux-android` via `cargo clippy --all-targets`, same
/// deny-warnings policy as the host clippy CI job. Catches lint issues in
/// target-specific modules (e.g. `orb_id_android.rs`) that the host-only
/// clippy run never compiles.
pub fn run_clippy(args: TestArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    build_with(
        &md,
        args.pkg,
        args.release,
        &["clippy", "--all-targets"],
        &["-D", "warnings"],
    )
    .map(|_| ())
}

fn build_with(
    md: &Metadata,
    pkg: Option<Package>,
    release: bool,
    subcmd: &[&str],
    trailing_args: &[&str],
) -> Result<Vec<Package>> {
    let excludes = unsupported_packages(md, TARGET);

    let built: Vec<Package> = match pkg {
        Some(pkg) => vec![pkg],
        None => md
            .workspace_packages()
            .into_iter()
            .filter(|p| !excludes.contains(p.name.as_str()))
            .cloned()
            .collect(),
    };
    let packages: Vec<&str> = built.iter().map(|p| p.name.as_str()).collect();
    for command in build_commands(&packages, release, subcmd, trailing_args) {
        cmd(&command)?;
    }
    Ok(built)
}

fn build_commands(
    packages: &[&str],
    release: bool,
    subcmd: &[&str],
    trailing_args: &[&str],
) -> Vec<Vec<String>> {
    // Separate invocations keep Linux defaults from being unified into the
    // Android service without disabling defaults for unrelated packages.
    let (backend_status, others): (Vec<_>, Vec<_>) = packages
        .iter()
        .copied()
        .partition(|name| *name == "orb-backend-status");
    let mut commands = Vec::new();
    for (group, android_collectors) in [(others, false), (backend_status, true)] {
        if group.is_empty() {
            continue;
        }
        let mut args = vec!["cargo"];
        args.extend_from_slice(subcmd);
        args.extend(["--target", TARGET]);
        if release {
            args.push("--release");
        }
        for package in group {
            args.extend(["-p", package]);
        }
        if android_collectors {
            args.extend(["--no-default-features", "--features", "android-collectors"]);
        }
        if !trailing_args.is_empty() {
            args.push("--");
            args.extend_from_slice(trailing_args);
        }
        commands.push(args.into_iter().map(str::to_owned).collect());
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_service_features_apply_to_build_test_and_clippy() {
        for (subcmd, trailing) in [
            (vec!["build"], vec![]),
            (vec!["build", "--tests"], vec![]),
            (vec!["clippy", "--all-targets"], vec!["-D", "warnings"]),
        ] {
            let mut expected = vec!["cargo"];
            expected.extend_from_slice(&subcmd);
            expected.extend([
                "--target",
                TARGET,
                "--release",
                "-p",
                "orb-backend-status",
                "--no-default-features",
                "--features",
                "android-collectors",
            ]);
            if !trailing.is_empty() {
                expected.push("--");
                expected.extend_from_slice(&trailing);
            }
            assert_eq!(
                build_commands(&["orb-backend-status"], true, &subcmd, &trailing),
                vec![expected],
            );
        }
    }

    #[test]
    fn workspace_build_isolates_android_service_and_preserves_other_defaults() {
        assert_eq!(
            build_commands(
                &["zenorb", "orb-backend-status", "zorb"],
                false,
                &["build"],
                &[]
            ),
            vec![
                vec![
                    "cargo", "build", "--target", TARGET, "-p", "zenorb", "-p", "zorb"
                ],
                vec![
                    "cargo",
                    "build",
                    "--target",
                    TARGET,
                    "-p",
                    "orb-backend-status",
                    "--no-default-features",
                    "--features",
                    "android-collectors"
                ],
            ],
        );
    }

    #[test]
    fn unrelated_package_does_not_build_backend_status() {
        assert_eq!(
            build_commands(&["zorb"], false, &["build"], &[]),
            vec![vec!["cargo", "build", "--target", TARGET, "-p", "zorb"]],
        );
        assert!(build_commands(&[], false, &["build"], &[]).is_empty());
    }

    #[test]
    fn recognizes_upstream_router_without_workspace_membership() {
        assert!(matches!(
            parse_build_package("zenohd"),
            Ok(BuildPackage::Zenohd)
        ));
    }

    #[test]
    fn router_version_matches_workspace_lock() {
        let lock = include_str!("../../../Cargo.lock");
        assert!(
            lock.contains(&format!("name = \"zenoh\"\nversion = \"{ZENOHD_VERSION}\""))
        );
    }

    #[test]
    fn router_install_is_isolated_by_version_and_profile() {
        let target = Path::new("/tmp/target");
        assert_eq!(
            zenohd_install_root(target, false),
            target.join("android-tools/zenohd/1.7.2/debug")
        );
        assert_eq!(
            zenohd_install_root(target, true),
            target.join("android-tools/zenohd/1.7.2/release")
        );
    }
}
