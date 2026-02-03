use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::OrbId;
use serde_json::Value;
use std::{
    borrow::Cow, fmt::Display, net::IpAddr, process::Stdio, str::FromStr,
    time::SystemTime,
};
use tokio::process::Command;
use zenorb::{zenoh::bytes::Encoding, Zenorb};
use zorb::{register_rkyv_types, Example};

const BUILD_INFO: BuildInfo = make_build_info!();

/// ANSI color codes for terminal output
const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[34m";
const ROSE: &str = "\x1b[38;5;204m";

/// Returns the current timestamp colored in yellow
fn colored_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Format as HH:MM:SS.mmm
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{ROSE}{hours:02}:{minutes:02}:{seconds:02}.{millis:03}{RESET}")
}

/// Colorizes a key expression in blue
fn colorize_key_expr(key_expr: impl Display) -> String {
    format!("{BLUE}{}{RESET}", key_expr)
}

/// ANSI color codes for JSON syntax highlighting
const JSON_KEY: &str = "\x1b[36m"; // Cyan for keys
const JSON_STRING: &str = "\x1b[32m"; // Green for string values
const JSON_NUMBER: &str = "\x1b[33m"; // Yellow for numbers
const JSON_BOOL: &str = "\x1b[35m"; // Magenta for booleans
const JSON_NULL: &str = "\x1b[90m"; // Gray for null

/// Colorizes JSON output with syntax highlighting (compact, no newlines)
fn colorize_json(json_str: &str) -> String {
    match serde_json::from_str::<Value>(json_str) {
        Ok(value) => colorize_value(&value),
        Err(_) => json_str.to_string(), // Return as-is if not valid JSON
    }
}

fn colorize_value(value: &Value) -> String {
    match value {
        Value::Null => format!("{JSON_NULL}null{RESET}"),
        Value::Bool(b) => format!("{JSON_BOOL}{b}{RESET}"),
        Value::Number(n) => format!("{JSON_NUMBER}{n}{RESET}"),
        Value::String(s) => format!("{JSON_STRING}\"{s}\"{RESET}"),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(colorize_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(obj) => {
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| {
                    format!("{JSON_KEY}\"{k}\"{RESET}: {}", colorize_value(v))
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about)]
struct Cli {
    /// Port to connect to
    #[arg(short, long, default_value_t = 7447)]
    port: u16,

    /// Remote IP address to connect to (defaults to 127.0.0.1)
    #[arg(short, long)]
    remote: Option<IpAddr>,

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

        Cmd::Sub { keyexpr, type_name } => {
            println!("Subscribing to {keyexpr}");

            let rx = zenorb
                .session()
                .declare_subscriber(format!("{}/{keyexpr}", zenorb.orb_id()))
                .await
                .map_err(|e| eyre!("{e}"))?;

            while let Ok(sample) = rx.recv_async().await {
                match sample.encoding() {
                    &Encoding::TEXT_PLAIN => {
                        let txt = sample.payload().try_to_string()?;
                        println!(
                            "{} {} :: {txt}",
                            colored_timestamp(),
                            colorize_key_expr(sample.key_expr())
                        );
                    }

                    &Encoding::TEXT_JSON | &Encoding::APPLICATION_JSON => {
                        let txt = sample.payload().try_to_string()?;
                        println!(
                            "{} {} :: {}",
                            colored_timestamp(),
                            colorize_key_expr(sample.key_expr()),
                            colorize_json(&txt)
                        );
                    }

                    &Encoding::ZENOH_BYTES => {
                        let rkyv_deser = type_name
                            .as_ref()
                            .and_then(|t| rkyv_registry.get(t.as_str()));

                        match rkyv_deser {
                            None => println!(
                                "{} {} :: could not deserialize",
                                colored_timestamp(),
                                colorize_key_expr(sample.key_expr())
                            ),

                            Some(deser_fn) => {
                                let contents = deser_fn(&sample.payload().to_bytes())?;
                                println!(
                                    "{} {} :: {contents}",
                                    colored_timestamp(),
                                    colorize_key_expr(sample.key_expr())
                                );
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

                        let cmd = match rkyv_deser {
                            None => {
                                println!(
                                    "{} {} :: could not deserialize, will execute command without substitution",
                                    colored_timestamp(),
                                    colorize_key_expr(sample.key_expr())
                                );

                                Cow::Borrowed(&command)
                            }

                            Some(deser_fn) => {
                                let contents = deser_fn(&sample.payload().to_bytes())?;
                                Cow::Owned(command.replace("%s%", &contents))
                            }
                        };

                        Command::new("/usr/bin/env")
                            .arg("bash")
                            .arg("-c")
                            .arg(cmd.as_str())
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
