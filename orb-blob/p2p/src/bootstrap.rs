use std::collections::BTreeSet;
use std::time::Duration;

use eyre::Context;
use eyre::Result;
use futures::TryFutureExt;
use iroh::NodeAddr;
use serde::{Deserialize, Serialize};
use tokio::time::error::Elapsed;
use tracing::warn;
use url::Url;

/// Responsible for managing discovery of bootstrapping peers
#[derive(Debug)]
pub struct Bootstrapper {
    /// Hard-coded list of node-ids.
    pub well_known_nodes: Vec<NodeAddr>,
    /// Provides a list of NodeIds via HTTPs that serve lists of NodeIds
    pub well_known_endpoints: Vec<Url>,
    /// Look up a known InfoHash in the DHT, to get a list of ip addresses that can then
    /// be asked for Gossip NodeIDs
    pub use_dht: bool,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
struct BootstrapperDiscoveryMessage {
    bootstrap_node_ids: Vec<NodeAddr>,
}

impl Bootstrapper {
    pub async fn find_bootstrap_peers(
        &self,
        timeout: Duration,
    ) -> Result<BTreeSet<NodeAddr>> {
        let client = reqwest::Client::new();
        let mut futures = Vec::with_capacity(self.well_known_endpoints.len());
        for endpoint in self.well_known_endpoints.clone() {
            let endpoint_clone = endpoint.clone();
            let fut = async {
                let response = client
                    .get(endpoint_clone)
                    .send()
                    .await
                    .wrap_err("failed to retrieve from endpoint")?
                    .json::<BootstrapperDiscoveryMessage>()
                    .await
                    .wrap_err("failed to get message from body")?;

                Ok::<_, eyre::Report>(response)
            };
            futures.push(
                tokio::time::timeout(timeout, fut).map_err(|Elapsed { .. }| endpoint),
            );
        }
        let mut bootstrap_nodes: BTreeSet<NodeAddr> =
            self.well_known_nodes.clone().into_iter().collect();
        for result in futures::future::join_all(futures).await {
            match result {
                Err(endpoint) => warn!("endpoint {endpoint} timed out"),
                Ok(Err(err)) => warn!("{err}"),
                Ok(Ok(msg)) => bootstrap_nodes.extend(msg.bootstrap_node_ids),
            }
        }

        Ok(bootstrap_nodes)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use iroh::SecretKey;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn example_keys(n: u8) -> Vec<SecretKey> {
        (0..n).map(|i| SecretKey::from_bytes(&[i; 32])).collect()
    }

    #[tokio::test]
    async fn test_bootstrap_http() {
        let mut example_nodeids: Vec<_> = example_keys(8)
            .iter()
            .map(SecretKey::public)
            .map(NodeAddr::from)
            .collect();
        let well_known_nodes = example_nodeids.split_off(4);
        let http_nodes = example_nodeids;
        let all_expected_nodes: BTreeSet<NodeAddr> = http_nodes
            .clone()
            .into_iter()
            .chain(well_known_nodes.clone())
            .collect();
        const API_PATH: &str = "/foo/bar";
        // Start a background HTTP server on a random local port
        let mock_server = MockServer::start().await;
        let bootstrap = Bootstrapper {
            well_known_nodes,
            well_known_endpoints: vec![Url::parse(&format!(
                "{}{API_PATH}",
                mock_server.uri()
            ))
            .unwrap()],
            use_dht: false,
        };
        // Arrange the behaviour of the MockServer adding a Mock:
        // when it receives a GET request on '/hello' it will respond with a 200.
        Mock::given(method("GET"))
            .and(path(API_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                BootstrapperDiscoveryMessage {
                    bootstrap_node_ids: http_nodes.clone(),
                },
            ))
            // Mounting the mock on the mock server - it's now effective!
            .mount(&mock_server)
            .await;

        let peers = bootstrap
            .find_bootstrap_peers(Duration::from_millis(10))
            .await
            .expect("failed to request");

        assert_eq!(peers, all_expected_nodes);
    }
}
