use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::OrbId;
use std::{process::Stdio, str::FromStr};
use tokio::process::Command;
use zenorb::{zenoh::bytes::Encoding, Zenorb};
use zorb::{register_rkyv_types, Example};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about)]
struct Cli {
    /// Port to connect to
    #[arg(short, long, default_value_t = 7447)]
    port: u16,

    /// Orb ID
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
        /// Fully qualified name of the type to deserialize
        #[arg(short = 't', long = "type")]
        type_name: Option<String>,
    },

    /// Execute a command when a message is received
    #[command(trailing_var_arg = true)]
    When {
        /// The key expression to subscribe to
        keyexpr: String,
        /// Fully qualified name of the type to deserialize
        #[arg(short = 't', long = "type")]
        type_name: Option<String>,
        /// The command to execute
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let rkyv_registry =
        register_rkyv_types!(zorb::Example, orb_connd_events::Connection);

    let cli = Cli::parse();

    let orb_id = match cli.orb_id {
        Some(oid) => OrbId::from_str(&oid).wrap_err("failed to parse orb id")?,
        None => OrbId::read().await.wrap_err("could not read orb id")?,
    };

    println!(
        "connecting to zenoh on port {} with orbid {}",
        cli.port, orb_id
    );

    let zenorb = Zenorb::from_cfg(zenorb::client_cfg(cli.port))
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

        Cmd::Sub { keyexpr, type_name } => {
            println!("Subscribing to {keyexpr}");

            let rx = zenorb
                .session()
                .declare_subscriber(format!("{}/{keyexpr}", zenorb.orb_id()))
                .await
                .map_err(|e| eyre!("{e}"))?;

            while let Ok(sample) = rx.recv_async().await {
                match sample.encoding() {
                    &Encoding::TEXT_PLAIN | &Encoding::TEXT_JSON => {
                        let txt = sample.payload().try_to_string()?;
                        println!("{} :: {txt}", sample.key_expr());
                    }

                    &Encoding::ZENOH_BYTES => {
                        let rkyv_deser = type_name
                            .as_ref()
                            .and_then(|t| rkyv_registry.get(t.as_str()));

                        match rkyv_deser {
                            None => println!(
                                "{} :: could not deserialize",
                                sample.key_expr()
                            ),

                            Some(deser_fn) => {
                                let contents =
                                    deser_fn(&sample.payload().to_bytes())?;
                                println!("{} :: {contents}", sample.key_expr());
                            }
                        }
                        println!("bytes!");
                    }

                    other => {
                        println!("received message with unsupported encoding {other}")
                    }
                }
            }
        }

        Cmd::When {
            keyexpr,
            type_name,
            command,
        } => {
            let command = command.join(" ");
            println!("Subscribing to {keyexpr}");

            let rx = zenorb
                .session()
                .declare_subscriber(format!("{}/{keyexpr}", zenorb.orb_id()))
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
                        let rkyv_deser = type_name
                            .as_ref()
                            .and_then(|t| rkyv_registry.get(t.as_str()));

                        match rkyv_deser {
                            None => println!(
                                "{} :: could not deserialize",
                                sample.key_expr()
                            ),
                            Some(deser_fn) => {
                                let contents = deser_fn(&sample.payload().to_bytes())?;
                                let cmd = command.replace("%s%", &contents);

                                Command::new("/usr/bin/env")
                                    .arg("bash")
                                    .arg("-c")
                                    .arg(cmd)
                                    .stdout(Stdio::inherit())
                                    .stderr(Stdio::inherit())
                                    .status()
                                    .await?;
                            }
                        }
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
