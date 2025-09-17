use color_eyre::{eyre::bail, Result};
use ssh2::Session as Ssh2Session;
use std::io::Read;
use std::net::TcpStream;
use tracing::{debug, info};

/// SSH wrapper that supports both password and key authentication
/// Will be removed in favor of ssh keys
pub struct SshWrapper {
    session: Ssh2Session,
}

impl SshWrapper {
    pub async fn connect_with_password(
        host: String,
        port: u16,
        username: String,
        password: String,
    ) -> Result<Self> {
        info!(
            "Connecting to {}@{}:{} with password authentication",
            username, host, port
        );

        let tcp = TcpStream::connect(format!("{}:{}", host, port)).map_err(|e| {
            color_eyre::eyre::eyre!("Failed to connect to SSH server: {}", e)
        })?;

        let mut session = Ssh2Session::new().map_err(|e| {
            color_eyre::eyre::eyre!("Failed to create SSH session: {}", e)
        })?;

        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| {
            color_eyre::eyre::eyre!("Failed to perform SSH handshake: {}", e)
        })?;

        session
            .userauth_password(&username, &password)
            .map_err(|e| {
                color_eyre::eyre::eyre!("SSH password authentication failed: {}", e)
            })?;

        if !session.authenticated() {
            bail!("SSH authentication failed");
        }

        info!("SSH authentication successful");

        Ok(Self { session })
    }

    pub async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        debug!("Executing command: {}", command);

        let mut channel = self.session.channel_session().map_err(|e| {
            color_eyre::eyre::eyre!("Failed to create SSH channel: {}", e)
        })?;

        channel
            .exec(command)
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
