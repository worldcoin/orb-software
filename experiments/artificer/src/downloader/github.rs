use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use futures::{StreamExt, TryStreamExt};
use tokio::io::AsyncRead;
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::config::sources;

use super::Client;

pub async fn download_artifact(
    client: &Client,
    source: sources::Github,
) -> Result<(Box<dyn AsyncRead + Send>, u64)> {
    let Some((owner, repo)) = source.repo.split_once('/') else {
        bail!("invalid repo url: expected `/` but was not present");
    };
    let repos = client.octo.repos(owner, repo);
    let release = repos
        .releases()
        .get_by_tag(&source.tag)
        .await
        .wrap_err("could not get release")?;
    let Some(ass) = release.assets.iter().find(|a| a.name == source.artifact) else {
        bail!("no asset named {} found", source.artifact);
    };
    let url = ass.url.clone();
    let total_bytes = ass.size; // 🍑

    let req = client.reqwest.get(url);
    let req = if let Some(t) = client.gh_token.to_owned() {
        req.bearer_auth(t)
    } else {
        req
    };
    let req = req
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("Accept", "application/octet-stream");

    tracing::debug!("sending request: {req:?}");
    let response = req
        .send()
        .await
        .wrap_err("failed to send download request")?
        .error_for_status()?;

    // convert from stream to tokio reader via `futures` and `tokio_util`
    let reader = response
        .bytes_stream()
        .map(|result| result.map_err(std::io::Error::other))
        .into_async_read()
        .compat();
    Ok((
        Box::new(reader),
        total_bytes.try_into().expect("should have converted"),
    ))
}
