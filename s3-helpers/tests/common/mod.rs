//! Helpers for tests.

use std::time::Duration;

use aws_config::{timeout::TimeoutConfig, BehaviorVersion, Region};
use aws_sdk_s3 as s3;
use bytes::Bytes;
use color_eyre::{
    eyre::{bail, ensure, Context as _},
    Result,
};
use orb_s3_helpers::S3Uri;
use testcontainers::{runners::AsyncRunner as _, ContainerAsync, ImageExt as _};
use testcontainers_modules::localstack::LocalStack;
use tokio::{io::AsyncReadExt as _, net::ToSocketAddrs};

/// A lot of this was adapted from
/// <https://github.com/testcontainers/testcontainers-rs-modules-community/blob/0b83d15d052f274e84fffaba4f49b5530c550169/examples/localstack.rs>
#[derive(Debug)]
pub struct TestCtx {
    client: s3::Client,
    _localstack: ContainerAsync<LocalStack>,
}

impl TestCtx {
    pub async fn new() -> Result<Self> {
        let request = LocalStack::default().with_env_var("SERVICES", "s3");
        let container = request
            .start()
            .await
            .wrap_err("failed to start testcontainer for localstack")?;

        let host_ip = container.get_host().await?;
        let host_port = container.get_host_port_ipv4(4566).await?;
        let addr = format!("{host_ip}:{host_port}");
        let endpoint_url = format!("http://{addr}");

        // TODO(@thebutlah): See if we can/should use aws_config
        // TODO(@thebutlah): See if we should instead make a general purpose test
        // helper for spinning up not just s3 but a variety of clients from a localstack.
        let creds = s3::config::Credentials::new("fake", "fake", None, None, "test");

        let config = s3::config::Builder::default()
            .timeout_config(
                TimeoutConfig::builder()
                    .operation_timeout(Duration::from_secs(10))
                    .build(),
            )
            .behavior_version(BehaviorVersion::v2024_03_28())
            .region(Region::new("us-east-1"))
            .credentials_provider(creds)
            .endpoint_url(endpoint_url)
            .force_path_style(true)
            .build();

        let client = s3::Client::from_conf(config);
        // avoids race condition where the tcp connection might be
        // refused
        wait_for_tcp(Duration::from_millis(1000), addr)
            .await
            .wrap_err("timed out waiting for tcp")?;
        client.list_buckets().max_buckets(1).send().await.wrap_err(
            "failed to list buckets as sanity check that localstack is running",
        )?;

        Ok(Self {
            client,
            _localstack: container,
        })
    }

    pub fn client(&self) -> &s3::Client {
        &self.client
    }

    pub async fn mk_bucket(&self, name: &str) -> Result<S3Uri> {
        let uri = S3Uri {
            bucket: name.to_owned(),
            key: String::new(),
        };

        self.client
            .create_bucket()
            .bucket(name)
            .send()
            .await
            .wrap_err_with(|| format!("failed to create bucket at {uri}"))?;

        Ok(uri)
    }

    pub async fn mk_object(
        &self,
        bucket: &str,
        key: &str,
        contents: Option<Bytes>,
    ) -> Result<S3Uri> {
        let uri = S3Uri {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        };
        let builder = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .if_none_match("*");
        let builder = if let Some(contents) = contents {
            builder.body(contents.into())
        } else {
            builder
        };
        builder
            .send()
            .await
            .wrap_err_with(|| format!("failed to create object at {uri}"))?;

        Ok(uri)
    }
}

pub async fn compare_file_to_buf(
    mut file: tokio::fs::File,
    compare_to: &[u8],
) -> Result<()> {
    // Compare file contents with original data in chunks
    let mut buf = vec![0u8; 8 * 1024]; // 8KiB chunks
    let mut pos = 0;
    loop {
        let n = file.read(&mut buf).await.wrap_err("failed to read file")?;
        if n == 0 {
            // EOF
            ensure!(
                pos == compare_to.len(),
                "file length mismatch: got {pos}, expected {}",
                compare_to.len()
            );
            break;
        }
        let Some(region) = compare_to.get(pos..pos + n) else {
            bail!(
                "file longer than expected: got {}, expected {}",
                pos + n,
                compare_to.len()
            )
        };
        ensure!(buf[..n] == *region, "content mismatch at position {pos}");
        pos += n;
    }

    Ok(())
}

async fn wait_for_tcp(timeout: Duration, addr: impl ToSocketAddrs) -> Result<()> {
    tokio::time::timeout(timeout, async {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                break;
            }
        }
    })
    .await
    .wrap_err("timed out")
}
