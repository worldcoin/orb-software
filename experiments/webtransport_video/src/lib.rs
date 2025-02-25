use std::sync::Arc;

use clap::Parser;
use color_eyre::Result;
use config_builder::State;
use derive_more::{AsRef, Deref, Into};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::info;
use wtransport::tls::Sha256DigestFmt;

use self::config_builder::{IsUnset, SetIdentity};
use crate::networking::{run_http_server, run_wt_server};

mod control;
mod networking;
mod wt_server;

#[derive(Debug, Parser, Clone)]
pub struct Args {
    /// The port to use for the http server
    #[clap(long, default_value = "8443")]
    http_port: u16,
    /// The port to use for the webtransport server
    #[clap(long, default_value = "1337")]
    wt_port: u16,
}

#[derive(Debug, bon::Builder)]
pub struct Config {
    pub wt_port: u16,
    pub http_port: u16,
    pub identity: wtransport::Identity,
    pub cancel: CancellationToken,
    pub frame_rx: watch::Receiver<EncodedPng>,
}

impl<S: State> ConfigBuilder<S> {
    /// Generates a new self-signed certificate for the identity.
    ///
    /// # Example
    /// ```
    /// # let config = Config::builder();
    /// config.identity_self_signed(["localhost", "127.0.0.1", "::1"])
    /// // ...
    ///
    /// ```
    pub fn identity_self_signed(
        self,
        subject_alt_names: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> ConfigBuilder<SetIdentity<S>>
    where
        S::Identity: IsUnset,
    {
        let identity = wtransport::Identity::self_signed_builder()
            .subject_alt_names(subject_alt_names)
            .from_now_utc()
            .validity_days(7)
            .build()
            .unwrap();

        self.identity(identity)
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            wt_port: self.wt_port,
            http_port: self.http_port,
            identity: self.identity.clone_identity(),
            cancel: self.cancel.clone(),
            frame_rx: self.frame_rx.clone(),
        }
    }
}

impl Config {
    pub fn spawn(self) -> WebtransportTaskHandle {
        WebtransportTaskHandle::spawn(self)
    }
}

#[derive(Debug)]
pub struct WebtransportTaskHandle {
    pub task_handle: tokio::task::JoinHandle<Result<()>>,
}

impl WebtransportTaskHandle {
    pub fn spawn(cfg: Config) -> Self {
        let cancel = cfg.cancel.clone();
        let task_handle = tokio::task::spawn(async move {
            cancel.run_until_cancelled(run(cfg)).await.unwrap_or(Ok(()))
        });
        Self { task_handle }
    }
}

pub async fn run(cfg: Config) -> Result<()> {
    let _cancel_guard = cfg.cancel.clone().drop_guard();

    let server_certificate_hashes = cfg.identity.certificate_chain().as_slice()[0]
        .hash()
        .fmt(Sha256DigestFmt::BytesArray);
    info!("server certificate hashes: {}", server_certificate_hashes);

    let wt_fut = async {
        let cancel = cfg.cancel.child_token();
        cancel
            .run_until_cancelled(run_wt_server(
                cfg.clone(),
                cancel.clone(),
                video_task.frame_rx,
            ))
            .await
            .unwrap_or(Ok(()))
    };

    let http_fut = async {
        let cancel = cancel.child_token();
        cancel
            .run_until_cancelled(run_http_server(
                args.clone(),
                cancel.clone(),
                identity.clone_identity(),
            ))
            .await
            .unwrap_or(Ok(()))
    };

    let video_task_fut =
        async { video_task.task_handle.await.wrap_err("video task panicked") };
    let ((), (), ()) = tokio::try_join!(wt_fut, http_fut, video_task_fut)?;
    Ok(())
}

/// Newtype on a vec, to indicate that this contains a png-encoded image.
#[derive(Debug, Into, AsRef, Clone, Deref)]
pub struct EncodedPng(pub Arc<Vec<u8>>);

impl EncodedPng {
    /// Equivalent to [`Self::clone`] but is more explicit that this operation is cheap.
    pub fn clone_cheap(&self) -> Self {
        EncodedPng(Arc::clone(&self.0))
    }
}

impl AsRef<[u8]> for EncodedPng {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}
