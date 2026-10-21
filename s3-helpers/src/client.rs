use std::future::Future;

use aws_config::{
    default_provider::credentials::DefaultCredentialsChain,
    environment::region::EnvironmentVariableRegionProvider,
    imds::region::ImdsRegionProvider, meta::region::RegionProviderChain,
    profile::region::ProfileFileRegionProvider, provider_config::ProviderConfig,
    retry::RetryConfig, stalled_stream_protection::StalledStreamProtectionConfig,
    BehaviorVersion,
};
use aws_sdk_s3::{config::ProvideCredentials, types::Object};
use camino::Utf8Path;
use color_eyre::{eyre::WrapErr as _, Result, Section as _};
use futures::TryStream;
use tracing::info;

use crate::{
    download::Progress, s3_uri::S3Uri, upload::UploadProgress, ExistingFileBehavior,
    ExistingObjectBehavior,
};

const TIMEOUT_RETRY_ATTEMPTS: u32 = 5;

/// Helper function for setting up aws credentials.
pub async fn client() -> Result<aws_sdk_s3::Client> {
    let http_client = http_client();
    let provider_config =
        ProviderConfig::without_region().with_http_client(http_client.clone());
    let region_provider =
        RegionProviderChain::first_try(EnvironmentVariableRegionProvider::new())
            .or_else(ProfileFileRegionProvider::builder().build())
            .or_else(
                ImdsRegionProvider::builder()
                    .configure(&provider_config)
                    .build(),
            )
            .or_else("us-east-1");
    let region = region_provider.region().await.expect("infallible");
    info!("using aws region: {region}");
    let credentials_provider = DefaultCredentialsChain::builder()
        .configure(provider_config)
        .region(region.clone())
        .build()
        .await;
    let _creds = credentials_provider
        .provide_credentials()
        .await
        .wrap_err("failed to get aws credentials")
        .with_note(|| {
            format!("AWS_PROFILE env var was {:?}", std::env::var("AWS_PROFILE"))
        })
        .with_suggestion(|| {
            "make sure that your aws credentials are set. Follow the instructions at
            https://worldcoin.github.io/orb-software/aws-creds"
        })
        .with_suggestion(|| {
            "try running `AWS_PROFILE=<profile> aws sso login` to refresh your \
            credentials"
        })?;

    let retry_config =
        RetryConfig::standard().with_max_attempts(TIMEOUT_RETRY_ATTEMPTS);

    let config = aws_config::defaults(BehaviorVersion::v2026_01_12())
        .http_client(http_client)
        .region(region_provider)
        .credentials_provider(credentials_provider)
        .retry_config(retry_config)
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .load()
        .await;

    Ok(aws_sdk_s3::Client::new(&config))
}

fn http_client() -> aws_sdk_s3::config::SharedHttpClient {
    use aws_smithy_http_client::tls::{rustls_provider::CryptoMode, Provider};

    aws_smithy_http_client::Builder::new()
        .tls_provider(Provider::Rustls(CryptoMode::Ring))
        .build_https()
}

/// Extension trait with several utility helper functions using the aws client.
pub trait ClientExt {
    /// Lists all s3 objects under some prefix.
    fn list_prefix(
        &self,
        s3_prefix: &S3Uri,
    ) -> impl TryStream<Ok = Object, Error = color_eyre::Report> + Send + Unpin;

    fn download_multipart(
        &self,
        s3_uri: &S3Uri,
        out_path: &Utf8Path,
        existing_file_behavior: ExistingFileBehavior,
        progress: impl FnMut(Progress) + Send + Unpin,
    ) -> impl Future<Output = Result<()>> + Send + Unpin;

    fn upload_multipart(
        &self,
        s3_uri: &S3Uri,
        in_path: &Utf8Path,
        existing_object_behavior: ExistingObjectBehavior,
        progress: impl FnMut(UploadProgress) + Send + Unpin,
    ) -> impl Future<Output = Result<()>> + Send + Unpin;
}

impl ClientExt for aws_sdk_s3::Client {
    fn list_prefix(
        &self,
        s3_prefix: &S3Uri,
    ) -> impl TryStream<Ok = Object, Error = color_eyre::Report> + Send + Unpin {
        crate::list_prefix::list_prefix(self, s3_prefix)
    }

    fn download_multipart(
        &self,
        s3_uri: &S3Uri,
        out_path: &Utf8Path,
        existing_file_behavior: ExistingFileBehavior,
        progress: impl FnMut(Progress) + Send,
    ) -> impl Future<Output = Result<()>> + Send + Unpin {
        // pinning ensures less pain for others
        Box::pin(crate::download::download_multipart(
            self,
            s3_uri,
            out_path,
            existing_file_behavior,
            progress,
        ))
    }

    fn upload_multipart(
        &self,
        s3_uri: &S3Uri,
        in_path: &Utf8Path,
        existing_object_behavior: ExistingObjectBehavior,
        progress: impl FnMut(UploadProgress) + Send,
    ) -> impl Future<Output = Result<()>> + Send + Unpin {
        Box::pin(crate::upload::upload_multipart(
            self,
            s3_uri,
            in_path,
            existing_object_behavior,
            progress,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aws_config::{BehaviorVersion, Region};
    use aws_sdk_s3::config::Credentials;
    use color_eyre::Result;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    #[tokio::test]
    async fn default_credentials_client_uses_modern_tls() -> Result<()> {
        if let Ok(expected) = std::env::var("ORB_S3_CREDENTIALS_TEST") {
            let result = super::client().await;
            assert_eq!(result.is_ok(), expected == "success", "{result:?}");

            return Ok(());
        }
        for expected in ["success", "missing"] {
            let mut command = tokio::process::Command::new(std::env::current_exe()?);
            command
                .args([
                    "--exact",
                    "client::tests::default_credentials_client_uses_modern_tls",
                    "--nocapture",
                ])
                .env_clear()
                .env("ORB_S3_CREDENTIALS_TEST", expected)
                .env("AWS_REGION", "us-east-1")
                .env("AWS_EC2_METADATA_DISABLED", "true")
                .env("AWS_CONFIG_FILE", "/dev/null")
                .env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null")
                .kill_on_drop(true);
            if expected == "success" {
                command
                    .env("AWS_ACCESS_KEY_ID", "test")
                    .env("AWS_SECRET_ACCESS_KEY", "test");
            }
            let output = tokio::time::timeout(Duration::from_secs(5), command.output())
                .await??;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn modern_client_sends_s3_request() -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).await?;
                assert_ne!(read, 0, "request ended before its headers");
                request.extend_from_slice(&buffer[..read]);
            }
            assert!(request.starts_with(b"GET /"));
            let body = "<ListAllMyBucketsResult><Buckets/></ListAllMyBucketsResult>";
            socket.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            ).as_bytes()).await?;

            Ok::<_, std::io::Error>(())
        });
        let config = aws_sdk_s3::config::Builder::default()
            .behavior_version(BehaviorVersion::v2026_01_12())
            .http_client(super::http_client())
            .region(Region::new("us-east-1"))
            .credentials_provider(Credentials::new("test", "test", None, None, "test"))
            .endpoint_url(format!("http://{address}"))
            .force_path_style(true)
            .build();
        let client = aws_sdk_s3::Client::from_conf(config);
        let result =
            tokio::time::timeout(Duration::from_secs(5), client.list_buckets().send())
                .await??;
        assert!(result.buckets().is_empty());
        server.await??;

        Ok(())
    }
}
