#![forbid(unsafe_code)]

mod config;
mod downloader;

use std::{
    collections::HashMap,
    fmt::Write,
    path::{Path, PathBuf},
    pin::pin,
    time::Duration,
};

use crate::downloader::Client;
use color_eyre::{eyre::WrapErr, Result};
use config::{sources::Source, ArtifactName};
use indicatif::{ProgressState, ProgressStyle};
use tokio::io::AsyncWrite;

use crate::config::{LockedSpec, Spec};

pub async fn run() -> Result<()> {
    let mut fs_io = MockedFs;
    let spec: Spec = toml::from_str(&fs_io.spec_contents().await?)
        .wrap_err("failed to parse spec toml")?;
    let _locked: LockedSpec = toml::from_str(&fs_io.lockfile_contents().await?)
        .wrap_err("failed to parse lockfile toml")?;

    let gh_token = fs_io.github_token_env();
    if gh_token.is_some() {
        tracing::info!("Using provided github token");
    } else {
        tracing::warn!("No github token provided");
    }
    let client = Client::new(gh_token)?;

    let sources = spec
        .artifacts
        .into_iter()
        .map(|(name, art)| (name, art.source.clone()))
        .collect();
    let dp = DownloadPlan { sources };
    dp.run(client, &mut fs_io).await
}

struct DownloadPlan {
    sources: HashMap<ArtifactName, Source>,
}

impl DownloadPlan {
    async fn run(self, client: Client, fs_io: &mut MockedFs) -> Result<()> {
        tracing::debug!("starting download plan");
        let multi_progress = indicatif::MultiProgress::new();
        let mut download_tasks: tokio::task::JoinSet<Result<u64>> =
            tokio::task::JoinSet::new();
        for (s_name, s) in self.sources {
            let writer = fs_io.writer_from_artifact(&s_name, &s).await?;
            let (reader, total_bytes) = match s {
                Source::Github(s) => {
                    crate::downloader::github::download_artifact(&client, s)
                        .await
                        .wrap_err("failed to download github source: {s:?}")?
                }
            };
            let style = ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({msg})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
        .progress_chars("#>-");
            let progress = multi_progress
                .add(
                    indicatif::ProgressBar::new(total_bytes)
                        .with_message(s_name.0.clone())
                        .with_style(style.clone()),
                )
                .wrap_async_read(Box::into_pin(reader));
            download_tasks.spawn(async move {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let nbytes =
                    tokio::io::copy(&mut pin!(progress), &mut Box::into_pin(writer))
                        .await
                        .wrap_err("failed to write body to writer")?;
                assert_eq!(nbytes, total_bytes, "nbytes and total bytes didn't match");
                Ok(nbytes)
            });
        }

        while let Some(result) = download_tasks.join_next().await {
            let _nbytes = result.wrap_err("task panicked")?.wrap_err("task errored")?;
        }
        Ok(())
    }
}

struct MockedFs;

impl MockedFs {
    async fn spec_contents(&self) -> Result<String> {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/config/example.toml"
        ));
        tokio::fs::read_to_string(path)
            .await
            .wrap_err("failed to read spec file")
    }

    fn github_token_env(&self) -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok()
    }

    async fn lockfile_contents(&self) -> Result<String> {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/config/example.lock"
        ));
        tokio::fs::read_to_string(path)
            .await
            .wrap_err("failed to read lock file")
    }

    async fn writer_from_artifact(
        &self,
        name: &ArtifactName,
        _src: &Source,
    ) -> Result<Box<dyn AsyncWrite + Send>> {
        // TODO: Use filesystem
        let p = PathBuf::from(std::env::var("HOME").unwrap())
            .join("Downloads")
            .join(&name.0);
        let f: Box<dyn AsyncWrite + Send> = Box::new(
            tokio::fs::File::create(p)
                .await
                .wrap_err("failed to create file")?,
        );
        Ok(f)
    }
}
