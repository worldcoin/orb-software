pub mod common;

use async_tempfile::TempDir;
use bytes::Bytes;
use camino::Utf8Path;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::TryStreamExt as _;

use orb_s3_helpers::{upload_dir::upload_dir, ClientExt, S3Uri};

use common::TestCtx;

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn test_upload_dir() -> Result<()> {
    let _ = color_eyre::install();

    let ctx = TestCtx::new().await?;
    let client = ctx.client();

    const BUCKET: &str = "mybucket";
    ctx.mk_bucket(BUCKET).await?;

    // Create temp dir with some files.
    let temp_dir = TempDir::new().await?;
    let base = Utf8Path::from_path(temp_dir.dir_path()).unwrap();

    // files
    let file_a = base.join("fileA.txt");
    let subdir = base.join("nested");
    tokio::fs::create_dir(&subdir).await?;
    let file_b = subdir.join("fileB.bin");

    tokio::fs::write(&file_a, b"Hello A").await?;
    tokio::fs::write(&file_b, vec![1u8, 2, 3, 4]).await?;

    let prefix: S3Uri = format!("s3://{BUCKET}/uploads/").parse().unwrap();

    upload_dir(client, base, &prefix, Some(2)).await?;

    // Verify objects exist.
    let objs: Vec<_> = client
        .list_prefix(&prefix)
        .try_collect()
        .await?;
    assert_eq!(objs.len(), 2);

    // Verify contents for one file.
    let key_a = format!("{}{}", prefix.key, "fileA.txt");
    let body = client
        .get_object()
        .bucket(BUCKET)
        .key(&key_a)
        .send()
        .await
        .wrap_err("failed get object")?
        .body
        .collect()
        .await?;
    assert_eq!(body.into_bytes(), Bytes::from_static(b"Hello A"));

    Ok(())
}
