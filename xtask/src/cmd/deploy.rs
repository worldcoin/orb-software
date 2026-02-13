use super::build;
use crate::cmd::cmd;
use crate::cmd::deb;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::Result;
use std::{
    env,
    io::{self, Write},
};

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub pkg: String,
}

pub fn run(args: Args) -> Result<()> {
    let Args { pkg } = args;
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

    for service in services {
        println!("\nrestarting service {service} on orb\n");
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
            "systemctl",
            "restart",
            service.as_str(),
        ])?;
    }

    Ok(())
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
