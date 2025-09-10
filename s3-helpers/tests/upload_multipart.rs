pub mod common;

use std::{path::Path, time::Duration};

use async_tempfile::TempDir;
use aws_sdk_s3::Client;
use bytes::Bytes;
use color_eyre::{
    eyre::{ensure, WrapErr as _},
    Result,
};
use orb_s3_helpers::{ClientExt as _, ExistingObjectBehavior, S3Uri, UploadProgress};
use tokio::time::timeout;

use common::TestCtx;

const NEW_BUCKET: &str = "new-bucket";

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn test_overwrite_behavior() -> Result<()> {
    let _ = color_eyre::install();
    let ctx = TestCtx::new().await?;
    let client = ctx.client();
    let tmpdir = TempDir::new().await?;
    println!("temp dir: {}", tmpdir.dir_path().display());

    // Arrange: create a small file locally
    let a_content = Bytes::from(vec![b'a'; 69]);
    let b_content = Bytes::from(vec![b'b'; 1337]);
    let a_path = tmpdir.dir_path().join("A");
    let b_path = tmpdir.dir_path().join("B");
    tokio::fs::write(&a_path, &a_content).await?;
    tokio::fs::write(&b_path, &b_content).await?;
    let uri: S3Uri = format!("s3://{NEW_BUCKET}/foo").parse().unwrap();

    // Act + Assert: should fail since bucket doesn't exist
    ensure!(
        upload_and_check(client, &uri, &a_path, ExistingObjectBehavior::Abort,)
            .await
            .is_err(),
        "should fail because bucket does not exist"
    );

    // Arrange: Create bucket
    ctx.mk_bucket(NEW_BUCKET).await?;

    // Act + Assert: Upload A succeeds with Abort (since it doesn't exist)
    upload_and_check(client, &uri, &a_path, ExistingObjectBehavior::Abort).await?;
    let downloaded = get_object_bytes(client, &uri).await?;
    assert_eq!(downloaded, a_content);

    // Confirm A fails to upload due to existing object when Abort
    ensure!(
        upload_and_check(client, &uri, &a_path, ExistingObjectBehavior::Abort,)
            .await
            .is_err(),
        "should always fail because object exists already"
    );
    // Verify contents preserved
    let downloaded = get_object_bytes(client, &uri).await?;
    assert_eq!(downloaded, a_content);

    // Confirm B fails to upload due to existing object when Abort
    ensure!(
        upload_and_check(client, &uri, &b_path, ExistingObjectBehavior::Abort,)
            .await
            .is_err(),
        "should always fail because object exists already"
    );
    // Verify contents preserved
    let downloaded = get_object_bytes(client, &uri).await?;
    assert_eq!(downloaded, a_content);

    // Confirm that B successfully uploads if we overwrite
    upload_and_check(client, &uri, &b_path, ExistingObjectBehavior::Overwrite).await?;
    let downloaded = get_object_bytes(client, &uri).await?;
    assert_eq!(downloaded, b_content);

    Ok(())
}

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn test_two_files() -> Result<()> {
    let _ = color_eyre::install();
    let ctx = TestCtx::new().await?;
    let client = ctx.client();
    let tmpdir = TempDir::new().await?;
    println!("temp dir: {tmpdir:?}");

    // Arrange: Create bucket
    ctx.mk_bucket(NEW_BUCKET).await?;

    // Arrange: Upload `A`
    const KEY_A: &str = "A";
    let a_content = Bytes::from(vec![b'a'; 69]);
    let a_path = tmpdir.dir_path().join(KEY_A);
    tokio::fs::write(&a_path, &a_content).await?;
    let a_uri: S3Uri = format!("s3://{NEW_BUCKET}/{KEY_A}").parse().unwrap();

    upload_and_check(client, &a_uri, &a_path, ExistingObjectBehavior::Abort).await?;
    assert_eq!(get_object_bytes(client, &a_uri).await?, a_content);

    // Arrange B
    const KEY_B: &str = "B";
    let b_uri: S3Uri = format!("s3://{NEW_BUCKET}/{KEY_B}").parse().unwrap();
    let b_path = tmpdir.dir_path().join(KEY_B);

    // Confirm B fails to upload (no local file yet)
    ensure!(
        upload_and_check(client, &b_uri, &b_path, ExistingObjectBehavior::Overwrite,)
            .await
            .is_err(),
        "should fail because local file does not exist"
    );

    // Arrange: create local file for B
    let b_content = Bytes::from(vec![b'b'; 96]);
    tokio::fs::write(&b_path, &b_content).await?;

    // Act + Assert: Upload B
    upload_and_check(client, &b_uri, &b_path, ExistingObjectBehavior::Abort).await?;
    assert_eq!(get_object_bytes(client, &b_uri).await?, b_content);

    // Act + Assert: Re-upload A with overwrite still succeeds
    upload_and_check(client, &a_uri, &a_path, ExistingObjectBehavior::Overwrite)
        .await?;
    assert_eq!(get_object_bytes(client, &a_uri).await?, a_content);

    Ok(())
}

async fn get_object_bytes(client: &Client, uri: &S3Uri) -> Result<Bytes> {
    let resp = client
        .get_object()
        .bucket(&uri.bucket)
        .key(&uri.key)
        .send()
        .await
        .wrap_err("failed to get object")?;
    let body = resp.body.collect().await?.into_bytes();
    Ok(body)
}

async fn upload_and_check(
    client: &Client,
    uri: &S3Uri,
    input: &Path,
    behavior: ExistingObjectBehavior,
) -> Result<()> {
    let mut total_uploaded = 0u64;
    let mut progress_call_count = 0u64;
    let res = timeout(
        Duration::from_secs(10),
        client.upload_multipart(
            uri,
            input.try_into().unwrap(),
            behavior,
            |UploadProgress {
                 bytes_so_far,
                 total_to_upload,
             }| {
                progress_call_count += 1;
                assert!(
                    bytes_so_far <= total_to_upload,
                    "bytes_so_far should be no bigger than the total to upload"
                );
                assert!(
                    total_uploaded <= bytes_so_far,
                    "bytes_so_far should be monotonic"
                );
                total_uploaded = bytes_so_far;
            },
        ),
    )
    .await
    .wrap_err("multipart_upload timed out")?;

    if let Err(err) = res {
        // If input file doesn't exist, ensure S3 object wasn't created
        if !input.exists() {
            let head = client
                .head_object()
                .bucket(&uri.bucket)
                .key(&uri.key)
                .send()
                .await;
            ensure!(head.is_err(), "object should not have been created");
        }
        return Err(err);
    }

    // On success, ensure progress called at least once and monotonic
    ensure!(
        progress_call_count > 0,
        "progress should be called at least once"
    );
    // No simple local file check (input can be anywhere), progress validated above.

    Ok(())
}
