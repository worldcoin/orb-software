use std::{convert::identity, time::Duration};

use futures::{StreamExt as _, TryFutureExt as _};
use tokio::{
    task::JoinHandle,
    time::{self, error::Elapsed, Instant},
};
use tracing::{debug, info, instrument, warn};
use zbus_systemd::systemd1::{self, ManagerProxy};

use crate::consts::{
    DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP, WORLDCOIN_CORE_UNIT_NAME,
};

/// Calculates the instant that is 20 minutes after the last signup event.
fn calculate_stop_deadline(last_signup_started_event: Instant) -> Instant {
    last_signup_started_event
        .checked_add(crate::consts::DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP)
        .expect("`Instant` should always be able to represent the timescales of this codebase")
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("reached timeout while waiting for worldcoin core service to be stopped")]
    Elapsed(#[from] Elapsed),
    #[error(
        "failed communicating over dbus; TODO: break this up into individual errors"
    )]
    Dbus(#[from] zbus::Error),
}

/// Spawns a task that shuts down worldcoin core after enough time has passed.
#[must_use]
pub fn spawn_shutdown_worldcoin_core_timer(
    proxy: ManagerProxy<'static>,
    mut last_signup_started_event: tokio::sync::watch::Receiver<Instant>,
) -> JoinHandle<Result<(), Error>> {
    tokio::spawn(async move {
        let trigger_stop = time::sleep_until(calculate_stop_deadline(
            *last_signup_started_event.borrow(),
        ));
        tokio::pin!(trigger_stop);
        loop {
            tokio::select!(

                // reset the trigger if a new signup has started
                _ = last_signup_started_event.changed() => {
                    info!(
                        duration_s = DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP.as_secs(),
                        "new signup started, resetting timer",
                    );
                    trigger_stop
                        .as_mut()
                        .reset(calculate_stop_deadline(*last_signup_started_event.borrow()));
                },

                () = &mut trigger_stop => {
                    break;
                }
            );
        }
        info!("deadline reached, shutting down worldcoin core service");
        let worldcoin_core_timeout_stop =
            get_worldcoin_core_timeout(proxy.clone()).await?;
        stop_worldcoin_core(proxy.clone(), WORLDCOIN_CORE_UNIT_NAME, "replace").await?;
        tokio::time::timeout(
            worldcoin_core_timeout_stop,
            has_worldcoin_core_stopped(proxy.clone()),
        )
        .await
        .map_err(From::from)
        .and_then(identity)
    })
}

#[must_use]
pub fn spawn_start_update_agent_after_core_shutdown_task(
    proxy: systemd1::ManagerProxy<'static>,
    shutdown_task: JoinHandle<Result<(), Error>>,
) -> JoinHandle<Result<(), Error>> {
    tokio::spawn(async move {
        match shutdown_task.await {
            Ok(Ok(())) => info!("worldcoin core shutdown task completed"),
            Ok(Err(e)) => {
                warn!(error = ?e, "worldcoin core shutdown task returned with error");
            }
            Err(e) => warn!(panic_msg = ?e, "worldcoin core shutdown task panicked"),
        }
        info!("calling `org.freedesktop.systemd1.Manager.StartUnit` to start update agent");
        proxy
            .start_unit("worldcoin-update-agent.service".into(), "replace".into())
            .await
            .map(|_| {})
            .map_err(Into::into)
    })
}
// #[instrument(
//     name = "spawn_update_agent_after_core_stopped",
//     skip_all,
// )]
// pub async fn spawn_start_update_agent_after_worldcoin_core_stopped_task(
//     proxy: ManagerProxy<'static>,
// ) -> JoinHandle<zbus::Result<()>> { tokio::spawn(async move { let timeout_duration =
//   get_worldcoin_core_timeout(proxy.clone()).await?; match tokio::time::timeout( timeout_duration,
//   has_worldcoin_core_stopped(proxy), ).await { Ok(_) => todo!("worldcoin core stopped"),
//   Err(elapsed) => { info!(error = %elapsed, "did not " } }; Ok(()) })

// }

#[instrument(skip_all, err, ret(Debug))]
async fn get_worldcoin_core_timeout(
    proxy: ManagerProxy<'static>,
) -> zbus::Result<Duration> {
    let worldcoin_core_service = proxy
        .get_unit(WORLDCOIN_CORE_UNIT_NAME.to_string())
        .and_then(|worldcoin_core_object| async {
            zbus_systemd::systemd1::ServiceProxy::builder(proxy.connection())
                .destination("org.freedesktop.systemd1")?
                .path(worldcoin_core_object)?
                .build()
                .await
        })
        .await?;
    worldcoin_core_service
        .timeout_stop_u_sec()
        .map_ok(Duration::from_micros)
        .await
}

/// Reports if the worldcoin core systemd service has stopped.
///
/// This function makes use of the fact that the first item produced by the `PropertyChangedStream`
/// is its current value. This is probably an implementation detail of zbus.
#[instrument(skip_all, err, ret)]
async fn has_worldcoin_core_stopped(proxy: ManagerProxy<'static>) -> Result<(), Error> {
    let orb_core_unit = proxy
        .get_unit(WORLDCOIN_CORE_UNIT_NAME.to_string())
        .and_then(|object_path| async {
            zbus_systemd::systemd1::UnitProxy::builder(proxy.connection())
                .destination("org.freedesktop.systemd1")?
                .path(object_path)?
                .build()
                .await
        })
        .await?;
    debug!("awaiting active state changed");
    let mut active_state_stream = orb_core_unit.receive_active_state_changed().await;

    // This makes use of the fact that the first iteration always returns the current state.
    // So if the service is already inactive or failed, then this loop will break and we
    // doesn't spin indefinitely.
    debug!("spinning");
    while let Some(event) = active_state_stream.next().await {
        match &*event.get().await? {
            "inactive" | "failed" => break,
            other => {
                info!(event = other, "received event");
            }
        }
    }
    Ok(())
}

#[instrument(
    skip(proxy),
    fields(dbus_method = "org.freedesktop.systemd1.Manager.StopUnit",)
)]
async fn stop_worldcoin_core(
    proxy: systemd1::ManagerProxy<'static>,
    unit_name: &'static str,
    stop_mode: &'static str,
) -> zbus::Result<()> {
    match proxy
        .stop_unit(unit_name.to_string(), stop_mode.to_string())
        .await
    {
        Ok(unit_path) => {
            debug!(
                job_object = unit_path.as_str(),
                "dbus method call successful"
            );
        }

        Err(zbus::Error::MethodError(name, detail, reply))
            if name == "org.freedesktop.systemd1.NoSuchUnit" =>
        {
            // We need to reconstruct the error here because the destructuring, guards and bindings
            // don't work in match statements
            let method_error = zbus::Error::MethodError(name, detail, reply);
            debug!(error = %method_error, "systemd mostl likely reported that worldcoin core is stopped");
        }

        Err(zbus::Error::FDO(e)) => {
            warn!(
                err = ?e,
                dbus_method = "org.freedesktop.systemd1.Manager.StopUnit",
                "encountered a D-Bus error when dbus method; permitting update",
            );
        }
        Err(e) => {
            return Err(e);
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::{calculate_stop_deadline, DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP};

    #[test]
    fn deadline_of_old_signup_event_is_in_the_past() {
        let an_hour_ago = Instant::now()
            .checked_sub(Duration::from_secs(60 * 60))
            .expect("`Instant` should always be able to represent current time minus 60 minutes");
        let stop_deadline = calculate_stop_deadline(an_hour_ago);
        assert!(stop_deadline < Instant::now());
    }

    #[test]
    fn deadline_of_now_is_wait_time() {
        let now = Instant::now();
        let calculated_stop_deadline = calculate_stop_deadline(now);
        let expected_stop_deadline = now
            .checked_add(DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP)
            .expect("`Instant` should always be able to represent current time + some minutes");
        assert_eq!(expected_stop_deadline, calculated_stop_deadline);
    }
}
