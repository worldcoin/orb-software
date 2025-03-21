use aws_sdk_s3::{types::Object, Client};
use color_eyre::eyre::WrapErr as _;
use futures::TryStream;

use crate::s3_url_parts::S3Uri;

/// See [`crate::ClientExt::list_prefix`].
pub(crate) fn list_prefix(
    client: &Client,
    s3_prefix: S3Uri,
) -> impl TryStream<Ok = Object, Error = color_eyre::Report> + Send + Unpin {
    // List objects in the bucket with the given prefix
    let mut paginator = client
        .list_objects_v2()
        .bucket(&s3_prefix.bucket)
        .prefix(&s3_prefix.key)
        .into_paginator()
        .send();

    // Pin it here just to make people's lives easier elsewhere
    Box::pin(async_stream::try_stream! {
        // Process each page of objects
        while let Some(page) = paginator.next().await {
            let page = page.wrap_err("error while listing s3 objects")?;
            for obj in page.contents() {
                let obj = obj.to_owned();
                yield obj;
            }
        }
    })
}
