use eyre::{eyre, Result};
use orb_backend_status_dbus::types::{NetIntf, NetStats};
use tracing::error;

const IFACE_WLAN0: &str = "wlan0";

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
    let values = net_stats
        .lines()
        .find(|line| line.trim_ascii_start().starts_with(IFACE_WLAN0))
        .ok_or_else(|| eyre!("unknown /proc/net/dev format"))?
        .split_whitespace()
        .skip(1)
        .map(str::parse)
        .collect::<Result<Vec<u64>, _>>()?;
    if values.len() < 11 {
        return Err(eyre!("unknown /proc/net/dev format"));
    }

    Ok(NetStats {
        interfaces: vec![NetIntf {
            name: IFACE_WLAN0.to_string(),
            rx_bytes: values[0],
            rx_packets: values[1],
            rx_errors: values[2],
            tx_bytes: values[8],
            tx_packets: values[9],
            tx_errors: values[10],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_poll_net_stats() {
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
        assert_eq!(net_stats.interfaces[0].name, IFACE_WLAN0);
        assert_eq!(net_stats.interfaces[0].tx_bytes, 992486687);
        assert_eq!(net_stats.interfaces[0].tx_packets, 776785);
        assert_eq!(net_stats.interfaces[0].tx_errors, 2);
        assert_eq!(net_stats.interfaces[0].rx_bytes, 583824134);
        assert_eq!(net_stats.interfaces[0].rx_packets, 881197);
        assert_eq!(net_stats.interfaces[0].rx_errors, 1);
    }
}
