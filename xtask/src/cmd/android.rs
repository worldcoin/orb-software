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
/// `<out_dir>/<crate>/content/{bin/<binary>,etc/init/<binary>.rc}` (the
/// APEX's actual root filesystem) plus sidecar `apex_manifest.json`,
/// `canned_fs_config`, and `file_contexts` - the full set of inputs
/// `nix/packages/android-apex.nix`'s `apexer` needs to produce a signed
/// `.apex`.
///
/// The `apex_manifest.json` name/version, `etc/init/*.rc` contents, and
/// `file_contexts` are placeholders (marked TODO) - they need real
/// reverse-DNS naming and a real SELinux domain/service class before the
/// resulting APEX is anything more than a structurally-valid placeholder.
pub fn run_payload(args: PayloadArgs) -> Result<Vec<String>> {
    let PayloadArgs { out_dir, release } = args;

    // Ensure the binaries staged below are actually up to date.
    run_build(BuildArgs { release })?;

    let md = MetadataCommand::new().no_deps().exec()?;
    let excluded = unsupported_packages()?;
    let profile_dir = if release { "release" } else { "debug" };

    fs::create_dir_all(&out_dir)?;

    let mut staged = Vec::new();

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

        // `content/` holds exactly what apexer should see as the APEX's
        // root filesystem ("/"). apex_manifest.json/canned_fs_config/
        // file_contexts are sidecar inputs *about* that content, read via
        // their own CLI flags - they must not live inside content/ itself,
        // or apexer's e2fsdroid scans them as if they were payload files
        // and fails looking them up in canned_fs_config.
        let pkg_out = out_dir.join(pkg.name.as_str());
        let content_dir = pkg_out.join("content");
        let bin_out = content_dir.join("bin");
        fs::create_dir_all(&bin_out)?;

        // We author every path in this payload ourselves below, so we can
        // emit its canned_fs_config directly instead of re-deriving it by
        // walking the tree afterward (see gen-canned-fs-config.nix, which
        // this replaces for crate-authored payloads). uid/gid 1000
        // (Android's `system`) is a placeholder default, same status as
        // the TODO_USER/TODO_SELINUX_DOMAIN placeholders below - it should
        // be revisited together with the real init.rc user/group.
        let mut fs_config = vec![
            "/ 1000 1000 0755".to_string(),
            "/apex_manifest.pb 1000 1000 0644".to_string(),
            "/bin 1000 1000 0755".to_string(),
        ];

        for bin in &binaries {
            let src = PathBuf::from("target")
                .join(TARGET)
                .join(profile_dir)
                .join(bin);
            let dst = bin_out.join(bin);
            fs::copy(&src, &dst).map_err(|e| {
                eyre!("failed to copy {} -> {}: {e}", src.display(), dst.display())
            })?;
            fs_config.push(format!("/bin/{bin} 1000 1000 0755"));
        }

        // TODO: "com.worldcoin.orb.*" is a placeholder reverse-DNS
        // namespace and "version": 1 is a placeholder - confirm the real
        // APEX naming/versioning scheme before shipping any of this.
        //
        // Android package names are Java identifiers joined by dots, so
        // `-` (common in crate names, e.g. orb-attest) isn't valid there -
        // aapt2 rejects it outright. `_` is the closest equivalent.
        let apex_name =
            format!("com.worldcoin.orb.{}", pkg.name.as_str().replace('-', "_"));
        fs::write(
            pkg_out.join("apex_manifest.json"),
            format!("{{\n  \"name\": \"{apex_name}\",\n  \"version\": 1\n}}\n"),
        )?;

        let init_dir = content_dir.join("etc/init");
        fs::create_dir_all(&init_dir)?;
        fs_config.push("/etc 1000 1000 0755".to_string());
        fs_config.push("/etc/init 1000 1000 0755".to_string());
        for bin in &binaries {
            fs::write(
                init_dir.join(format!("{bin}.rc")),
                format!(
                    "# TODO: placeholder, not a working init script. Needs a real \
                     SELinux domain (see external/sepolicy) plus a real decision on \
                     class/user/group/oneshot before this can boot the daemon.\n\
                     service {bin} /apex/{apex_name}/bin/{bin}\n    \
                     class TODO_CLASS\n    user TODO_USER\n    group TODO_GROUP\n    \
                     seclabel u:r:TODO_SELINUX_DOMAIN:s0\n",
                    bin = bin,
                ),
            )?;
            fs_config.push(format!("/etc/init/{bin}.rc 1000 1000 0644"));
        }

        fs_config.sort();
        fs::write(
            pkg_out.join("canned_fs_config"),
            fs_config.join("\n") + "\n",
        )?;

        // TODO: placeholder SELinux context for every path - matches no
        // real sepolicy type yet. `mkfs.erofs --file-contexts` wants
        // Android's file_contexts regex format (`<path-regex> <context>`),
        // so a single catch-all entry is syntactically valid here, unlike
        // the plain per-path listing in canned_fs_config.
        fs::write(
            pkg_out.join("file_contexts"),
            "(/.*)?    u:object_r:TODO_SELINUX_CONTEXT:s0\n",
        )?;

        println!("staged payload for `{}` at {}", pkg.name, pkg_out.display());
        staged.push(pkg.name.as_str().to_owned());
    }

    Ok(staged)
}

#[derive(ClapArgs, Debug)]
pub struct ApexArgs {
    /// Directory to write the resulting `<crate>.apex` files into.
    #[arg(long, default_value = "target/android-apex")]
    pub out_dir: PathBuf,
    /// Stage/build binaries in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Stages Android payloads (like `android-apex-payload`) and packages each
/// into a signed `<out_dir>/<crate>.apex`, using the `build-apex` tool from
/// `nix/packages/android-apex.nix` (exposed as the `build-apex` flake
/// package). Requires `nix` on PATH.
pub fn run_apex(args: ApexArgs) -> Result<()> {
    let ApexArgs { out_dir, release } = args;

    let payload_out_dir = PathBuf::from("target/android-apex-payloads");
    let packages = run_payload(PayloadArgs {
        out_dir: payload_out_dir.clone(),
        release,
    })?;

    let build_apex_link = "target/android-apex/build-apex";
    cmd(&[
        "nix",
        "build",
        "--extra-experimental-features",
        "nix-command flakes",
        ".#build-apex",
        "--out-link",
        build_apex_link,
    ])?;
    let build_apex_bin = format!("{build_apex_link}/bin/build-apex");

    fs::create_dir_all(&out_dir)?;

    for pkg in &packages {
        let payload_dir = payload_out_dir.join(pkg);
        let apex_out = out_dir.join(format!("{pkg}.apex"));
        let payload_dir_str = payload_dir
            .to_str()
            .ok_or_else(|| eyre!("non-utf8 path: {}", payload_dir.display()))?;
        let apex_out_str = apex_out
            .to_str()
            .ok_or_else(|| eyre!("non-utf8 path: {}", apex_out.display()))?;

        cmd(&[build_apex_bin.as_str(), payload_dir_str, apex_out_str])?;
        println!("packaged `{pkg}` -> {}", apex_out.display());
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
