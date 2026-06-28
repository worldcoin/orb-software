use super::build;
use crate::cmd::cmd;
use crate::cmd::deb;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use serde_json::Value;
use std::{
    env,
    io::{self, Write},
    process::Command,
};

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[arg(long)]
    pub bin: bool,
    #[arg(long)]
    pub teleport: bool,
    pub pkg: String,
}

pub fn run(args: Args) -> Result<()> {
    let Args { bin, teleport, pkg } = args;

    if bin {
        deploy_bin(pkg, teleport)
    } else {
        deploy_deb(pkg, teleport)
    }
}

fn deploy_deb(pkg: String, teleport: bool) -> Result<()> {
    let target = "aarch64-unknown-linux-gnu".to_string();

    let target_orb = resolve_deploy_target(teleport)?;
    println!("\ndeploying to orb via {}\n", target_orb.description());

    let services = get_crate_systemd_services(&pkg);
    println!("associated systemd services: {services:?}\n");

    build::run(build::Args {
        pkg: pkg.clone(),
        target: target.clone(),
    })?;

    deb::run(deb::Args {
        pkg: pkg.clone(),
        target,
    })?;

    let deb_path = format!("./target/deb/{pkg}.deb");
    let remote_deb = format!("/home/worldcoin/{pkg}.deb");

    println!("\ncopying .deb file to orb");
    copy_to_target(&target_orb, deb_path.as_str(), remote_deb.as_str())?;

    println!("installing .deb pkg on orb\n");
    run_remote_command(
        &target_orb,
        &[
            "sudo",
            "apt",
            "install",
            "--reinstall",
            remote_deb.as_str(),
            "-y",
        ],
    )?;

    restart_services(&target_orb, services)?;

    Ok(())
}

fn deploy_bin(pkg: String, teleport: bool) -> Result<()> {
    let target = "aarch64-unknown-linux-gnu";

    let target_orb = resolve_deploy_target(teleport)?;
    println!("\ndeploying to orb via {}\n", target_orb.description());

    let services = get_crate_systemd_services(&pkg);
    println!("associated systemd services: {services:?}\n");

    let bin = get_crate_binary_name(&pkg)?;

    cmd(&[
        "cargo",
        "zigbuild",
        "--target",
        target,
        "--release",
        "-p",
        pkg.as_str(),
    ])?;

    let bin_path = format!("./target/{target}/release/{bin}");
    let remote_bin = format!("/home/worldcoin/{bin}");
    let install_target = format!("/usr/local/bin/{bin}");

    println!("\ncopying binary to orb");
    copy_to_target(&target_orb, bin_path.as_str(), remote_bin.as_str())?;

    println!("installing binary to /usr/local/bin on orb\n");
    run_remote_command(
        &target_orb,
        &[
            "sudo",
            "install",
            "-m",
            "0755",
            remote_bin.as_str(),
            install_target.as_str(),
        ],
    )?;

    restart_services(&target_orb, services)?;

    Ok(())
}

enum DeployTarget {
    Ssh { host: String, worldcoin_pw: String },
    Teleport { host: String },
}

impl DeployTarget {
    fn description(&self) -> &str {
        match self {
            Self::Ssh { host, .. } | Self::Teleport { host } => host,
        }
    }

    fn scp_target(&self, remote_path: &str) -> String {
        format!("{}:{remote_path}", self.description())
    }
}

fn resolve_deploy_target(teleport: bool) -> Result<DeployTarget> {
    if teleport {
        let orb_id = env_or_input("orb id", "ORB_ID");
        let _worldcoin_pw = env_or_input("\nworldcoin user password", "WORLDCOIN_PW");
        let teleport_id = resolve_teleport_id(&orb_id)?;
        return Ok(DeployTarget::Teleport {
            host: format!("worldcoin@{teleport_id}"),
        });
    }

    let orb_ip = env_or_input("orb ip", "ORB_IP");
    let worldcoin_pw = env_or_input("\nworldcoin user password", "WORLDCOIN_PW");
    Ok(DeployTarget::Ssh {
        host: format!("worldcoin@{orb_ip}"),
        worldcoin_pw,
    })
}

fn resolve_teleport_id(orb_id: &str) -> Result<String> {
    let output = Command::new("tsh")
        .arg("ls")
        .arg("--format=json")
        .output()?;

    if !output.status.success() {
        return Err(eyre!(
            "`tsh ls --format=json` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = String::from_utf8_lossy(&output.stdout);
    parse_teleport_id_from_tsh_json(&output, orb_id)
}

fn parse_teleport_id_from_tsh_json(output: &str, orb_id: &str) -> Result<String> {
    let nodes: Value = serde_json::from_str(output)?;
    let nodes = nodes
        .as_array()
        .ok_or_else(|| eyre!("`tsh ls --format=json` output was not a JSON array"))?;

    let mut latest_match: Option<(&str, &str)> = None;
    for node in nodes {
        if node.get("kind").and_then(Value::as_str) != Some("node") {
            continue;
        }
        if node
            .pointer("/spec/cmd_labels/orb-id/result")
            .and_then(Value::as_str)
            != Some(orb_id)
        {
            continue;
        }

        let teleport_id = node
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                eyre!("matching teleport node for orb id {orb_id} is missing metadata.name")
            })?;
        let expires = node
            .pointer("/metadata/expires")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                eyre!(
                    "matching teleport node {teleport_id} for orb id {orb_id} is missing metadata.expires"
                )
            })?;

        if latest_match
            .map(|(_, latest_expires)| expires > latest_expires)
            .unwrap_or(true)
        {
            latest_match = Some((teleport_id, expires));
        }
    }

    latest_match
        .map(|(teleport_id, _)| teleport_id.to_owned())
        .ok_or_else(|| eyre!("could not find teleport node matching orb id {orb_id}"))
}

fn copy_to_target(
    target: &DeployTarget,
    local_path: &str,
    remote_path: &str,
) -> Result<()> {
    let remote_target = target.scp_target(remote_path);
    match target {
        DeployTarget::Ssh { worldcoin_pw, .. } => cmd(&[
            "sshpass",
            "-p",
            worldcoin_pw,
            "scp",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            local_path,
            remote_target.as_str(),
        ]),
        DeployTarget::Teleport { .. } => {
            cmd(&["tsh", "scp", local_path, remote_target.as_str()])
        }
    }
}

fn run_remote_command(target: &DeployTarget, args: &[&str]) -> Result<()> {
    match target {
        DeployTarget::Ssh { host, worldcoin_pw } => {
            let mut command = vec![
                "sshpass",
                "-p",
                worldcoin_pw.as_str(),
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                host.as_str(),
            ];
            command.extend_from_slice(args);
            cmd(&command)
        }
        DeployTarget::Teleport { host } => {
            let mut command = vec!["tsh", "ssh", host.as_str()];
            command.extend_from_slice(args);
            cmd(&command)
        }
    }
}

fn restart_services(target: &DeployTarget, services: Vec<String>) -> Result<()> {
    for service in services {
        println!("\nrestarting service {service} on orb\n");
        run_remote_command(
            target,
            &["sudo", "systemctl", "restart", service.as_str()],
        )?;
    }

    Ok(())
}

fn get_crate_binary_name(pkg: &str) -> Result<String> {
    let md = MetadataCommand::new().no_deps().exec()?;
    let pkg_name = pkg;
    let pkg = md
        .packages
        .into_iter()
        .find(|p| p.name.as_str() == pkg)
        .ok_or_else(|| eyre!("could not find crate {pkg} in the workspace"))?;

    let bins = pkg
        .targets
        .into_iter()
        .filter(|target| target.is_bin())
        .map(|target| target.name)
        .collect::<Vec<_>>();

    match bins.as_slice() {
        [] => Err(eyre!("crate {pkg_name} does not define a binary target")),
        [bin] => Ok(bin.clone()),
        _ => bins
            .iter()
            .find(|bin| bin.as_str() == pkg.name.as_str())
            .cloned()
            .ok_or_else(|| {
                eyre!("crate {pkg_name} defines multiple binary targets: {bins:?}")
            }),
    }
}

fn get_crate_systemd_services(pkg: &str) -> Vec<String> {
    let md = MetadataCommand::new().no_deps().exec().unwrap();
    let pkg = md
        .packages
        .into_iter()
        .find(|p| p.name.as_str() == pkg)
        .unwrap_or_else(|| panic!("could not find crate {pkg} in the workspace"));

    pkg.metadata
        .get("deb")
        .and_then(|deb| deb.get("systemd-units")?.as_array())
        .map(|units| {
            units
                .iter()
                .filter_map(|unit| unit.get("unit-name")?.as_str())
                .map(|name| name.to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn env_or_input(input_name: &str, env_var: &str) -> String {
    if let Ok(v) = env::var(env_var) {
        return v;
    };

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
    use super::*;
    use serde_json::json;

    fn node(name: Option<&str>, expires: Option<&str>, orb_id: &str) -> Value {
        let mut node = json!({
            "kind": "node",
            "metadata": {},
            "spec": {
                "cmd_labels": {
                    "orb-id": {
                        "result": orb_id
                    }
                }
            }
        });
        if let Some(name) = name {
            node["metadata"]["name"] = json!(name);
        }
        if let Some(expires) = expires {
            node["metadata"]["expires"] = json!(expires);
        }
        node
    }

    #[test]
    fn resolves_matching_orb_id_and_picks_latest_expires() {
        let output = json!([
            node(
                Some("ignored-non-match"),
                Some("2026-06-15T13:44:53Z"),
                "d2cd59de"
            ),
            node(
                Some("older-match"),
                Some("2026-06-15T13:17:28Z"),
                "bce8234c"
            ),
            node(
                Some("newer-match"),
                Some("2026-06-15T13:44:31Z"),
                "bce8234c"
            ),
        ])
        .to_string();

        let teleport_id = parse_teleport_id_from_tsh_json(&output, "bce8234c")
            .expect("latest matching node should resolve");
        assert_eq!(teleport_id, "newer-match");
    }

    #[test]
    fn reports_bad_tsh_json_shapes() {
        let cases = vec![
            (json!({}).to_string(), "d2cd59de", "was not a JSON array"),
            (
                json!([node(
                    Some("other-node"),
                    Some("2026-06-15T13:44:53Z"),
                    "other"
                )])
                .to_string(),
                "d2cd59de",
                "could not find teleport node matching orb id d2cd59de",
            ),
            (
                json!([node(None, Some("2026-06-15T13:44:53Z"), "d2cd59de")])
                    .to_string(),
                "d2cd59de",
                "missing metadata.name",
            ),
            (
                json!([node(Some("teleport-node-id"), None, "d2cd59de")]).to_string(),
                "d2cd59de",
                "missing metadata.expires",
            ),
        ];

        for (output, orb_id, expected_error) in cases {
            let err = parse_teleport_id_from_tsh_json(&output, orb_id)
                .expect_err("invalid tsh output should fail");
            assert!(
                err.to_string().contains(expected_error),
                "expected {err:?} to contain {expected_error:?}"
            );
        }
    }
}
