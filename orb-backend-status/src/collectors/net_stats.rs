use eyre::{eyre, Result};
use orb_backend_status_dbus::types::{NetIntf, NetStats};
use orb_backend_status_dbus::BackendStatusT;
use orb_telemetry::TraceCtx;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::dbus::intf_impl::BackendStatusImpl;

const IFACE_WLAN0: &str = "wlan0";
const IFACE_WWAN0: &str = "wwan0";

pub fn spawn_reporter(
    backend_status: BackendStatusImpl,
    interval: std::time::Duration,
    shutdown_token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = time::interval(interval);
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,
                _ = ticker.tick() => {}
            }

            match poll_net_stats().await {
                Ok(net_stats) => {
                    if let Err(e) =
                        backend_status.provide_net_stats(net_stats, TraceCtx::collect())
                    {
                        error!("failed to update net stats: {e:?}");
                    }
                }
                Err(e) => {
                    error!("failed to poll net stats: {e:?}");
                }
            }
        }
    })
}

pub async fn poll_net_stats() -> Result<NetStats, eyre::Error> {
    let net_stats = match tokio::fs::read_to_string("/proc/net/dev").await {
        Ok(net_stats) => net_stats,
        Err(e) => {
            error!("failed to read /proc/net/dev: {e:?}");
            return Err(e.into());
        }
    };

    parse_net_stats(&net_stats)
}

fn parse_net_stats(net_stats: &str) -> Result<NetStats, eyre::Error> {
    let mut interfaces = Vec::new();

    // Try to parse stats for both WLAN0 and WWAN0 interfaces
    for iface_name in [IFACE_WLAN0, IFACE_WWAN0] {
        if let Some(interface) = parse_interface_stats(net_stats, iface_name)? {
            interfaces.push(interface);
        }
    }

    Ok(NetStats { interfaces })
}

fn parse_interface_stats(
    net_stats: &str,
    iface_name: &str,
) -> Result<Option<NetIntf>, eyre::Error> {
    let line = match net_stats
        .lines()
        .find(|line| line.trim_ascii_start().starts_with(iface_name))
    {
        Some(line) => line,
        None => return Ok(None), // Interface not found
    };

    let values = line
        .split_whitespace()
        .skip(1)
        .map(str::parse)
        .collect::<Result<Vec<u64>, _>>()?;

    if values.len() < 11 {
        return Err(eyre!(
            "unknown /proc/net/dev format for interface {}",
            iface_name
        ));
    }

    Ok(Some(NetIntf {
        name: iface_name.to_string(),
        rx_bytes: values[0],
        rx_packets: values[1],
        rx_errors: values[2],
        tx_bytes: values[8],
        tx_packets: values[9],
        tx_errors: values[10],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_poll_net_stats_wlan_only() {
        let proc_net_dev = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes      packets errs drop fifo frame compressed multicast|bytes     packets errs drop fifo colls carrier compressed
    lo: 351106997 3114910    0    0    0     0          0         0 351106997 3114910    0    0    0     0       0          0
dummy0:         0       0    0    0    0     0          0         0         0       0    0    0    0     0       0          0
  can0: 73177394  3672003    0    0    0     0          0         0   749643    49279    0    0    0     0       0          0
 wlan0: 583824134  881197    1    0    0     0          0         0 992486687  776785    2    0    0     0       0          0
        "#;

        let net_stats = parse_net_stats(proc_net_dev).unwrap();
        assert_eq!(net_stats.interfaces.len(), 1);

        let wlan_interface = &net_stats.interfaces[0];
        assert_eq!(wlan_interface.name, IFACE_WLAN0);
        assert_eq!(wlan_interface.tx_bytes, 992486687);
        assert_eq!(wlan_interface.tx_packets, 776785);
        assert_eq!(wlan_interface.tx_errors, 2);
        assert_eq!(wlan_interface.rx_bytes, 583824134);
        assert_eq!(wlan_interface.rx_packets, 881197);
        assert_eq!(wlan_interface.rx_errors, 1);
    }

    #[tokio::test]
    async fn test_poll_net_stats_both_interfaces() {
        let proc_net_dev = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes      packets errs drop fifo frame compressed multicast|bytes     packets errs drop fifo colls carrier compressed
    lo: 351106997 3114910    0    0    0     0          0         0 351106997 3114910    0    0    0     0       0          0
dummy0:         0       0    0    0    0     0          0         0         0       0    0    0    0     0       0          0
  can0: 73177394  3672003    0    0    0     0          0         0   749643    49279    0    0    0     0       0          0
 wlan0: 583824134  881197    1    0    0     0          0         0 992486687  776785    2    0    0     0       0          0
 wwan0: 123456789  654321    3    0    0     0          0         0 987654321  321987    4    0    0     0       0          0
        "#;

        let net_stats = parse_net_stats(proc_net_dev).unwrap();
        assert_eq!(net_stats.interfaces.len(), 2);

        // Find wlan0 interface
        let wlan_interface = net_stats
            .interfaces
            .iter()
            .find(|i| i.name == IFACE_WLAN0)
            .expect("wlan0 interface should be present");
        assert_eq!(wlan_interface.tx_bytes, 992486687);
        assert_eq!(wlan_interface.tx_packets, 776785);
        assert_eq!(wlan_interface.tx_errors, 2);
        assert_eq!(wlan_interface.rx_bytes, 583824134);
        assert_eq!(wlan_interface.rx_packets, 881197);
        assert_eq!(wlan_interface.rx_errors, 1);

        // Find wwan0 interface
        let wwan_interface = net_stats
            .interfaces
            .iter()
            .find(|i| i.name == IFACE_WWAN0)
            .expect("wwan0 interface should be present");
        assert_eq!(wwan_interface.tx_bytes, 987654321);
        assert_eq!(wwan_interface.tx_packets, 321987);
        assert_eq!(wwan_interface.tx_errors, 4);
        assert_eq!(wwan_interface.rx_bytes, 123456789);
        assert_eq!(wwan_interface.rx_packets, 654321);
        assert_eq!(wwan_interface.rx_errors, 3);
    }

    #[tokio::test]
    async fn test_poll_net_stats_no_target_interfaces() {
        let proc_net_dev = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes      packets errs drop fifo frame compressed multicast|bytes     packets errs drop fifo colls carrier compressed
    lo: 351106997 3114910    0    0    0     0          0         0 351106997 3114910    0    0    0     0       0          0
dummy0:         0       0    0    0    0     0          0         0         0       0    0    0    0     0       0          0
  can0: 73177394  3672003    0    0    0     0          0         0   749643    49279    0    0    0     0       0          0
        "#;

        let net_stats = parse_net_stats(proc_net_dev).unwrap();
        assert_eq!(net_stats.interfaces.len(), 0);
    }
}
