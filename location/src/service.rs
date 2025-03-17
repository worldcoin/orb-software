use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument};

use crate::data::{WifiNetwork, CellularInfo};
use crate::errors::Result;

/// Trait defining a service that can be started and shutdown gracefully
pub trait Service: Send + Sync + 'static {
    fn start(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
    
    fn shutdown(&self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
    
    fn join(&self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// Data collector service trait - collects WiFi and/or cellular data
pub trait DataCollector: Send + Sync + 'static {
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
    
    fn get_wifi_networks(&self) -> Pin<Box<dyn Future<Output = Vec<WifiNetwork>> + Send>>;
    
    fn get_cellular_info(&self) -> Pin<Box<dyn Future<Output = Option<CellularInfo>> + Send>>;
}

/// Status reporter service trait - reports collected data to backend
pub trait StatusReporter: Send + Sync + 'static {
    /// Send status update to backend
    fn send_status(&self, 
                  wifi_networks: &[WifiNetwork], 
                  cellular_info: Option<&CellularInfo>) 
        -> Pin<Box<dyn Future<Output = Result<String>> + Send>>;
}

/// Basic service implementation that can be used as a base for other services
pub struct BaseService {
    pub name: String,
    pub cancel_token: CancellationToken,
    pub shutdown_complete: Arc<Mutex<bool>>,
}

impl BaseService {
    /// Create a new base service with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cancel_token: CancellationToken::new(),
            shutdown_complete: Arc::new(Mutex::new(false)),
        }
    }
    
    /// Setup signal handlers for graceful shutdown
    #[instrument(skip(self))]
    pub fn setup_signal_handlers(&self) {
        let token = self.cancel_token.clone();
        let service_name = self.name.clone();
        
        tokio::spawn(async move {
            let mut sigint = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::interrupt()
            ).unwrap();
            let mut sigterm = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate()
            ).unwrap();
            
            tokio::select! {
                _ = sigint.recv() => {
                    info!(service = %service_name, "Received SIGINT, shutting down gracefully");
                }
                _ = sigterm.recv() => {
                    info!(service = %service_name, "Received SIGTERM, shutting down gracefully");
                }
            }
            
            token.cancel();
        });
    }
    
    /// Helper to run an operation with retry logic
    #[instrument(skip_all)]
    pub async fn with_retry<T, F, Fut>(
        &self,
        operation_name: &str,
        max_retries: u32,
        operation: F,
    ) -> Result<T> 
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        for attempt in 0..=max_retries {
            if attempt > 0 {
                debug!(
                    service = %self.name,
                    operation = %operation_name,
                    attempt = attempt + 1, 
                    "Retrying operation"
                );
                tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
            }
            
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if attempt == max_retries {
                        error!(
                            service = %self.name,
                            operation = %operation_name,
                            error = %e,
                            "Operation failed after multiple attempts"
                        );
                        return Err(e);
                    } else {
                        debug!(
                            service = %self.name,
                            operation = %operation_name,
                            error = %e,
                            "Operation failed, will retry"
                        );
                    }
                }
            }
        }
        
        // This should be unreachable due to the loop structure
        unreachable!()
    }
} 
