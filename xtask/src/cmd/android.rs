use crate::cmd::cmd;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use std::fs;
use std::path::PathBuf;

const TARGET: &str = "aarch64-linux-android";

/// Names of workspace packages whose `[package.metadata.orb]
/// unsupported_targets` lists `aarch64-linux-android` - the same mechanism
/// `ci/rust_ci_helper.py` already uses to exclude Darwin-incompatible
/// crates from `cargo nextest run --workspace`.
fn unsupported_packages() -> Result<Vec<String>> {
    let md = MetadataCommand::new().no_deps().exec()?;
    let mut names: Vec<String> = md
        .workspace_packages()
        .into_iter()
        .filter(|pkg| {
            pkg.metadata
                .get("orb")
                .and_then(|orb| orb.get("unsupported_targets"))
                .and_then(|targets| targets.as_array())
                .is_some_and(|targets| {
                    targets.iter().any(|t| t.as_str() == Some(TARGET))
                })
        })
        .map(|pkg| pkg.name.as_str().to_owned())
        .collect();
    names.sort();

    Ok(names)
}

#[derive(ClapArgs, Debug)]
pub struct BuildArgs {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: unlike `android-sweep`, a build failure here is an error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let BuildArgs { release } = args;
    let excludes = unsupported_packages()?;

    println!(
        "skipping (unsupported on {TARGET}): {}",
        excludes.join(", ")
    );

    let mut cmd_args = vec!["cargo", "build", "--workspace", "--target", TARGET];
    if release {
        cmd_args.push("--release");
    }
    for pkg in &excludes {
        cmd_args.push("--exclude");
        cmd_args.push(pkg);
    }

    cmd(&cmd_args)
}

#[derive(ClapArgs, Debug)]
pub struct PayloadArgs {
    /// Directory to stage per-crate APEX payloads into.
    #[arg(long, default_value = "target/android-apex-payloads")]
    pub out_dir: PathBuf,
    /// Use binaries from a release build.
    #[arg(long)]
    pub release: bool,
}

/// Stages one APEX payload directory per Android-supported binary crate, so
/// each project gets its own APEX rather than one shared one:
/// `<out_dir>/<crate>/bin/<binary>`, `<out_dir>/<crate>/apex_manifest.json`,
/// and a placeholder `<out_dir>/<crate>/etc/init/<binary>.rc`.
///
/// This only stages payload *contents* - it does not invoke `apexer` to
/// produce a signed `.apex` file. `apexer` requires a cluster of AOSP-built
/// host tools (avbtool, mkfs.erofs, aapt2, and its own protobuf-generated
/// manifest parser) that aren't available in this workspace; see the PR
/// description for what's needed to take this further.
///
/// The `apex_manifest.json` name/version and the `etc/init/*.rc` contents
/// are placeholders (marked TODO) - they need real reverse-DNS naming and a
/// real SELinux domain/service class before they're usable.
pub fn run_payload(args: PayloadArgs) -> Result<()> {
    let PayloadArgs { out_dir, release } = args;

    // Ensure the binaries staged below are actually up to date.
    run_build(BuildArgs { release })?;

    let md = MetadataCommand::new().no_deps().exec()?;
    let excluded = unsupported_packages()?;
    let profile_dir = if release { "release" } else { "debug" };

    fs::create_dir_all(&out_dir)?;

    for pkg in md.workspace_packages() {
        if excluded.iter().any(|e| e == pkg.name.as_str()) {
            continue;
        }
        let binaries: Vec<&str> = pkg
            .targets
            .iter()
            .filter(|t| t.is_bin())
            .map(|t| t.name.as_str())
            .collect();
        if binaries.is_empty() {
            continue;
        }

        let pkg_out = out_dir.join(pkg.name.as_str());
        let bin_out = pkg_out.join("bin");
        fs::create_dir_all(&bin_out)?;

        for bin in &binaries {
            let src = PathBuf::from("target")
                .join(TARGET)
                .join(profile_dir)
                .join(bin);
            let dst = bin_out.join(bin);
            fs::copy(&src, &dst).map_err(|e| {
                eyre!("failed to copy {} -> {}: {e}", src.display(), dst.display())
            })?;
        }

        // TODO: "com.worldcoin.orb.*" is a placeholder reverse-DNS
        // namespace and "version": 1 is a placeholder - confirm the real
        // APEX naming/versioning scheme before shipping any of this.
        fs::write(
            pkg_out.join("apex_manifest.json"),
            format!(
                "{{\n  \"name\": \"com.worldcoin.orb.{}\",\n  \"version\": 1\n}}\n",
                pkg.name
            ),
        )?;

        let init_dir = pkg_out.join("etc/init");
        fs::create_dir_all(&init_dir)?;
        for bin in &binaries {
            fs::write(
                init_dir.join(format!("{bin}.rc")),
                format!(
                    "# TODO: placeholder, not a working init script. Needs a real \
                     SELinux domain (see external/sepolicy) plus a real decision on \
                     class/user/group/oneshot before this can boot the daemon.\n\
                     service {bin} /apex/com.worldcoin.orb.{pkg}/bin/{bin}\n    \
                     class TODO_CLASS\n    user TODO_USER\n    group TODO_GROUP\n    \
                     seclabel u:r:TODO_SELINUX_DOMAIN:s0\n",
                    bin = bin,
                    pkg = pkg.name,
                ),
            )?;
        }

        println!("staged payload for `{}` at {}", pkg.name, pkg_out.display());
    }

    Ok(())
}

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
    /// Only attempt these crates instead of every workspace member.
    pub pkgs: Vec<String>,
}

/// Attempts to build every requested crate (default: the whole workspace)
/// for Android, reporting which succeed and which don't. Not every crate is
/// expected to build: this is a discovery sweep, not a release gate.
pub fn run(args: Args) -> Result<()> {
    let Args { release, pkgs } = args;

    let pkgs = if pkgs.is_empty() {
        let md = MetadataCommand::new().no_deps().exec()?;
        let mut names: Vec<String> = md
            .workspace_packages()
            .into_iter()
            .map(|pkg| pkg.name.as_str().to_owned())
            .collect();
        names.sort();
        names
    } else {
        pkgs
    };

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for pkg in &pkgs {
        println!("\n=== building `{pkg}` for {TARGET} ===");

        let mut cmd_args =
            vec!["cargo", "build", "--target", TARGET, "-p", pkg.as_str()];
        if release {
            cmd_args.push("--release");
        }

        if cmd(&cmd_args).is_ok() {
            succeeded.push(pkg.clone());
        } else {
            failed.push(pkg.clone());
        }
    }

    println!("\n=== {TARGET} build sweep summary ===");
    println!("succeeded ({}):", succeeded.len());
    for pkg in &succeeded {
        println!("  {pkg}");
    }
    println!("failed ({}):", failed.len());
    for pkg in &failed {
        println!("  {pkg}");
    }

    Ok(())
}
