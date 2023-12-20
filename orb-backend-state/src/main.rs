mod api;
mod context;
mod dbus_interface;
mod dbus_proxies;
mod state;

use std::time::{Duration, Instant};

use build_info::{make_build_info, BuildInfo};
use clap::Parser;
use color_eyre::eyre::bail;
use color_eyre::{eyre::WrapErr, Result};
use context::Context;
use futures::FutureExt;
use tokio::{select, sync::watch};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};
use zbus::export::futures_util::StreamExt;

use crate::state::State;
use crate::{api::Token, dbus_proxies::AuthTokenProxy};

const ONE_DAY: Duration = Duration::from_secs(60 * 60 * 24);
const RETRY_DELAY_MIN: Duration = Duration::from_secs(1);
const RETRY_DELAY_MAX: Duration = Duration::from_secs(60);

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.git.describe, styles=make_clap_v3_styles())]
struct Cli {}

// No need to waste RAM with a threadpool.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let _args = Cli::parse();

    let conn = zbus::Connection::session()
        .await
        .wrap_err("failed to connect to zbus session")?;
    let msg_stream = zbus::MessageStream::from(conn.clone());
    let dbus_disconnect_task_handle = tokio::spawn(async move {
        // Until the stream terminates, this will never complete.
        let _ = msg_stream.count().await;
        bail!("zbus connection terminated!");
    });

    let (watch_token_task_handle, ctx) = {
        // Use env var if present - useful for testing
        const VAR: &str = "ORB_AUTH_TOKEN";
        if let Ok(token) = std::env::var(VAR) {
            std::env::remove_var(VAR);
            tracing::warn!("using env var `ORB_AUTH_TOKEN` instead of daemon");
            assert!(!token.is_empty());
            let (_send, recv) = watch::channel(Token::from(token));
            (
                tokio::task::spawn(std::future::pending()),
                Context::new(recv)
                    .await
                    .wrap_err("failed to create context")?,
            )
        } else {
            let token_proxy = AuthTokenProxy::new(&conn)
                .await
                .wrap_err("failed to create AuthToken proxy")?;
            let (watch_token_task_handle, token_watcher) =
                watch_token_task(token_proxy)
                    .await
                    .wrap_err("failed to spawn watch token task")?;
            (watch_token_task_handle, Context::new(token_watcher).await?)
        }
    };

    let iface_ref: zbus::InterfaceRef<self::dbus_interface::Interface> = {
        const IFACE_PATH: &str = "/org/worldcoin/BackendState1";
        let conn = zbus::ConnectionBuilder::session()
            .wrap_err("failed to establish user session dbus connection")?
            .name("org.worldcoin.BackendState1")
            .wrap_err("failed to get name")?
            .serve_at(
                IFACE_PATH,
                self::dbus_interface::Interface::new(ctx.clone()),
            )
            .wrap_err("failed to serve at")?
            .build()
            .await
            .wrap_err("failed to build")?;
        let obj_serv = conn.object_server();
        obj_serv
            .interface(IFACE_PATH)
            .await
            .expect("should be successful because we already registered")
    };
    let notify_state_task_handle = spawn_notify_state_task(iface_ref, ctx.clone());
    let poll_backend_task_handle = tokio::task::spawn(poll_backend(ctx));
    tracing::info!("Started orb-backend-state service");

    #[allow(unreachable_code)]
    let _: ((), (), (), ()) = tokio::try_join!(
        notify_state_task_handle
            .map(|r| r.wrap_err("notify_state task exited unexpectedly")?),
        watch_token_task_handle
            .map(|r| r.wrap_err("watch_token task exited unexpectedly")),
        dbus_disconnect_task_handle
            .map(|r| r.wrap_err("connection_refresh task exited unexpectedly")?),
        poll_backend_task_handle
            .map(|r| r.wrap_err("poll_backend task exited unexpectedly"))
    )?;
    Ok(())
}

/// Spawns tokio task for monitoring the token.
async fn watch_token_task(
    token_proxy: crate::dbus_proxies::AuthTokenProxy<'static>,
) -> Result<(tokio::task::JoinHandle<()>, watch::Receiver<Token>)> {
    let initial_value = Token::from(token_proxy.token().await?);
    let (send, recv) = watch::channel(initial_value);
    let join_handle = tokio::task::spawn(async move {
        let mut token_changed = token_proxy.receive_token_changed().await;
        while let Some(token) = token_changed.next().await {
            let token = token.get().await.expect("should have received token");
            send.send(Token::from(token))
                .expect("should have sent token to watchers");
        }
    });
    Ok((join_handle, recv))
}

/// Updates the shared state by fetching it from the backend
async fn update_state(ctx: &Context) -> Result<State> {
    let token = ctx.token.borrow().to_owned();
    let orb_id = &ctx.orb_id;
    let new_state = crate::api::get_state(orb_id, &token)
        .await
        .wrap_err("Error while fetching state from backend");
    match new_state {
        Ok(s) => {
            tracing::info!("Fetched new state: {s:?}");
            ctx.state.update(s.clone());
            Ok(s)
        }
        Err(e) => {
            tracing::error!(err = ?e, "Error while fetching state.");
            Err(e)
        }
    }
}

/// Repeatedly polls the backend for the current state.
async fn poll_backend(mut ctx: Context) -> ! {
    let mut delay = RETRY_DELAY_MIN;
    let mut next_attempt = Instant::now();
    loop {
        select! {
            // Responsible for polling repeatedly
            _ = tokio::time::sleep_until(next_attempt.into()) => {
                next_attempt = match update_state(&ctx).await {
                    Ok(s) => {
                        delay = RETRY_DELAY_MIN;
                        s.expires_at()
                    }
                    Err(_err) => {
                        // Exponential backoff
                        delay *= 2;
                        delay = Duration::min(delay, RETRY_DELAY_MAX);
                        Instant::now() + delay
                    },
                };
            }
            // If we got an update, this cancels the above sleep and updates the next attempt
            // to be at the correct expiry time.
            s = ctx.state.wait_for_update() => {
                delay = RETRY_DELAY_MIN;
                next_attempt = s.expires_at()
            }
        };
    }
}

/// Listens for changes to state, and signals that change to the dbus interface.
fn spawn_notify_state_task(
    iface: zbus::InterfaceRef<crate::dbus_interface::Interface>,
    mut ctx: Context,
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::task::spawn(async move {
        let signal_ctx = iface.signal_context();
        loop {
            ctx.state.wait_for_update().await;
            iface
                .get()
                .await
                .state_changed(signal_ctx)
                .await
                .wrap_err("failed to signal state change")?;
        }
    })
}

fn make_clap_v3_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
