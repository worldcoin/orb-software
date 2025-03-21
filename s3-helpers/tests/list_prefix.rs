mod common;

use aws_sdk_s3::types::Object;
use color_eyre::{Report, Result};
use futures::TryStreamExt as _;
use orb_s3_helpers::ClientExt;

use common::TestCtx;

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn test_list_prefix() -> Result<()> {
    color_eyre::install()?;
    let ctx = TestCtx::new().await?;
    let client = ctx.client();

    // Assert
    let _: Report = client
        .list_prefix("s3://nonexistant/".parse().unwrap())
        .try_next()
        .await
        .expect_err("nonexistant buckets should not list");

    // Act
    const NEW_BUCKET: &str = "new-bucket";
    ctx.mk_bucket(NEW_BUCKET).await?;
    const KEY_A: &str = "A";
    ctx.mk_object(NEW_BUCKET, KEY_A, None).await?;

    // Assert
    let objs: Vec<Object> = client
        .list_prefix(format!("s3://{NEW_BUCKET}/").parse().unwrap())
        .try_collect()
        .await
        .expect("there is a matching bucket");
    assert_eq!(objs.len(), 1, "only 1 matching object");
    assert_eq!(objs[0].key.as_deref(), Some(KEY_A), "key should match");
    let _: Report = client
        .list_prefix("s3://nonexistant/".parse().unwrap())
        .try_next()
        .await
        .expect_err("nonexistant buckets should not list");

    // Act
    const KEY_B: &str = "B";
    ctx.mk_object(NEW_BUCKET, KEY_B, None).await?;

    // Assert
    let objs: Vec<Object> = client
        .list_prefix(format!("s3://{NEW_BUCKET}/").parse().unwrap())
        .try_collect()
        .await
        .expect("there is a matching bucket");
    assert_eq!(objs.len(), 2, "only 2 matching objects");
    assert_eq!(
        objs.iter()
            .filter(|o| o.key.as_deref() == Some(KEY_A))
            .count(),
        1,
        "only one A should match"
    );
    assert_eq!(
        objs.iter()
            .filter(|o| o.key.as_deref() == Some(KEY_B))
            .count(),
        1,
        "only one B should match"
    );
    let _: Report = client
        .list_prefix("s3://nonexistant/".parse().unwrap())
        .try_next()
        .await
        .expect_err("nonexistant buckets should not list");

    Ok(())
}
