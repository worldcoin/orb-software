use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::{debug, info};

use crate::data::WifiNetwork;

// Structure to store a WiFi network with its last seen timestamp
#[derive(Clone, Debug)]
pub struct TimestampedNetwork {
    pub network: WifiNetwork,
    pub last_seen: Instant,
}

// Manager for the in-memory network database
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
    pub fn update_networks(&self, new_networks: Vec<WifiNetwork>) -> usize {
        let now = Instant::now();
        let mut networks = self.networks.lock().unwrap();
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
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let mut networks = self.networks.lock().unwrap();
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
    pub fn get_current_networks(&self) -> Vec<WifiNetwork> {
        let now = Instant::now();
        let networks = self.networks.lock().unwrap();
        
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
    
    // Get count of networks in memory
    pub fn network_count(&self) -> usize {
        self.networks.lock().unwrap().len()
    }
} 
