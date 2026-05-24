use super::build;
use crate::cmd::cmd;
use crate::cmd::deb;
use cargo_metadata::{MetadataCommand, Package};
use clap::Args as ClapArgs;
use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};
use serde_json::Value;
use std::{
    env,
    io::{self, Write},
    path::Path,
    process::Command,
};

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub pkg: String,
}

#[derive(Debug)]
enum RemoteTarget {
    Ssh { host: String, password: String },
    Teleport(TeleportTarget),
}

#[derive(Debug)]
struct PackageDeployInfo {
    services: Vec<String>,
    bind_mounts: Vec<BindMount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindMount {
    binary_name: String,
    local_path: String,
    remote_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeleportTarget {
    tunnel: String,
    orb_id: Option<String>,
    orb_name: Option<String>,
    release: Option<String>,
    release_type: Option<String>,
}

impl TeleportTarget {
    fn matches(&self, query: &str) -> bool {
        let query = normalize_teleport_query(query);
        self.tunnel == query
            || self.orb_id.as_deref() == Some(query)
            || self.orb_name.as_deref() == Some(query)
    }

    fn is_stage(&self) -> bool {
        matches!(self.release_type.as_deref(), Some("stage"))
            || self
                .release
                .as_deref()
                .is_some_and(|release| release.ends_with("-stage"))
    }

    fn remote(&self) -> String {
        format!("root@{}", self.tunnel)
    }

    fn description(&self) -> String {
        let mut details = vec![format!("tunnel={}", self.tunnel)];
        if let Some(orb_id) = &self.orb_id {
            details.push(format!("orb-id={orb_id}"));
        }
        if let Some(orb_name) = &self.orb_name {
            details.push(format!("orb-name={orb_name}"));
        }
        if let Some(release) = &self.release {
            details.push(format!("release={release}"));
        }
        if let Some(release_type) = &self.release_type {
            details.push(format!("release_type={release_type}"));
        }

        details.join(", ")
    }
}

pub fn run(args: Args) -> Result<()> {
    let Args { pkg } = args;
    let build_target = "aarch64-unknown-linux-gnu".to_string();
    let package_info = get_package_deploy_info(&pkg, &build_target)?;
    let orb_target = orb_target_or_input();
    let remote_target = resolve_remote_target(&orb_target)?;

    match &remote_target {
        RemoteTarget::Ssh { host, .. } => {
            println!("\ndeploying to orb via ssh: host={host}, user=worldcoin\n");
        }
        RemoteTarget::Teleport(target) => {
            println!(
                "\ndeploying to orb via teleport: {}\n",
                target.description()
            );
        }
    }

    println!("associated systemd services: {:?}\n", package_info.services);

    match remote_target {
        RemoteTarget::Ssh { host, password } => {
            deb::run(deb::Args {
                pkg: pkg.clone(),
                target: build_target.clone(),
            })?;
            deploy_deb_over_ssh(&pkg, &host, &password, &package_info.services)?;
        }
        RemoteTarget::Teleport(target) if target.is_stage() => {
            if package_info.bind_mounts.is_empty() {
                bail!(
                    "package {pkg} has no /usr/local/bin assets to bind mount for stage deploy"
                );
            }

            println!(
                "stage release detected, deploying binaries via /tmp bind mounts\n"
            );

            build::run(build::Args {
                pkg: pkg.clone(),
                target: build_target.clone(),
            })?;
            deploy_stage_over_teleport(
                &target,
                &package_info.bind_mounts,
                &package_info.services,
            )?;
        }
        RemoteTarget::Teleport(target) => {
            deb::run(deb::Args {
                pkg: pkg.clone(),
                target: build_target,
            })?;
            deploy_deb_over_teleport(&pkg, &target, &package_info.services)?;
        }
    }

    Ok(())
}

fn deploy_deb_over_ssh(
    pkg: &str,
    host: &str,
    password: &str,
    services: &[String],
) -> Result<()> {
    let ssh_host = format!("worldcoin@{host}");
    let remote_deb = format!("/home/worldcoin/{pkg}.deb");
    let local_deb = format!("./target/deb/{pkg}.deb");

    println!("copying .deb file to orb");
    cmd(&[
        "sshpass",
        "-p",
        password,
        "scp",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        local_deb.as_str(),
        format!("{ssh_host}:{remote_deb}").as_str(),
    ])?;

    println!("installing .deb pkg on orb\n");
    cmd(&[
        "sshpass",
        "-p",
        password,
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        ssh_host.as_str(),
        "sudo",
        "apt",
        "install",
        "--reinstall",
        remote_deb.as_str(),
        "-y",
    ])?;

    restart_services_over_ssh(&ssh_host, password, services)
}

fn deploy_deb_over_teleport(
    pkg: &str,
    target: &TeleportTarget,
    services: &[String],
) -> Result<()> {
    let remote = target.remote();
    let remote_deb = format!("/tmp/{pkg}.deb");
    let local_deb = format!("./target/deb/{pkg}.deb");

    println!("copying .deb file to orb");
    cmd(&[
        "tsh",
        "scp",
        local_deb.as_str(),
        format!("{remote}:{remote_deb}").as_str(),
    ])?;

    println!("installing .deb pkg on orb\n");
    cmd(&[
        "tsh",
        "ssh",
        remote.as_str(),
        "apt",
        "install",
        "--reinstall",
        remote_deb.as_str(),
        "-y",
    ])?;

    restart_services_over_teleport(target, services)
}

fn deploy_stage_over_teleport(
    target: &TeleportTarget,
    bind_mounts: &[BindMount],
    services: &[String],
) -> Result<()> {
    let remote = target.remote();

    for mount in bind_mounts {
        let remote_tmp = format!("/tmp/{}", mount.binary_name);
        println!(
            "copying {} to {} via teleport",
            mount.local_path, mount.remote_path
        );
        cmd(&[
            "tsh",
            "scp",
            mount.local_path.as_str(),
            format!("{remote}:{remote_tmp}").as_str(),
        ])?;
    }

    cmd(&[
        "tsh",
        "ssh",
        remote.as_str(),
        "mount",
        "-o",
        "remount,exec,rw",
        "/tmp",
    ])?;

    for mount in bind_mounts {
        let remote_tmp = format!("/tmp/{}", mount.binary_name);
        cmd(&[
            "tsh",
            "ssh",
            remote.as_str(),
            "chmod",
            "755",
            remote_tmp.as_str(),
        ])?;
        cmd(&[
            "tsh",
            "ssh",
            remote.as_str(),
            "mount",
            "--bind",
            remote_tmp.as_str(),
            mount.remote_path.as_str(),
        ])?;
    }

    restart_services_over_teleport(target, services)
}

fn restart_services_over_ssh(
    ssh_host: &str,
    password: &str,
    services: &[String],
) -> Result<()> {
    for service in services {
        println!("\nrestarting service {service} on orb\n");
        cmd(&[
            "sshpass",
            "-p",
            password,
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            ssh_host,
            "sudo",
            "systemctl",
            "restart",
            service.as_str(),
        ])?;
    }

    Ok(())
}

fn restart_services_over_teleport(
    target: &TeleportTarget,
    services: &[String],
) -> Result<()> {
    let remote = target.remote();

    for service in services {
        println!("\nrestarting service {service} on orb\n");
        cmd(&[
            "tsh",
            "ssh",
            remote.as_str(),
            "systemctl",
            "restart",
            service.as_str(),
        ])?;
    }

    Ok(())
}

fn get_package_deploy_info(pkg: &str, target: &str) -> Result<PackageDeployInfo> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .wrap_err("failed to load cargo metadata")?;
    let package = metadata
        .packages
        .into_iter()
        .find(|package| package.name.as_str() == pkg)
        .ok_or_else(|| eyre!("could not find crate {pkg} in the workspace"))?;

    Ok(PackageDeployInfo {
        services: get_crate_systemd_services(&package),
        bind_mounts: get_bind_mounts(&package, target),
    })
}

fn get_crate_systemd_services(pkg: &Package) -> Vec<String> {
    pkg.metadata
        .get("deb")
        .and_then(|deb| deb.get("systemd-units")?.as_array())
        .map(|units| {
            units
                .iter()
                .filter_map(|unit| unit.get("unit-name")?.as_str())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn get_bind_mounts(pkg: &Package, target: &str) -> Vec<BindMount> {
    pkg.metadata
        .get("deb")
        .and_then(|deb| deb.get("assets"))
        .and_then(Value::as_array)
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| bind_mount_from_asset(asset, target))
                .collect()
        })
        .unwrap_or_default()
}

fn bind_mount_from_asset(asset: &Value, target: &str) -> Option<BindMount> {
    let asset = asset.as_array()?;
    let source = asset.first()?.as_str()?;
    let destination = asset.get(1)?.as_str()?;

    if !source.starts_with("target/release/") {
        return None;
    }

    if !destination.starts_with("/usr/local/bin") {
        return None;
    }

    let binary_name = Path::new(source).file_name()?.to_str()?.to_owned();
    let remote_path = if destination.ends_with('/') {
        format!("{destination}{binary_name}")
    } else {
        destination.to_owned()
    };

    Some(BindMount {
        local_path: format!("./target/{target}/release/{binary_name}"),
        remote_path,
        binary_name,
    })
}

fn resolve_remote_target(target: &str) -> Result<RemoteTarget> {
    let target = strip_user_prefix(target).to_owned();

    if looks_like_ssh_target(&target) {
        let password = env_or_input("\nworldcoin user password", "WORLDCOIN_PW");
        return Ok(RemoteTarget::Ssh {
            host: target,
            password,
        });
    }

    Ok(RemoteTarget::Teleport(resolve_teleport_target(&target)?))
}

fn looks_like_ssh_target(target: &str) -> bool {
    if let Some((prefix, suffix)) = target.split_once(':') {
        if is_uuid(prefix) && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
    }

    target.contains('.') || target.contains(':')
}

fn resolve_teleport_target(query: &str) -> Result<TeleportTarget> {
    let output = command_output(&["tsh", "ls", "-v"]).wrap_err(
        "failed to query Teleport. Make sure `tsh` is installed and logged in",
    )?;
    let matches: Vec<_> = output
        .lines()
        .filter_map(parse_teleport_target)
        .filter(|target| target.matches(query))
        .collect();

    match matches.as_slice() {
        [] => bail!(
            "could not find Teleport target `{query}`. Try the tunnel id, orb-id, or orb-name from `tsh ls -v`"
        ),
        [target] => Ok(target.clone()),
        many => {
            let descriptions = many
                .iter()
                .map(TeleportTarget::description)
                .collect::<Vec<_>>()
                .join("\n");
            bail!("multiple Teleport targets matched `{query}`:\n{descriptions}")
        }
    }
}

fn parse_teleport_target(line: &str) -> Option<TeleportTarget> {
    let mut tunnel = None;
    let mut orb_id = None;
    let mut orb_name = None;
    let mut release = None;
    let mut release_type = None;

    for token in line.split_whitespace() {
        let token = token.trim_matches(',');
        if tunnel.is_none() && is_uuid(token) {
            tunnel = Some(token.to_owned());
        }

        for label in token.split(',') {
            let Some((key, value)) = label.split_once('=') else {
                continue;
            };
            match key {
                "orb-id" => orb_id = Some(value.to_owned()),
                "orb-name" => orb_name = Some(value.to_owned()),
                "release" => release = Some(value.to_owned()),
                "release_type" => release_type = Some(value.to_owned()),
                _ => {}
            }
        }
    }

    Some(TeleportTarget {
        tunnel: tunnel?,
        orb_id,
        orb_name,
        release,
        release_type,
    })
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for (index, byte) in bytes.iter().enumerate() {
        let is_hyphen = matches!(index, 8 | 13 | 18 | 23);
        if is_hyphen {
            if *byte != b'-' {
                return false;
            }
            continue;
        }

        if !byte.is_ascii_hexdigit() {
            return false;
        }
    }

    true
}

fn command_output(args: &[&str]) -> Result<String> {
    let (program, rest) = args.split_first().ok_or_else(|| eyre!("empty cmd"))?;
    let output = Command::new(program)
        .args(rest)
        .output()
        .wrap_err_with(|| format!("failed to run {program}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} exited with {}: {}", output.status, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn strip_user_prefix(target: &str) -> &str {
    target.rsplit_once('@').map_or(target, |(_, host)| host)
}

fn normalize_teleport_query(target: &str) -> &str {
    let target = strip_user_prefix(target);
    if let Some((prefix, suffix)) = target.split_once(':') {
        if is_uuid(prefix) && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return prefix;
        }
    }

    target
}

fn orb_target_or_input() -> String {
    if let Ok(v) = env::var("ORB") {
        return v;
    };

    if let Ok(v) = env::var("ORB_IP") {
        eprintln!("ORB_IP is deprecated, use ORB instead.");
        return v;
    };

    prompt_for_input("orb ip / hostname / teleport target", "ORB")
}

fn env_or_input(input_name: &str, env_var: &str) -> String {
    if let Ok(v) = env::var(env_var) {
        return v;
    };

    prompt_for_input(input_name, env_var)
}

fn prompt_for_input(input_name: &str, env_var: &str) -> String {
    println!("{env_var} not set. Please input {input_name}: ");
    print!("> ");
    io::stdout().flush().unwrap();

    let mut name = String::new();

    io::stdin()
        .read_line(&mut name)
        .expect("Failed to read line");

    name.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{bind_mount_from_asset, parse_teleport_target};
    use serde_json::json;

    #[test]
    fn parses_teleport_target_from_tsh_output() {
        let line = "orb 8047bc3d-3b88-4dc2-a382-b65139ca99bc ⟵ Tunnel address=10.108.1.133,city=Munich,country=DE,orb-id=bba85baa,orb-name=ota-hilly,release=7.16.1-diamond-stage,release_type=stage";
        let target = parse_teleport_target(line).expect("target should parse");

        assert_eq!(target.tunnel, "8047bc3d-3b88-4dc2-a382-b65139ca99bc");
        assert_eq!(target.orb_id.as_deref(), Some("bba85baa"));
        assert_eq!(target.orb_name.as_deref(), Some("ota-hilly"));
        assert_eq!(target.release.as_deref(), Some("7.16.1-diamond-stage"));
        assert_eq!(target.release_type.as_deref(), Some("stage"));
        assert!(target.is_stage());
    }

    #[test]
    fn bind_mount_asset_only_uses_built_usr_local_bin_assets() {
        let asset =
            json!(["target/release/orb-update-agent", "/usr/local/bin/", "755"]);
        let mount = bind_mount_from_asset(&asset, "aarch64-unknown-linux-gnu")
            .expect("asset should become bind mount");

        assert_eq!(
            mount.local_path,
            "./target/aarch64-unknown-linux-gnu/release/orb-update-agent"
        );
        assert_eq!(mount.remote_path, "/usr/local/bin/orb-update-agent");
        assert_eq!(mount.binary_name, "orb-update-agent");
    }

    #[test]
    fn bind_mount_asset_ignores_non_binary_assets() {
        let asset = json!(["warn_renamed.sh", "/usr/bin/orb-slot-ctrl", "755"]);
        assert!(bind_mount_from_asset(&asset, "aarch64-unknown-linux-gnu").is_none());

        let asset =
            json!(["sound/assets/*.wav", "/home/worldcoin/data/sounds/", "644"]);
        assert!(bind_mount_from_asset(&asset, "aarch64-unknown-linux-gnu").is_none());
    }
}
