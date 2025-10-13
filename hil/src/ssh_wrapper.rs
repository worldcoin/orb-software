use color_eyre::{eyre::bail, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;
use tracing::{debug, info};

static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Authentication method for SSH connection
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Password-based authentication
    Password(String),
    /// Key-based authentication
    Key {
        /// Path to private key file
        private_key_path: PathBuf,
    },
}

/// SSH connection arguments
#[derive(Debug, Clone)]
pub struct SshConnectArgs {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}

/// SSH wrapper that uses SSH ControlMaster for persistent connections
pub struct SshWrapper {
    connect_args: SshConnectArgs,
    control_path: PathBuf,
}

impl SshWrapper {
    pub async fn connect(args: SshConnectArgs) -> Result<Self> {
        info!("Connecting to {}@{}:{}", args.username, args.host, args.port);

        // Create a unique control path for this connection.
        // Each connection gets its own socket file to avoid conflicts when:
        // - Reconnecting after device reboots (old socket may still exist)
        // - Multiple connections in the same process (e.g., retrying failed connections)
        // Format: /tmp/ssh-control-{host}-{pid}-{counter}
        let connection_id = CONNECTION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let control_path = std::env::temp_dir().join(format!(
            "ssh-control-{}-{}-{}",
            args.host,
            std::process::id(),
            connection_id
        ));

        // Clean up any stale socket file before establishing new connection
        let _ = tokio::fs::remove_file(&control_path).await;

        let wrapper = Self {
            connect_args: args,
            control_path,
        };

        // Establish the master connection
        wrapper.establish_master_connection().await?;

        // Test the connection
        wrapper.test_connection().await?;

        info!("SSH authentication successful");
        Ok(wrapper)
    }

    async fn establish_master_connection(&self) -> Result<()> {
        let mut ssh_command = Command::new("ssh");

        // ControlMaster options to establish persistent connection
        ssh_command
            .arg("-M") // Master mode
            .arg("-N") // Don't execute a remote command
            .arg("-f") // Go to background
            .arg("-o")
            .arg(format!("ControlPath={}", self.control_path.display()))
            .arg("-o")
            .arg("ControlMaster=yes")
            .arg("-o")
            .arg("ControlPersist=10m") // Keep connection alive for 10 minutes
            .arg("-p")
            .arg(self.connect_args.port.to_string())
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null")
            .arg("-o")
            .arg("LogLevel=ERROR");

        // Add authentication method
        match &self.connect_args.auth {
            AuthMethod::Password(password) => {
                // Use sshpass for password authentication
                let mut sshpass_command = Command::new("sshpass");
                sshpass_command
                    .arg("-p")
                    .arg(password)
                    .arg("ssh")
                    .arg("-M")
                    .arg("-N")
                    .arg("-f")
                    .arg("-o")
                    .arg(format!("ControlPath={}", self.control_path.display()))
                    .arg("-o")
                    .arg("ControlMaster=yes")
                    .arg("-o")
                    .arg("ControlPersist=10m")
                    .arg("-p")
                    .arg(self.connect_args.port.to_string())
                    .arg("-o")
                    .arg("StrictHostKeyChecking=no")
                    .arg("-o")
                    .arg("UserKnownHostsFile=/dev/null")
                    .arg("-o")
                    .arg("LogLevel=ERROR")
                    .arg(format!(
                        "{}@{}",
                        self.connect_args.username, self.connect_args.host
                    ));

                let output = sshpass_command
                    .output()
                    .await
                    .map_err(|e| {
                        color_eyre::eyre::eyre!(
                            "Failed to execute sshpass: {}. Make sure sshpass is installed.",
                            e
                        )
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "Failed to establish SSH master connection: {}",
                        stderr
                    );
                }

                return Ok(());
            }
            AuthMethod::Key { private_key_path } => {
                ssh_command.arg("-i").arg(private_key_path);
            }
        }

        ssh_command.arg(format!(
            "{}@{}",
            self.connect_args.username, self.connect_args.host
        ));

        let output = ssh_command.output().await.map_err(|e| {
            color_eyre::eyre::eyre!("Failed to establish SSH master connection: {}", e)
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Failed to establish SSH master connection: {}",
                stderr
            );
        }

        Ok(())
    }

    pub async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        debug!("Executing command: {}", command);

        let mut ssh_command = Command::new("ssh");

        // Use the existing control master
        ssh_command
            .arg("-o")
            .arg(format!("ControlPath={}", self.control_path.display()))
            .arg("-o")
            .arg("ControlMaster=no")
            .arg(format!(
                "{}@{}",
                self.connect_args.username, self.connect_args.host
            ))
            .arg(command);

        let output = ssh_command
            .output()
            .await
            .map_err(|e| color_eyre::eyre::eyre!("Failed to execute ssh command: {}", e))?;

        let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_status = output.status.code().unwrap_or(-1);

        debug!(
            "Command '{}' completed with exit status: {}",
            command, exit_status
        );

        Ok(CommandResult {
            stdout: stdout_str,
            stderr: stderr_str,
            exit_status,
        })
    }

    pub async fn test_connection(&self) -> Result<()> {
        let result = self.execute_command("echo connection_test").await?;

        if result.exit_status != 0 {
            bail!(
                "Connection test failed with exit status: {}. Stderr: {}",
                result.exit_status,
                result.stderr
            );
        }

        if !result.stdout.contains("connection_test") {
            bail!("Connection test output unexpected: {}", result.stdout);
        }

        info!("Connection test successful");
        Ok(())
    }

}

impl Drop for SshWrapper {
    fn drop(&mut self) {
        // Clean up control socket file
        let control_path = self.control_path.clone();
        tokio::spawn(async move {
            let _ = tokio::fs::remove_file(&control_path).await;
        });
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
