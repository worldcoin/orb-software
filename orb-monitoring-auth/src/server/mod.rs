use crate::{server::secure_storage::SecureStorage, Token};
use chrono::{DateTime, Utc};
use clap::ArgMatches;
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_info::OrbId;
use prelude::applet::Applet;
use speare::{
    mini::{self, OnErr},
    Backoff, Limit,
};
use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::signal::unix::{self, SignalKind};
use tracing::{error, warn};

mod refresh_token;
mod secure_storage;
mod serve_token;

pub const DEFAULT_SOCKET: &str = "/run/orb-monitoring-auth-server/socket";
#[derive(Debug, Default)]
pub struct OrbMonitoringAuthServer;

impl Applet for OrbMonitoringAuthServer {
    fn name(&self) -> &'static str {
        "orb-monitoring-auth-server"
    }

    fn about(&self) -> &'static str {
        "fetches monitoring token from our backend, stores it in SecureStorage and serves it to datadog-agent"
    }

    fn main(&self, _args: &ArgMatches) -> ExitCode {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("failed to build tokio runtime")
            .and_then(|rt| {
                rt.block_on(async { main(Dependencies::new().await?).await })
            });

        match result {
            Err(e) => {
                error!("orb-auth-monitoring-server failed with {e:?}");
                ExitCode::FAILURE
            }

            Ok(_) => ExitCode::SUCCESS,
        }
    }
}

async fn main(deps: Dependencies) -> Result<()> {
    let token: StoredToken = Default::default();

    get_token_from_secure_storage(&deps.secure_storage, &token).await?;

    let speare = mini::root();
    let restart = OnErr::Restart {
        max: Limit::None,
        backoff: Backoff::Static(Duration::from_secs(30)),
    };

    speare
        .task_with()
        .args(serve_token::Args {
            token: token.clone(),
            socket_path: deps.server_socket_path,
            dd_agent_uid: deps.dd_agent_uid,
        })
        .on_err(restart)
        .spawn(serve_token::task)?;

    speare
        .task_with()
        .args(refresh_token::Args {
            token,
            secure_storage: deps.secure_storage,
            endpoint: deps.token_endpoint,
            dbus: deps.dbus,
            interval: deps.refresh_token_interval,
            clock: deps.clock,
            orb_id: deps.orb_id,
        })
        .on_err(restart)
        .spawn(refresh_token::task)?;

    let mut sigterm = unix::signal(SignalKind::terminate())?;
    let mut sigint = unix::signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = sigterm.recv() => warn!("received SIGTERM"),
        _ = sigint.recv()  => warn!("received SIGINT"),
    };

    speare.abort_children()?;

    Ok(())
}

type StoredToken = Arc<RwLock<Option<Token>>>;

#[cfg_attr(feature = "testing", faux::create)]
pub struct Clock;

#[cfg_attr(feature = "testing", faux::methods)]
impl Clock {
    pub fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct Dependencies {
    pub token_endpoint: String,
    pub server_socket_path: PathBuf,
    pub dd_agent_uid: u32,
    pub clock: Clock,
    pub dbus: zbus::Connection,
    pub refresh_token_interval: Duration,
    pub orb_id: OrbId,
    pub secure_storage: SecureStorage,
}

impl Dependencies {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            token_endpoint: "todo".into(),
            server_socket_path: DEFAULT_SOCKET.into(),
            dd_agent_uid: crate::DD_AGENT_UID,
            clock: Clock,
            dbus: zbus::Connection::session().await?,
            refresh_token_interval: Duration::from_hours(4),
            orb_id: OrbId::read().await?,
            secure_storage: SecureStorage::new().await?,
        })
    }
}

async fn get_token_from_secure_storage(
    ss: &SecureStorage,
    token: &StoredToken,
) -> Result<()> {
    let ss_token = ss.get().await?;
    (*token.write().map_err(|e| eyre!("{e:?}"))?) = ss_token;

    Ok(())
}
