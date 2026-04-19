use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::OrbId;
use std::{net::IpAddr, process::Stdio, str::FromStr};
use tokio::process::Command;
use zenorb::{zenoh::bytes::Encoding, Zenorb};
use zorb::{color, Example};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about)]
struct Cli {
    /// Port to connect to
    #[arg(short, long, default_value_t = 7447)]
    port: u16,

    /// Remote IP address to connect to (defaults to 127.0.0.1)
    #[arg(short, long)]
    remote: Option<IpAddr>,

    /// Orb ID (e.g, 74471234)
    #[arg(short, long)]
    orb_id: Option<String>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a message to a key expression
    Pub {
        /// The key expression to publish to
        keyexpr: String,
        /// The payload to publish
        payload: String,
    },

    /// Subscribe to a key expression
    Sub {
        /// The key expression to subscribe to
        keyexpr: String,
    },

    /// Execute a command when a message is received
    #[command(trailing_var_arg = true)]
    When {
        /// The key expression to subscribe to
        keyexpr: String,
        /// The command to execute
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    let orb_id = match cli.orb_id {
        Some(oid) => OrbId::from_str(&oid).wrap_err("failed to parse orb id")?,
        None => OrbId::read().await.wrap_err("could not read orb id")?,
    };

    let host = cli
        .remote
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());

    println!(
        "connecting to zenoh on {}:{} with orbid {}",
        host, cli.port, orb_id
    );

    let mut cfg = zenorb::client_cfg(cli.port);
    if let Some(remote) = &cli.remote {
        cfg.insert_json5(
            "connect/endpoints",
            &format!(r#"["tcp/{remote}:{}"]"#, cli.port),
        )
        .unwrap();
    }

    let zenorb = Zenorb::from_cfg(cfg)
        .orb_id(orb_id)
        .with_name("zorb")
        .await?;

    match cli.command {
        Cmd::Pub { keyexpr, payload } => {
            let (payload, encoding) = match keyexpr.as_str() {
                "examplefoo" => {
                    let bytes = rkyv::to_bytes::<_, 64>(&Example::Foo)?;
                    (bytes.to_vec(), Encoding::ZENOH_BYTES)
                }

                "examplebar" => {
                    let bytes = rkyv::to_bytes::<_, 64>(&Example::Bar)?;
                    (bytes.to_vec(), Encoding::ZENOH_BYTES)
                }

                _ => (payload.as_bytes().to_vec(), Encoding::TEXT_PLAIN),
            };

            zenorb
                .session()
                .put(format!("{}/{keyexpr}", zenorb.orb_id()), payload)
                .encoding(encoding)
                .await
                .map_err(|e| eyre!("{e}"))?;

            println!("published to {keyexpr} successfully");
        }

        Cmd::Sub { keyexpr } => {
            println!("Subscribing to {keyexpr}");

            let rx = zenorb
                .declare_subscriber(&keyexpr)
                .await
                .map_err(|e| eyre!("{e}"))?;

            while let Ok(sample) = rx.recv_async().await {
                match sample.encoding() {
                    &Encoding::TEXT_PLAIN => {
                        let txt = sample.payload().try_to_string()?;
                        println!(
                            "{} {} :: {txt}",
                            color::timestamp(),
                            color::key_expr(sample.key_expr())
                        );
                    }

                    &Encoding::TEXT_JSON | &Encoding::APPLICATION_JSON => {
                        let txt = sample.payload().try_to_string()?;
                        println!(
                            "{} {} :: {}",
                            color::timestamp(),
                            color::key_expr(sample.key_expr()),
                            color::json(&txt)
                        );
                    }

                    &Encoding::ZENOH_BYTES => {
                        println!(
                            "{} {} :: could not deserialize",
                            color::timestamp(),
                            color::key_expr(sample.key_expr())
                        );
                    }

                    other => {
                        println!("received message with unsupported encoding {other}")
                    }
                }
            }
        }

        Cmd::When { keyexpr, command } => {
            let command = command.join(" ");
            println!("Subscribing to {keyexpr}");

            let rx = zenorb
                .declare_subscriber(&keyexpr)
                .await
                .map_err(|e| eyre!("{e}"))?;

            while let Ok(sample) = rx.recv_async().await {
                match sample.encoding() {
                    &Encoding::TEXT_PLAIN | &Encoding::TEXT_JSON => {
                        let txt = sample.payload().try_to_string()?;
                        let cmd = command.replace("%s%", &txt);

                        Command::new("/usr/bin/env")
                            .arg("bash")
                            .arg("-c")
                            .arg(cmd)
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit())
                            .status()
                            .await?;
                    }

                    &Encoding::ZENOH_BYTES => {
                        println!("{} {} :: could not deserialize, will execute command without substitution", color::timestamp(), color::key_expr(sample.key_expr()));

                        Command::new("/usr/bin/env")
                            .arg("bash")
                            .arg("-c")
                            .arg(&command)
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit())
                            .status()
                            .await?;
                    }

                    other => {
                        println!("received message with unsupported encoding {other}")
                    }
                }
            }
        }
    }

    Ok(())
}
