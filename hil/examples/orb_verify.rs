//! CLI tool to run verification commands on an Orb device over SSH.
//!
//! # Usage
//!
//! ```bash
//! # With password authentication (password provided on command line)
//! cargo run --example orb_verify -- --hostname 192.168.1.100 --password "secret" update-verifier
//!
//! # With key-based authentication
//! cargo run --example orb_verify -- --hostname 192.168.1.100 --key-path ~/.ssh/id_rsa capsule-status
//!
//! # Interactive password prompt (if no auth method specified)
//! cargo run --example orb_verify -- --hostname 192.168.1.100 all
//! ```

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use color_eyre::{eyre::WrapErr, Result};
use dialoguer::Password;
use orb_hil::{verify, AuthMethod, SshConnectArgs, SshWrapper};
use secrecy::SecretString;
use tracing::info;
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

#[derive(Parser, Debug)]
#[command(
    name = "orb-verify",
    about = "Run verification commands on an Orb device over SSH",
    version
)]
struct Cli {
    /// Hostname of the Orb device
    #[arg(long)]
    hostname: String,

    /// Username for SSH connection
    #[arg(long, default_value = "worldcoin")]
    username: String,

    /// Password for authentication (if not provided, will prompt interactively)
    #[arg(long)]
    password: Option<SecretString>,

    /// Path to SSH private key for authentication
    #[arg(long)]
    key_path: Option<PathBuf>,

    /// SSH port
    #[arg(long, default_value = "22")]
    port: u16,

    #[command(subcommand)]
    command: VerifyCommand,
}

#[derive(Debug, Subcommand)]
enum VerifyCommand {
    /// Run orb-update-verifier
    UpdateVerifier,
    /// Get capsule update status from nvbootctrl
    CapsuleStatus,
    /// Run check-my-orb
    CheckMyOrb,
    /// Get boot time using systemd-analyze
    BootTime,
    /// Run all verification commands
    All,
}

impl Cli {
    fn get_auth_method(&self) -> Result<AuthMethod> {
        match (&self.password, &self.key_path) {
            // Key path takes precedence if both are somehow provided
            (_, Some(key_path)) => Ok(AuthMethod::Key {
                private_key_path: key_path.clone(),
            }),
            (Some(password), None) => Ok(AuthMethod::Password(password.clone())),
            (None, None) => {
                // Prompt for password interactively
                let password = Password::new()
                    .with_prompt(format!(
                        "Password for {}@{}",
                        self.username, self.hostname
                    ))
                    .interact()
                    .wrap_err("Failed to read password")?;

                Ok(AuthMethod::Password(SecretString::from(password)))
            }
        }
    }

    async fn connect_ssh(&self) -> Result<SshWrapper> {
        let connect_args = SshConnectArgs {
            hostname: self.hostname.clone(),
            port: self.port,
            username: self.username.clone(),
            auth: self.get_auth_method()?,
        };

        SshWrapper::connect(connect_args)
            .await
            .wrap_err("Failed to establish SSH connection to Orb device")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();

    info!("Connecting to Orb device at {}:{}", cli.hostname, cli.port);
    let session = cli.connect_ssh().await?;
    info!("Successfully connected to Orb device");

    match cli.command {
        VerifyCommand::UpdateVerifier => {
            run_update_verifier(&session).await?;
        }
        VerifyCommand::CapsuleStatus => {
            run_capsule_status(&session).await?;
        }
        VerifyCommand::CheckMyOrb => {
            run_check_my_orb(&session).await?;
        }
        VerifyCommand::BootTime => {
            run_boot_time(&session).await?;
        }
        VerifyCommand::All => {
            run_all_verifications(&session).await?;
        }
    }

    Ok(())
}

async fn run_update_verifier(session: &SshWrapper) -> Result<()> {
    info!("Running orb-update-verifier...");
    let output = verify::run_update_verifier(session).await?;
    println!("=== Update Verifier Output ===");
    println!("{output}");

    Ok(())
}

async fn run_capsule_status(session: &SshWrapper) -> Result<()> {
    info!("Getting capsule update status...");
    let status = verify::get_capsule_update_status(session).await?;
    println!("=== Capsule Update Status ===");
    println!("{status}");

    Ok(())
}

async fn run_check_my_orb(session: &SshWrapper) -> Result<()> {
    info!("Running check-my-orb...");
    let output = verify::run_check_my_orb(session).await?;
    println!("=== Check My Orb Output ===");
    println!("{output}");

    Ok(())
}

async fn run_boot_time(session: &SshWrapper) -> Result<()> {
    info!("Getting boot time...");
    let output = verify::get_boot_time(session).await?;
    println!("=== Boot Time ===");
    println!("{output}");

    Ok(())
}

async fn run_all_verifications(session: &SshWrapper) -> Result<()> {
    println!("\n========================================");
    println!("Running all verification commands...");
    println!("========================================\n");

    if let Err(e) = run_update_verifier(session).await {
        println!("Error: {e}");
    }
    println!();

    if let Err(e) = run_capsule_status(session).await {
        println!("Error: {e}");
    }
    println!();

    if let Err(e) = run_check_my_orb(session).await {
        println!("Error: {e}");
    }
    println!();

    if let Err(e) = run_boot_time(session).await {
        println!("Error: {e}");
    }

    println!("\n========================================");
    println!("All verifications completed!");
    println!("========================================");

    Ok(())
}
