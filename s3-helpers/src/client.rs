use std::future::Future;

use aws_config::{
    meta::{credentials::CredentialsProviderChain, region::RegionProviderChain},
    retry::RetryConfig,
    stalled_stream_protection::StalledStreamProtectionConfig,
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
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let region = region_provider.region().await.expect("infallible");
    info!("using aws region: {region}");
    let credentials_provider = CredentialsProviderChain::default_provider().await;
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

    let config = aws_config::defaults(BehaviorVersion::v2025_08_07())
        .region(region_provider)
        .credentials_provider(credentials_provider)
        .retry_config(retry_config)
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .load()
        .await;

    Ok(aws_sdk_s3::Client::new(&config))
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
