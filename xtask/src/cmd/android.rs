use crate::cmd::target::unsupported_crates;
use crate::cmd::{args, cmd, cmd_captured};
use cargo_metadata::{Metadata, MetadataCommand};
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const TARGET: &str = "aarch64-linux-android";

#[derive(ClapArgs, Debug)]
pub struct BuildArgs {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: any failure among the non-excluded crates is a hard error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    run_build_with(&md, args)
}

/// Like [`run_build`], but reuses metadata the caller already fetched
/// instead of shelling out to `cargo metadata` again.
fn run_build_with(md: &Metadata, args: BuildArgs) -> Result<()> {
    let BuildArgs { release } = args;
    let excludes = unsupported_crates(md, TARGET);

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

/// Stages one APEX payload directory per Android-supported binary crate:
/// `<out_dir>/<crate>/content/{bin/<binary>,etc/init/<binary>.rc}` plus
/// sidecar `apex_manifest.json`, `canned_fs_config`, and `file_contexts` -
/// the inputs `nix/packages/android-apex.nix`'s `apexer` needs to produce a
/// signed `.apex`.
///
/// The manifest name, `etc/init/*.rc` contents, and `file_contexts` are
/// TODO placeholders needing real naming and SELinux details.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_payload(md: &Metadata, out_dir: &Path, release: bool) -> Result<Vec<String>> {
    // Rebuild first, so the binaries staged below aren't stale.
    run_build_with(md, BuildArgs { release })?;

    let excluded = unsupported_crates(md, TARGET);
    let profile_dir = if release { "release" } else { "debug" };
    // Absolute, so this works regardless of the invoking cwd.
    let target_dir = md.target_directory.as_std_path();

    fs::create_dir_all(out_dir)?;

    let mut staged = Vec::new();

    for pkg in md.workspace_packages() {
        if excluded.contains(pkg.name.as_str()) {
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

        // content/ must hold exactly the APEX's root filesystem; the
        // sidecar manifest/canned_fs_config/file_contexts live outside it,
        // or apexer's e2fsdroid mistakes them for payload files.
        let pkg_out = out_dir.join(pkg.name.as_str());
        // Recreate from scratch, so a stale binary/init script from a prior
        // run isn't bundled into an APEX whose canned_fs_config no longer
        // lists it.
        if pkg_out.exists() {
            fs::remove_dir_all(&pkg_out)?;
        }
        let content_dir = pkg_out.join("content");
        let bin_out = content_dir.join("bin");
        fs::create_dir_all(&bin_out)?;

        // We author every path ourselves, so canned_fs_config is emitted
        // directly instead of walking the tree afterward. uid/gid 1000
        // (`system`) is a placeholder, same status as TODO_USER/
        // TODO_SELINUX_DOMAIN below.
        let mut fs_config = vec![
            "/ 1000 1000 0755".to_string(),
            "/apex_manifest.pb 1000 1000 0644".to_string(),
            "/bin 1000 1000 0755".to_string(),
        ];

        for bin in &binaries {
            let src = target_dir.join(TARGET).join(profile_dir).join(bin);
            let dst = bin_out.join(bin);
            fs::copy(&src, &dst).map_err(|e| {
                eyre!("failed to copy {} -> {}: {e}", src.display(), dst.display())
            })?;
            fs_config.push(format!("/bin/{bin} 1000 1000 0755"));
        }

        let pkg_name = pkg
            .name
            .as_str()
            .strip_prefix("orb-")
            .unwrap_or(pkg.name.as_str());
        let apex_name = format!("com.worldcoin.orb.{}", pkg_name.replace('-', "_"));

        // apex_manifest's `version` must be a monotonically increasing
        // int64, not the semver this crate is actually released under (see
        // Cargo.toml), so encode that semver into one instead of a
        // placeholder constant - assumes minor/patch stay under 1000,
        // true for every crate version in this workspace today. The
        // original string is kept as `versionName` for humans.
        let version = &pkg.version;
        let version_code =
            version.major * 1_000_000 + version.minor * 1_000 + version.patch;
        fs::write(
            pkg_out.join("apex_manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "name": apex_name,
                "version": version_code,
                "versionName": version.to_string(),
                // Required for `adb install --force-non-staged` (see
                // run_deploy) to activate immediately instead of being
                // rejected/ignored by apexd.
                "supportsRebootlessUpdate": true,
            }))?,
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

        // TODO: placeholder SELinux context, matches no real sepolicy type
        // yet. A single catch-all regex entry is valid file_contexts
        // syntax, unlike the per-path listing in canned_fs_config.
        fs::write(
            pkg_out.join("file_contexts"),
            "(/.*)?    u:object_r:TODO_SELINUX_CONTEXT:s0\n",
        )?;

        staged.push(pkg.name.as_str().to_owned());
    }

    Ok(staged)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const DEFAULT_APEX_OUT_DIR: &str = "target/android-apex";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(ClapArgs, Debug)]
pub struct ApexArgs {
    /// Crate to package. If omitted, packages every Android-supported
    /// crate's APEX.
    pub pkg: Option<String>,
    /// Directory to write the resulting `<crate>.apex` files into.
    #[arg(long, default_value = DEFAULT_APEX_OUT_DIR)]
    pub out_dir: PathBuf,
    /// Stage/build binaries in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Prints a captured command's stdout+stderr to this process' matching
/// stream (stdout on success, stderr on failure), so a job's own tool
/// output is still visible even though nothing streamed live.
fn print_captured(output: &std::process::Output) {
    let mut stream: Box<dyn Write> = if output.status.success() {
        Box::new(std::io::stdout())
    } else {
        Box::new(std::io::stderr())
    };
    let _ = stream.write_all(&output.stdout);
    let _ = stream.write_all(&output.stderr);
}

/// Stages Android payloads and packages each into a signed
/// `<out_dir>/<crate>.apex`, using the `build-apex` tool from
/// `nix/packages/android-apex.nix` (exposed as the `build-apex` flake
/// package). Requires `nix` on PATH. `out_dir` must already exist. Returns
/// the path to each produced `.apex`.
///
/// Only compiled for x86_64-linux: `build-apex` (see
/// nix/packages/android-apex.nix) only ships a Linux/x86_64 prebuilt
/// mkfs.erofs and is only exposed for that system in
/// nix/shells/flake-outputs.nix.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn run_apex(args: ApexArgs) -> Result<Vec<PathBuf>> {
    let ApexArgs {
        pkg,
        out_dir,
        release,
    } = args;

    // Fetched once and threaded through run_build_with/run_payload/
    // unsupported_crates below, instead of each shelling out to `cargo
    // metadata` again for data that can't change mid-run.
    let md = MetadataCommand::new().no_deps().exec()?;

    // Absolute, so this works regardless of the invoking cwd. "build-apex"
    // must match the flake output name in nix/shells/flake-outputs.nix
    // (packages."x86_64-linux"."build-apex") - nothing checks the two
    // stay in sync, so grep for "build-apex" in both places if renaming.
    let flake_ref = format!("{}#build-apex", md.workspace_root);

    // A unique dir per invocation: `run_payload` tears down and recreates
    // each package's subdirectory on every call, so a fixed shared path
    // would race across concurrent `android-apex` invocations.
    let payload_out_dir_handle = tempfile::tempdir()?;
    let payload_out_dir = payload_out_dir_handle.path().to_path_buf();
    let packages = run_payload(&md, &payload_out_dir, release)?;

    let packages = match &pkg {
        Some(want) if packages.iter().any(|p| p == want) => vec![want.clone()],
        Some(want) => {
            return Err(eyre!(
                "package `{want}` not found in workspace, has no binary \
                 target, or is unsupported on {TARGET}"
            ));
        }
        None => packages,
    };

    fs::create_dir_all(&out_dir)?;

    // A unique dir per invocation: `nix build --out-link` replaces
    // whatever's at that path, so a fixed path would race across runs.
    let build_apex_link_dir = tempfile::tempdir()?;
    let build_apex_link = build_apex_link_dir.path().join("build-apex");
    cmd(&args![
        "nix",
        "build",
        "--extra-experimental-features",
        "nix-command flakes",
        &flake_ref,
        "--out-link",
        &build_apex_link,
    ])?;
    let build_apex_bin = build_apex_link.join("bin/build-apex");

    // Keep going on failure, so one bad crate doesn't block every other
    // APEX that's otherwise ready. One thread per package, uncapped: each
    // build-apex invocation spends most of its time waiting on apexer/
    // aapt2/avbtool/mkfs.erofs subprocesses rather than burning CPU itself,
    // so it's not purely core-bound - a few dozen threads is affordable.
    // Each thread just returns its captured output; nothing is printed
    // until every thread has joined, so it's all sequential and doesn't
    // need a stdout/stderr lock to stay unmangled.
    let payload_out_dir = &payload_out_dir;
    let out_dir = &out_dir;
    let build_apex_bin = &build_apex_bin;
    let outcomes: HashMap<String, Result<std::process::Output>> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = packages
                .iter()
                .map(|pkg| {
                    scope.spawn(move || {
                        let payload_dir = payload_out_dir.join(pkg);
                        let apex_out = out_dir.join(format!("{pkg}.apex"));
                        let result =
                            cmd_captured(&[build_apex_bin, &payload_dir, &apex_out]);
                        (pkg.clone(), result)
                    })
                })
                .collect();

            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for (pkg, result) in outcomes {
        match result {
            Ok(output) => {
                print_captured(&output);
                if output.status.success() {
                    succeeded.push(pkg);
                } else {
                    failed.push(pkg);
                }
            }
            Err(e) => {
                eprintln!("{pkg}: {e}");
                failed.push(pkg);
            }
        }
    }
    succeeded.sort();
    failed.sort();

    println!("\n=== apex packaging summary ===");
    println!("succeeded ({}): {}", succeeded.len(), succeeded.join(", "));

    if !failed.is_empty() {
        println!("failed ({}): {}", failed.len(), failed.join(", "));
        return Err(eyre!("failed to package APEX for: {}", failed.join(", ")));
    }

    Ok(succeeded
        .into_iter()
        .map(|pkg| out_dir.join(format!("{pkg}.apex")))
        .collect())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(ClapArgs, Debug)]
pub struct DeployArgs {
    /// Crate to install. If omitted, installs every Android-supported
    /// crate's APEX.
    pub pkg: Option<String>,
    /// Stage/build binaries in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Substring apexd's `adb install` prints when the device has never seen
/// this APEX package before - `adb install`/`--force-non-staged` can only
/// update a package already recorded as part of a built-in partition, see
/// docs/src/android.md.
const APEX_NEW_PACKAGE_MARKER: &str = "INSTALL_FAILED_PACKAGE_CHANGED";

/// Disables dm-verity and reboots for it to take effect, unless it's
/// already disabled - `adb disable-verity` reports that
/// itself, so no separate check is needed. Needs a userdebug/eng build.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn disable_verity() -> Result<()> {
    cmd(&["adb", "root"])?;

    let output = cmd_captured(&["adb", "disable-verity"])?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(eyre!(
            "adb disable-verity exited with {}: {text}",
            output.status
        ));
    }
    if text.contains("already disabled") {
        return Ok(());
    }

    cmd(&["adb", "reboot"])?;
    cmd(&["adb", "wait-for-device"])
}

/// Bootstraps a never-before-installed APEX by pushing it directly onto a
/// writable `/vendor/apex`, so apexd starts treating it as if it shipped
/// with the device's built-in partition (see docs/src/android.md).
/// One-time per flash - wiped by the next full flash/OTA.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn seed_apex_on_device(apex_path: &Path) -> Result<()> {
    disable_verity()?;
    cmd(&["adb", "root"])?;
    cmd(&["adb", "remount"])?;
    cmd(&args!["adb", "push", apex_path, "/vendor/apex/"])?;
    cmd(&["adb", "reboot"])?;
    cmd(&["adb", "wait-for-device"])
}

/// Runs `android-apex`, then `adb install`s the result(s). Installs with
/// `-t -r -g --force-non-staged` so each APEX is usable immediately,
/// without a reboot. If the device has never had a given package before,
/// seeds it via [`seed_apex_on_device`] and retries once. Only compiled
/// for x86_64-linux, same restriction as [`run_apex`].
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn run_deploy(args: DeployArgs) -> Result<()> {
    let DeployArgs { pkg, release } = args;

    let out_dir = PathBuf::from(DEFAULT_APEX_OUT_DIR);
    let apexes = run_apex(ApexArgs {
        pkg,
        out_dir,
        release,
    })?;

    for apex_path in &apexes {
        println!("\ninstalling via adb: {}", apex_path.display());
        let output = cmd_captured(&args![
            "adb",
            "install",
            "-t",
            "-r",
            "-g",
            "--force-non-staged",
            apex_path
        ])?;

        if output.status.success() {
            continue;
        }

        let text = String::from_utf8_lossy(&output.stderr);
        if !text.contains(APEX_NEW_PACKAGE_MARKER) {
            return Err(eyre!("adb install exited with {}: {text}", output.status));
        }

        println!(
            "\n`{}` has never been installed on this device before - hold \
             on, seeding it onto /vendor/apex first...",
            apex_path.display()
        );
        seed_apex_on_device(apex_path)?;

        println!("\nretrying install of {}", apex_path.display());
        cmd(&args![
            "adb",
            "install",
            "-t",
            "-r",
            "-g",
            "--force-non-staged",
            apex_path
        ])?;
    }

    Ok(())
}
