use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::data::WifiNetwork;

#[derive(Clone, Debug)]
pub struct TimestampedNetwork {
    pub network: WifiNetwork,
    pub last_seen: Instant,
}

/// Manager for the in-memory network database
#[derive(Clone)]
pub struct NetworkManager {
    networks: Arc<Mutex<HashMap<String, TimestampedNetwork>>>,
    expiry_duration: Duration,
}

impl NetworkManager {
    pub fn new(expiry_seconds: u64) -> Self {
        NetworkManager {
            networks: Arc::new(Mutex::new(HashMap::new())),
            expiry_duration: Duration::from_secs(expiry_seconds),
        }
    }

    // Update networks with new scan results
    pub async fn update_networks(&self, new_networks: Vec<WifiNetwork>) -> usize {
        let now = Instant::now();
        let mut networks = self.networks.lock().await;
        let mut new_count = 0;
        
        for network in new_networks {
            let bssid = network.bssid.clone();
            if !networks.contains_key(&bssid) {
                new_count += 1;
                debug!(ssid = network.ssid, bssid = bssid, "New network found");
            }
            
            networks.insert(bssid, TimestampedNetwork {
                network,
                last_seen: now,
            });
        }
        
        new_count
    }
    
    // Clean up expired networks
    pub async fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let mut networks = self.networks.lock().await;
        let before_count = networks.len();
        
        networks.retain(|_, timestamped| {
            now.duration_since(timestamped.last_seen) < self.expiry_duration
        });
        
        let removed_count = before_count - networks.len();
        if removed_count > 0 {
            info!(removed = removed_count, remaining = networks.len(), "Cleaned up expired networks");
        }
        
        removed_count
    }
    
    // Get all current (non-expired) networks
    pub async fn get_current_networks(&self) -> Vec<WifiNetwork> {
        let now = Instant::now();
        let networks = self.networks.lock().await;
        
        let result = networks
            .values()
            .filter(|timestamped| {
                now.duration_since(timestamped.last_seen) < self.expiry_duration
            })
            .map(|timestamped| timestamped.network.clone())
            .collect::<Vec<WifiNetwork>>();
            
        debug!(count = result.len(), "Retrieved current non-expired networks");
        result
    }
    
    pub async fn network_count(&self) -> usize {
        self.networks.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    
    fn create_test_network(ssid: &str, bssid: &str, signal: i32) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.to_string(),
            bssid: bssid.to_string(),
            signal_level: signal,
            frequency: 2412,
            flags: "".to_string(),
        }
    }
    
    #[test]
    fn test_update_networks() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create a new NetworkManager with a short expiry time
            let manager = NetworkManager::new(1);
            
            // Create some test networks
            let networks = vec![
                create_test_network("Test1", "00:11:22:33:44:55", -50),
                create_test_network("Test2", "00:11:22:33:44:66", -60),
            ];
            
            // Update the manager with the networks
            let new_count = manager.update_networks(networks.clone()).await;
            
            // Both networks should be added as new
            assert_eq!(new_count, 2);
            
            // Check the total count
            assert_eq!(manager.network_count().await, 2);
            
            // Update with one new network and one existing network
            let networks2 = vec![
                create_test_network("Test2", "00:11:22:33:44:66", -65), // Changed signal
                create_test_network("Test3", "00:11:22:33:44:77", -70),  // New network
            ];
            
            let new_count2 = manager.update_networks(networks2).await;
            
            // Only one new network should be added
            assert_eq!(new_count2, 1);
            
            // Check the total count
            assert_eq!(manager.network_count().await, 3);
            
            // Get current networks
            let current = manager.get_current_networks().await;
            
            // Should have 3 networks
            assert_eq!(current.len(), 3);
            
            let test2 = current.iter().find(|n| n.bssid == "00:11:22:33:44:66").unwrap();
            assert_eq!(test2.signal_level, -65);
        });
    }
    
    #[test]
    fn test_cleanup_expired() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            // Create a new NetworkManager with a very short expiry time (1 second)
            let manager = NetworkManager::new(1);
            
            // Create some test networks
            let networks = vec![
                create_test_network("Test1", "00:11:22:33:44:55", -50),
                create_test_network("Test2", "00:11:22:33:44:66", -60),
            ];
            
            // Update the manager with the networks
            manager.update_networks(networks).await;
            
            // Should have 2 networks
            assert_eq!(manager.network_count().await, 2);
            
            // Wait for expiry time
            tokio::time::sleep(Duration::from_secs(2)).await;
            
            // Clean up expired networks
            let removed = manager.cleanup_expired().await;
            
            // Both networks should be removed
            assert_eq!(removed, 2);
            
            // Check the total count
            assert_eq!(manager.network_count().await, 0);
            
            // Add a new network after cleanup
            let new_networks = vec![
                create_test_network("Test3", "00:11:22:33:44:77", -70),
            ];
            
            manager.update_networks(new_networks).await;
            
            // Should have 1 network again
            assert_eq!(manager.network_count().await, 1);
        });
    }
} 
