//! Helpers for tests.
//!
//! A lot of this was adapted from
//! <https://github.com/testcontainers/testcontainers-rs-modules-community/blob/0b83d15d052f274e84fffaba4f49b5530c550169/examples/localstack.rs>

use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3 as s3;
use color_eyre::{eyre::Context as _, Result};
use orb_s3_helpers::S3Uri;
use testcontainers::{runners::AsyncRunner as _, ContainerAsync, ImageExt as _};
use testcontainers_modules::localstack::LocalStack;

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
        // Set up AWS client
        let endpoint_url = format!("http://{host_ip}:{host_port}");

        // TODO(@thebutlah): See if we can/should use aws_config
        // TODO(@thebutlah): See if we should instead make a general purpose test
        // helper for spinning up not just s3 but a variety of clients from a localstack.
        let creds = s3::config::Credentials::new("fake", "fake", None, None, "test");

        let config = s3::config::Builder::default()
            .behavior_version(BehaviorVersion::v2024_03_28())
            .region(Region::new("us-east-1"))
            .credentials_provider(creds)
            .endpoint_url(endpoint_url)
            .force_path_style(true)
            .build();

        let client = s3::Client::from_conf(config);

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
        contents: Option<Vec<u8>>,
    ) -> Result<S3Uri> {
        let uri = S3Uri {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        };
        let builder = self.client.put_object().bucket(bucket).key(key);
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
