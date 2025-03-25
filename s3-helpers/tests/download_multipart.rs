pub mod common;

use std::{path::Path, time::Duration};

use async_tempfile::TempDir;
use aws_sdk_s3::Client;
use bytes::Bytes;
use color_eyre::{
    eyre::{ensure, WrapErr as _},
    Report, Result,
};
use orb_s3_helpers::{ClientExt, ExistingFileBehavior, Progress, S3Uri};
use tokio::time::timeout;

use common::{compare_file_to_buf, TestCtx};

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

    // Act + Assert
    let _: Report = download_and_check(
        client,
        &"s3://doesnt/exist".parse().unwrap(),
        &tmpdir.dir_path().join("doesntmatter"),
        ExistingFileBehavior::Abort,
    )
    .await
    .expect_err("should fail because no such object exists");

    // Arrange
    ctx.mk_bucket(NEW_BUCKET).await?;
    let a_content = Bytes::from(vec![b'a'; 69]);
    const A_KEY: &str = "A";
    let a_uri = mk_obj(&ctx, &a_content, A_KEY).await?;
    let a_path = tmpdir.dir_path().join(A_KEY);

    // Act + Assert
    // Check that A downloads
    {
        download_and_check(client, &a_uri, &a_path, ExistingFileBehavior::Abort)
            .await
            .wrap_err("failed to get file `a` that we just uploaded")?;
        let a_file = tokio::fs::File::open(&a_path)
            .await
            .wrap_err("failed to open file `a`")?;
        compare_file_to_buf(a_file, &a_content)
            .await
            .wrap_err("downloaded file didn't match expected content")?;
    }
    // confirm A fails to download due to existing file
    {
        ensure!(
            download_and_check(client, &a_uri, &a_path, ExistingFileBehavior::Abort)
                .await
                .is_err(),
            "should always fail because file path exists already"
        );
        let a_file = tokio::fs::File::open(&a_path)
            .await
            .wrap_err("failed to open file `a`")?;
        compare_file_to_buf(a_file, &a_content).await.wrap_err(
            "`a`'s contents were not preserved even though we requested an abort",
        )?;
    }
    // Confirm that A successfully downloads if we override
    {
        download_and_check(client, &a_uri, &a_path, ExistingFileBehavior::Overwrite)
            .await
            .wrap_err("filed to download file `a`")?;
        let a_file = tokio::fs::File::open(&a_path)
            .await
            .wrap_err("failed to open file `a`")?;
        compare_file_to_buf(a_file, &a_content)
            .await
            .wrap_err("downloaded file `a` didn't match expected content")?;
    }

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
    let a_uri = mk_obj(&ctx, &a_content, KEY_A).await?;
    let a_path = tmpdir.dir_path().join(KEY_A);

    // Act + Assert
    // Check that A downloads
    {
        download_and_check(client, &a_uri, &a_path, ExistingFileBehavior::Abort)
            .await
            .wrap_err("failed to get file `a` that we just uploaded")?;
        let a_file = tokio::fs::File::open(&a_path)
            .await
            .wrap_err("failed to open file `a`")?;
        compare_file_to_buf(a_file, &a_content)
            .await
            .wrap_err("downloaded file didn't match expected content")?;
    }

    // Arrange
    const KEY_B: &str = "B";
    let b_uri: S3Uri = format!("s3://{NEW_BUCKET}/{KEY_B}").parse().unwrap();
    let b_path = tmpdir.dir_path().join(KEY_B);

    // confirm B fails to download (doesn't exist)
    {
        ensure!(
            download_and_check(
                client,
                &b_uri,
                &b_path,
                ExistingFileBehavior::Overwrite
            )
            .await
            .is_err(),
            "should always fail because file `b` doesn't exist yet"
        );

        ensure!(
            !tokio::fs::try_exists(&b_path).await.unwrap(),
            "the download failed, so the file should not have been created"
        );
    }

    // Arrange: Upload `B`
    let b_content = Bytes::from(vec![b'b'; 96]);
    let new_uri = mk_obj(&ctx, &b_content, KEY_B).await?;
    assert_eq!(new_uri, b_uri, "sanity");

    // Act + Assert
    // Confirm that A successfully downloads still
    {
        download_and_check(client, &a_uri, &a_path, ExistingFileBehavior::Overwrite)
            .await
            .wrap_err("filed to download file `a`")?;
        let a_file = tokio::fs::File::open(&a_path)
            .await
            .wrap_err("failed to open file `a`")?;
        compare_file_to_buf(a_file, &a_content)
            .await
            .wrap_err("downloaded file `a` didn't match expected content")?;
    }

    // confirm B downloads
    {
        download_and_check(client, &b_uri, &b_path, ExistingFileBehavior::Abort)
            .await
            .wrap_err("filed to download file `b`")?;
        let b_file = tokio::fs::File::open(&b_path)
            .await
            .wrap_err("failed to open file `b`")?;
        compare_file_to_buf(b_file, &b_content)
            .await
            .wrap_err("downloaded file `b` didn't match expected content")?;
    }

    Ok(())
}

async fn mk_obj(ctx: &TestCtx, bytes: &Bytes, key: &'static str) -> Result<S3Uri> {
    let new_uri = ctx
        .mk_object(NEW_BUCKET, key, Some(bytes.clone()))
        .await
        .wrap_err("failed to upload file")?;
    assert_eq!(
        new_uri,
        format!("s3://{NEW_BUCKET}/{key}").parse().unwrap(),
        "sanity"
    );
    Ok(new_uri)
}

async fn download_and_check(
    client: &Client,
    uri: &S3Uri,
    out: &Path,
    overwrite: ExistingFileBehavior,
) -> Result<()> {
    let mut total_downloaded = 0;
    let mut progress_call_count = 0;
    timeout(
        Duration::from_secs(10),
        client.download_multipart(
            uri,
            out.try_into().unwrap(),
            overwrite,
            |Progress {
                 bytes_so_far,
                 total_to_download,
             }| {
                println!("bytes so far: {bytes_so_far}");
                progress_call_count += 1;
                assert!(
                    bytes_so_far <= total_to_download,
                    "bytes_so_far should be no bigger than the total_to_download"
                );
                assert!(
                    total_downloaded <= bytes_so_far,
                    "bytes_so_far should be monotonic"
                );
                total_downloaded = bytes_so_far;
            },
        ),
    )
    .await
    .wrap_err("multipart_download timed out")?
    .wrap_err("download failed")?;
    ensure!(
        progress_call_count > 0,
        "progress should be called at least once"
    );
    ensure!(
        tokio::fs::try_exists(&out).await.unwrap(),
        "file should be created at output path"
    );
    let file_len = tokio::fs::metadata(out)
        .await
        .wrap_err("failed to get metadata")?
        .len();
    ensure!(
        file_len == total_downloaded,
        "total downloaded should match file len"
    );

    Ok(())
}
