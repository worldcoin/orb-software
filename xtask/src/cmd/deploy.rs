use super::build;
use crate::cmd::cmd;
use crate::cmd::deb;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use std::{
    env,
    io::{self, Write},
};

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[arg(long)]
    pub bin: bool,
    pub pkg: String,
}

pub fn run(args: Args) -> Result<()> {
    let Args { bin, pkg } = args;

    if bin {
        deploy_bin(pkg)
    } else {
        deploy_deb(pkg)
    }
}

fn deploy_deb(pkg: String) -> Result<()> {
    let target = "aarch64-unknown-linux-gnu".to_string();

    let orb_ip = env_or_input("orb ip", "ORB_IP");
    let worldcoin_pw = env_or_input("\nworldcoin user password", "WORLDCOIN_PW");

    println!("\ndeploying to orb with ip address: {orb_ip}\nuser: worldcoin\npassword: '{worldcoin_pw}'\n");

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
    let host = format!("worldcoin@{orb_ip}");
    let scp_target = format!("{host}:/home/worldcoin");
    let remote_deb = format!("./{pkg}.deb");

    println!("\ncopying .deb file to orb");
    cmd(&[
        "sshpass",
        "-p",
        worldcoin_pw.as_str(),
        "scp",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        deb_path.as_str(),
        scp_target.as_str(),
    ])?;

    println!("installing .deb pkg on orb\n");
    cmd(&[
        "sshpass",
        "-p",
        worldcoin_pw.as_str(),
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        host.as_str(),
        "sudo",
        "apt",
        "install",
        "--reinstall",
        remote_deb.as_str(),
        "-y",
    ])?;

    restart_services(&worldcoin_pw, &host, services)?;

    Ok(())
}

fn deploy_bin(pkg: String) -> Result<()> {
    let target = "aarch64-unknown-linux-gnu";

    let orb_ip = env_or_input("orb ip", "ORB_IP");
    let worldcoin_pw = env_or_input("\nworldcoin user password", "WORLDCOIN_PW");

    println!("\ndeploying to orb with ip address: {orb_ip}\nuser: worldcoin\npassword: '{worldcoin_pw}'\n");

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
    let host = format!("worldcoin@{orb_ip}");
    let remote_bin = format!("/home/worldcoin/{bin}");
    let scp_target = format!("{host}:{remote_bin}");
    let install_target = format!("/usr/local/bin/{bin}");

    println!("\ncopying binary to orb");
    cmd(&[
        "sshpass",
        "-p",
        worldcoin_pw.as_str(),
        "scp",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        bin_path.as_str(),
        scp_target.as_str(),
    ])?;

    println!("installing binary to /usr/local/bin on orb\n");
    cmd(&[
        "sshpass",
        "-p",
        worldcoin_pw.as_str(),
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        host.as_str(),
        "sudo",
        "install",
        "-m",
        "0755",
        remote_bin.as_str(),
        install_target.as_str(),
    ])?;

    restart_services(&worldcoin_pw, &host, services)?;

    Ok(())
}

fn restart_services(
    worldcoin_pw: &str,
    host: &str,
    services: Vec<String>,
) -> Result<()> {
    for service in services {
        println!("\nrestarting service {service} on orb\n");
        cmd(&[
            "sshpass",
            "-p",
            worldcoin_pw,
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            host,
            "sudo",
            "systemctl",
            "restart",
            service.as_str(),
        ])?;
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
