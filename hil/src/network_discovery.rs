use crate::{AuthMethod, SshConnectArgs, SshWrapper};
use color_eyre::{eyre::bail, Result};
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Configuration for network discovery on USB ethernet interfaces
#[derive(Debug, Clone)]
pub struct NetworkDiscovery {
    pub username: String,
    pub auth: AuthMethod,
    pub port: u16,
    pub ip_range_start: u8,
    pub ip_range_end: u8,
    pub connection_timeout: Duration,
}

/// Information about a discovered Orb device
#[derive(Debug, Clone)]
pub struct DiscoveredOrb {
    pub hostname: String,
    pub interface: String,
}

impl NetworkDiscovery {
    /// Discovers an Orb device on USB ethernet interfaces (orbeth0-3)
    pub async fn discover_orb(&self) -> Result<DiscoveredOrb> {
        info!("Starting Orb discovery on USB ethernet interfaces");

        let interfaces = enumerate_orbeth_interfaces().await?;

        if interfaces.is_empty() {
            bail!(
                "No USB ethernet interfaces (orbeth0-3) found.\n\
                Ensure Orb is connected via USB and udev rules are configured."
            );
        }

        info!("Found USB ethernet interfaces: {:?}", interfaces);

        let mut tasks = Vec::new();
        for interface in interfaces.iter() {
            let interface = interface.clone();
            let discovery = self.clone();
            let task =
                tokio::spawn(async move { discovery.scan_interface(&interface).await });
            tasks.push(task);
        }

        let discovery_result = timeout(self.connection_timeout, async {
            loop {
                for task in &mut tasks {
                    if task.is_finished() {
                        match task.await {
                            Ok(Ok(discovered)) => return Ok(discovered),
                            Ok(Err(e)) => debug!("Interface scan failed: {}", e),
                            Err(e) => warn!("Task panicked: {}", e),
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        match discovery_result {
            Ok(Ok(discovered)) => Ok(discovered),
            Ok(Err(e)) => Err(e),
            Err(_) => bail!(
                "Failed to discover Orb on USB ethernet after {}s.\n\
                Scanned interfaces: {}\n\
                IP range: 10.42.0.{}-{}\n\
                Suggestion: Verify Orb is powered on and SSH is running.\n\
                Or use --hostname to specify manually.",
                self.connection_timeout.as_secs(),
                interfaces.join(", "),
                self.ip_range_start,
                self.ip_range_end
            ),
        }
    }

    /// Scans a specific interface for responsive Orb devices
    async fn scan_interface(&self, interface: &str) -> Result<DiscoveredOrb> {
        debug!("Scanning interface {} for Orb devices", interface);

        let mut tasks = Vec::new();
        for ip_suffix in self.ip_range_start..=self.ip_range_end {
            let ip = format!("10.42.0.{}", ip_suffix);
            let interface = interface.to_string();
            let discovery = self.clone();

            let task = tokio::spawn(async move {
                discovery.test_ssh_connection(&ip, &interface).await
            });
            tasks.push(task);
        }

        loop {
            for task in &mut tasks {
                if task.is_finished() {
                    match task.await {
                        Ok(Ok(discovered)) => {
                            info!(
                                "Successfully connected to Orb at {} on {}",
                                discovered.hostname, discovered.interface
                            );
                            return Ok(discovered);
                        }
                        Ok(Err(e)) => debug!("SSH test failed: {}", e),
                        Err(e) => warn!("Task panicked: {}", e),
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            if tasks.iter().all(|t| t.is_finished()) {
                break;
            }
        }

        bail!("No responsive Orb found on interface {}", interface)
    }

    /// Tests SSH connection to a specific IP address
    async fn test_ssh_connection(
        &self,
        ip: &str,
        interface: &str,
    ) -> Result<DiscoveredOrb> {
        debug!("Testing SSH connection to {} on {}", ip, interface);

        let connect_args = SshConnectArgs {
            hostname: ip.to_string(),
            port: self.port,
            username: self.username.clone(),
            auth: self.auth.clone(),
        };

        let test_result = timeout(Duration::from_secs(3), async {
            SshWrapper::connect(connect_args).await
        })
        .await;

        match test_result {
            Ok(Ok(_wrapper)) => {
                debug!("SSH connection successful to {} on {}", ip, interface);

                Ok(DiscoveredOrb {
                    hostname: ip.to_string(),
                    interface: interface.to_string(),
                })
            }
            Ok(Err(e)) => {
                debug!("SSH connection failed to {}: {}", ip, e);
                Err(e)
            }
            Err(_) => {
                debug!("SSH connection timed out to {}", ip);
                bail!("Connection timeout")
            }
        }
    }
}

/// Enumerates USB ethernet interfaces (orbeth0-3) that are currently UP
async fn enumerate_orbeth_interfaces() -> Result<Vec<String>> {
    let sys_net_path = "/sys/class/net";
    let mut interfaces = Vec::new();

    let mut entries = tokio::fs::read_dir(sys_net_path).await?;

    while let Some(entry) = entries.next_entry().await? {
        let interface_name = entry.file_name();
        let interface_str = interface_name.to_string_lossy();

        if interface_str.starts_with("orbeth") && interface_str.len() == 7 {
            let operstate_path =
                format!("{}/{}/operstate", sys_net_path, interface_str);

            if let Ok(state) = tokio::fs::read_to_string(&operstate_path).await
                && state.trim() == "up"
            {
                interfaces.push(interface_str.to_string());
                debug!("Found active USB ethernet interface: {}", interface_str);
            }
        }
    }

    interfaces.sort();

    Ok(interfaces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn test_enumerate_interfaces() {
        let result = enumerate_orbeth_interfaces().await;
        assert!(result.is_ok());
    }
}
