use color_eyre::{eyre::bail, Result};
use ssh2::Session as Ssh2Session;
use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Authentication method for SSH connection
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Password-based authentication
    Password(String),
    /// Key-based authentication
    Key {
        /// Path to private key file
        private_key_path: PathBuf,
        /// Optional passphrase for encrypted private key
        passphrase: Option<String>,
    },
}

/// SSH wrapper that supports both password and key authentication
pub struct SshWrapper {
    session: Arc<Mutex<Ssh2Session>>,
}

impl SshWrapper {
    pub async fn connect(
        host: String,
        port: u16,
        username: String,
        auth: AuthMethod,
    ) -> Result<Self> {
        info!("Connecting to {}@{}:{}", username, host, port);

        let session = tokio::task::spawn_blocking(move || -> Result<Ssh2Session> {
            let tcp = TcpStream::connect(format!("{host}:{port}")).map_err(|e| {
                color_eyre::eyre::eyre!("Failed to connect to SSH server: {}", e)
            })?;

            let mut session = Ssh2Session::new().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to create SSH session: {}", e)
            })?;

            session.set_tcp_stream(tcp);
            session.handshake().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to perform SSH handshake: {}", e)
            })?;

            match auth {
                AuthMethod::Password(password) => {
                    session
                        .userauth_password(&username, &password)
                        .map_err(|e| {
                            color_eyre::eyre::eyre!("SSH password authentication failed: {}", e)
                        })?;
                }
                AuthMethod::Key {
                    private_key_path,
                    passphrase,
                } => {
                    session
                        .userauth_pubkey_file(
                            &username,
                            None, // public key path (None = derive from private key)
                            &private_key_path,
                            passphrase.as_deref(),
                        )
                        .map_err(|e| {
                            color_eyre::eyre::eyre!(
                                "SSH key authentication failed with key {}: {}",
                                private_key_path.display(),
                                e
                            )
                        })?;
                }
            }

            if !session.authenticated() {
                bail!("SSH authentication failed");
            }

            Ok(session)
        })
        .await
        .map_err(|e| color_eyre::eyre::eyre!("SSH connection task panicked: {}", e))??;

        info!("SSH authentication successful");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
        })
    }

    pub async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        debug!("Executing command: {}", command);

        let session = Arc::clone(&self.session);
        let command = command.to_string();

        tokio::task::spawn_blocking(move || -> Result<CommandResult> {
            let session = session
                .lock()
                .map_err(|e| color_eyre::eyre::eyre!("Failed to lock session: {}", e))?;

            let mut channel = session.channel_session().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to create SSH channel: {}", e)
            })?;

            channel
                .exec(&command)
                .map_err(|e| color_eyre::eyre::eyre!("Failed to execute command: {}", e))?;

            let mut stdout = String::new();
            let mut stderr = String::new();

            channel
                .read_to_string(&mut stdout)
                .map_err(|e| color_eyre::eyre::eyre!("Failed to read stdout: {}", e))?;

            channel
                .stderr()
                .read_to_string(&mut stderr)
                .map_err(|e| color_eyre::eyre::eyre!("Failed to read stderr: {}", e))?;

            channel.wait_eof().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to wait for command completion: {}", e)
            })?;

            channel.close().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to close SSH channel: {}", e)
            })?;

            let exit_status = channel.exit_status().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to get command exit status: {}", e)
            })?;

            debug!(
                "Command '{}' completed with exit status: {}",
                command, exit_status
            );

            Ok(CommandResult {
                stdout,
                stderr,
                exit_status,
            })
        })
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Command execution task panicked: {}", e))?
    }

    pub async fn test_connection(&self) -> Result<()> {
        let result = self.execute_command("echo connection_test").await?;

        if result.exit_status != 0 {
            bail!(
                "Connection test failed with exit status: {}",
                result.exit_status
            );
        }

        if !result.stdout.contains("connection_test") {
            bail!("Connection test output unexpected: {}", result.stdout);
        }

        info!("Connection test successful");
        Ok(())
    }
}

#[derive(Debug)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

impl CommandResult {
    pub fn is_success(&self) -> bool {
        self.exit_status == 0
    }
}
