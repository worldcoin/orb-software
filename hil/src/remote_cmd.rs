use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use color_eyre::{
    eyre::{bail, WrapErr as _},
    Result,
};
use secrecy::SecretString;
use tokio::process::Command;
use tracing::{debug, info};

use crate::ssh_wrapper::{AuthMethod, CommandResult, SshConnectArgs, SshWrapper};

pub const DEFAULT_SSH_USERNAME: &str = "worldcoin";
pub const DEFAULT_TELEPORT_USERNAME: &str = "root";

/// Shared clap args for SSH/Teleport remote connections. Flatten into a command
/// with `#[command(flatten)]`. The command is responsible for its own
/// `--transport` and `--timeout` fields since their defaults differ per command.
#[derive(Debug, Clone, Args)]
pub struct RemoteArgs {
    /// Override the SSH hostname (takes precedence over --orb-id derived hostname)
    #[arg(long)]
    pub hostname: Option<String>,

    /// Username for SSH/Teleport
    #[arg(long)]
    pub username: Option<String>,

    /// SSH port (used only with --transport ssh)
    #[arg(long, default_value = "22")]
    pub port: u16,

    /// Password for SSH authentication (mutually exclusive with --key-path)
    #[arg(long)]
    pub password: Option<SecretString>,

    /// Path to SSH private key (mutually exclusive with --password)
    #[arg(long)]
    pub key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum CopyDirection {
    /// Copy a local file to the remote host.
    Upload,
    /// Copy a remote file to the local host.
    Download,
}

impl RemoteArgs {
    pub fn resolve_auth(
        &self,
        transport: RemoteTransport,
    ) -> Result<Option<AuthMethod>> {
        match transport {
            RemoteTransport::Ssh => match (&self.password, &self.key_path) {
                (Some(password), None) => {
                    Ok(Some(AuthMethod::Password(password.clone())))
                }
                (None, Some(private_key_path)) => Ok(Some(AuthMethod::Key {
                    private_key_path: private_key_path.clone(),
                })),
                (None, None) => {
                    bail!("--transport ssh requires --password or --key-path")
                }
                (Some(_), Some(_)) => {
                    bail!("--password and --key-path are mutually exclusive")
                }
            },
            RemoteTransport::Teleport => {
                if self.password.is_some() || self.key_path.is_some() {
                    bail!(
                        "--password/--key-path can only be used with --transport ssh"
                    );
                }
                Ok(None)
            }
        }
    }

    pub async fn connect(
        self,
        transport: RemoteTransport,
        timeout: Duration,
        orb_config: &crate::orb::OrbConfig,
    ) -> Result<RemoteSession> {
        RemoteSession::connect(self, transport, timeout, orb_config.orb_id.clone())
            .await
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RemoteTransport {
    Ssh,
    Teleport,
}

pub struct RemoteSession {
    inner: RemoteSessionInner,
}

enum RemoteSessionInner {
    Ssh(SshWrapper),
    Teleport(TeleportSession),
}

struct TeleportSession {
    target: String,
    username: String,
    timeout: Duration,
}

impl RemoteSession {
    pub async fn connect(
        args: RemoteArgs,
        transport: RemoteTransport,
        timeout: Duration,
        orb_id: Option<String>,
    ) -> Result<Self> {
        match transport {
            RemoteTransport::Ssh => {
                let hostname =
                    resolve_ssh_hostname(args.hostname.as_deref(), orb_id.as_deref())?;
                let auth = args.resolve_auth(transport)?.ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "ssh transport requires password or key authentication"
                    )
                })?;
                let username = args
                    .username
                    .unwrap_or_else(|| DEFAULT_SSH_USERNAME.to_owned());

                let connect_args = SshConnectArgs {
                    hostname,
                    port: args.port,
                    username,
                    auth,
                };
                let session =
                    tokio::time::timeout(timeout, SshWrapper::connect(connect_args))
                        .await
                        .wrap_err("ssh connection timed out")?
                        .wrap_err("failed to establish ssh connection")?;

                Ok(Self {
                    inner: RemoteSessionInner::Ssh(session),
                })
            }
            RemoteTransport::Teleport => {
                let auth = args.resolve_auth(transport)?;
                if auth.is_some() {
                    bail!(
                        "teleport transport does not support --password or --key-path"
                    );
                }
                if args.port != 22 {
                    bail!("teleport transport does not use ssh port");
                }

                let target = resolve_teleport_target(
                    args.hostname.as_deref(),
                    orb_id.as_deref(),
                    timeout,
                )
                .await?;
                let username = args
                    .username
                    .unwrap_or_else(|| DEFAULT_TELEPORT_USERNAME.to_owned());

                info!("Connecting to {}@{} via Teleport", username, target);
                let session = Self {
                    inner: RemoteSessionInner::Teleport(TeleportSession {
                        target,
                        username,
                        timeout,
                    }),
                };
                session.test_connection().await?;
                Ok(session)
            }
        }
    }

    pub async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        match &self.inner {
            RemoteSessionInner::Ssh(session) => session.execute_command(command).await,
            RemoteSessionInner::Teleport(session) => {
                session.execute_command(command).await
            }
        }
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

    pub async fn copy_file(
        &self,
        local: &Path,
        remote: &Path,
        direction: CopyDirection,
    ) -> Result<()> {
        match &self.inner {
            RemoteSessionInner::Ssh(session) => {
                session.copy_file(local, remote, direction).await
            }
            RemoteSessionInner::Teleport(session) => {
                session.copy_file(local, remote, direction).await
            }
        }
    }
}

impl TeleportSession {
    async fn execute_command(&self, command: &str) -> Result<CommandResult> {
        debug!("Executing command over teleport: {}", command);
        let output = tokio::time::timeout(self.timeout, async {
            Command::new("tsh")
                .arg("ssh")
                .arg(format!("{}@{}", self.username, self.target))
                .arg(command)
                .output()
                .await
        })
        .await
        .wrap_err("teleport command timed out")?
        .wrap_err("failed to execute tsh ssh command")?;

        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_status: output.status.code().unwrap_or(-1),
        })
    }

    async fn copy_file(
        &self,
        local: &Path,
        remote: &Path,
        direction: CopyDirection,
    ) -> Result<()> {
        let remote_spec =
            format!("{}@{}:{}", self.username, self.target, remote.display());

        let mut tsh = Command::new("tsh");
        tsh.arg("scp");

        match direction {
            CopyDirection::Upload => {
                debug!("tsh scp upload: {} -> {}", local.display(), remote_spec);
                tsh.arg(local).arg(&remote_spec);
            }
            CopyDirection::Download => {
                debug!("tsh scp download: {} -> {}", remote_spec, local.display());
                tsh.arg(&remote_spec).arg(local);
            }
        }

        let output = tokio::time::timeout(self.timeout, tsh.output())
            .await
            .wrap_err("tsh scp timed out")?
            .wrap_err("failed to execute tsh scp")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tsh scp failed: {}", stderr);
        }

        Ok(())
    }
}

fn resolve_ssh_hostname(
    hostname: Option<&str>,
    orb_id: Option<&str>,
) -> Result<String> {
    if let Some(hostname) = hostname {
        return Ok(hostname.to_owned());
    }

    let orb_id = orb_id.ok_or_else(|| {
        color_eyre::eyre::eyre!("ssh transport requires hostname or orb-id")
    })?;
    Ok(format!("orb-{orb_id}.local"))
}

async fn resolve_teleport_target(
    hostname: Option<&str>,
    orb_id_query: Option<&str>,
    timeout: Duration,
) -> Result<String> {
    if let Some(hostname) = hostname {
        return Ok(hostname.to_owned());
    }

    let orb_id_query = orb_id_query.ok_or_else(|| {
        color_eyre::eyre::eyre!("teleport transport requires hostname or orb-id")
    })?;

    let tsh_ls_output = tokio::time::timeout(timeout, async {
        Command::new("tsh").arg("ls").arg("-v").output().await
    })
    .await
    .wrap_err("`tsh ls -v` timed out")?
    .wrap_err("failed to execute `tsh ls -v`")?;

    if !tsh_ls_output.status.success() {
        let stderr = String::from_utf8_lossy(&tsh_ls_output.stderr);
        bail!("`tsh ls -v` failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&tsh_ls_output.stdout);
    parse_tsh_target_by_orb_id(&stdout, orb_id_query)
}

fn parse_tsh_target_by_orb_id(output: &str, orb_id_query: &str) -> Result<String> {
    let mut matches = Vec::new();
    for line in output.lines() {
        let Some(orb_id) = extract_orb_id_field(line) else {
            continue;
        };
        if !orb_id.contains(orb_id_query) {
            continue;
        }

        let Some(target) = line.split_whitespace().nth(1) else {
            continue;
        };
        matches.push((orb_id.to_owned(), target.to_owned()));
    }

    match matches.len() {
        0 => bail!(
            "could not resolve teleport target for orb-id query '{}' from `tsh ls -v` output",
            orb_id_query
        ),
        1 => Ok(matches.remove(0).1),
        _ => {
            let conflict_list = matches
                .into_iter()
                .map(|(orb_id, target)| format!("{orb_id}->{target}"))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "orb-id query '{}' matched multiple teleport targets: {}. Use --hostname to select one target",
                orb_id_query,
                conflict_list
            )
        }
    }
}

fn extract_orb_id_field(line: &str) -> Option<&str> {
    let start = line.find("orb-id=")? + "orb-id=".len();
    let rest = &line[start..];
    let end = rest
        .find(|ch: char| ch == ',' || ch.is_whitespace())
        .unwrap_or(rest.len());
    let orb_id = &rest[..end];
    if orb_id.is_empty() {
        return None;
    }

    Some(orb_id)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn resolves_tsh_target_for_exact_orb_id() {
        let output = "orb 7e3e8aa4-988e-4e95-95e4-476cda10bee6 \
            \u{2190} Tunnel address=10.103.0.166,orb-id=bba85baa,orb-name=ota-hilly";

        let target = parse_tsh_target_by_orb_id(output, "bba85baa")
            .expect("exact orb-id should resolve");
        assert_eq!(target, "7e3e8aa4-988e-4e95-95e4-476cda10bee6");
    }

    #[test]
    fn resolves_tsh_target_for_partial_orb_id_query() {
        let output = "orb 7e3e8aa4-988e-4e95-95e4-476cda10bee6 \
            \u{2190} Tunnel address=10.103.0.166,orb-id=bba85baa,orb-name=ota-hilly";

        let target = parse_tsh_target_by_orb_id(output, "bba")
            .expect("partial query should resolve");
        assert_eq!(target, "7e3e8aa4-988e-4e95-95e4-476cda10bee6");
    }

    #[test]
    fn fails_when_tsh_output_has_multiple_matches() {
        let output = "\
orb 11111111-1111-1111-1111-111111111111 \u{2190} Tunnel orb-id=bba85baa,orb-name=orb-1
orb 22222222-2222-2222-2222-222222222222 \u{2190} Tunnel orb-id=bba85bbf,orb-name=orb-2";

        let err = parse_tsh_target_by_orb_id(output, "bba85")
            .expect_err("must fail on ambiguity");
        assert!(err
            .to_string()
            .contains("matched multiple teleport targets"));
    }

    #[test]
    fn resolve_ssh_hostname_prefers_hostname_arg() {
        let hostname = resolve_ssh_hostname(Some("orb-override.local"), None)
            .expect("explicit hostname should be used");
        assert_eq!(hostname, "orb-override.local");
    }

    #[test]
    fn resolve_ssh_hostname_falls_back_to_orb_id() {
        let hostname = resolve_ssh_hostname(None, Some("bba85baa"))
            .expect("orb-id should resolve to mDNS hostname");
        assert_eq!(hostname, "orb-bba85baa.local");
    }

    #[test]
    fn resolve_ssh_hostname_requires_hostname_or_orb_id() {
        let err = resolve_ssh_hostname(None, None)
            .expect_err("either hostname or orb-id must be provided");
        assert!(err
            .to_string()
            .contains("ssh transport requires hostname or orb-id"));
    }

    fn sample_remote_args() -> RemoteArgs {
        RemoteArgs {
            hostname: None,
            username: None,
            port: 22,
            password: None,
            key_path: None,
        }
    }

    #[test]
    fn ssh_transport_requires_auth() {
        let args = sample_remote_args();
        let err = args
            .resolve_auth(RemoteTransport::Ssh)
            .expect_err("ssh must require auth");
        assert!(err
            .to_string()
            .contains("--transport ssh requires --password or --key-path"));
    }

    #[test]
    fn ssh_transport_accepts_password_auth() {
        let mut args = sample_remote_args();
        args.password = Some(SecretString::from("password".to_owned()));

        let auth = args
            .resolve_auth(RemoteTransport::Ssh)
            .expect("password auth should be accepted");
        assert!(matches!(auth, Some(AuthMethod::Password(_))));
    }

    #[test]
    fn ssh_transport_rejects_both_auth_methods() {
        let mut args = sample_remote_args();
        args.password = Some(SecretString::from("password".to_owned()));
        args.key_path = Some(PathBuf::from("/tmp/id_rsa"));

        let err = args
            .resolve_auth(RemoteTransport::Ssh)
            .expect_err("ssh must reject dual auth methods");
        assert!(err
            .to_string()
            .contains("--password and --key-path are mutually exclusive"));
    }

    #[test]
    fn teleport_transport_rejects_ssh_auth_flags() {
        let mut args = sample_remote_args();
        args.password = Some(SecretString::from("password".to_owned()));

        let err = args
            .resolve_auth(RemoteTransport::Teleport)
            .expect_err("teleport must reject ssh auth flags");
        assert!(err
            .to_string()
            .contains("--password/--key-path can only be used with --transport ssh"));
    }
}
