use std::time::Duration;

use tap::TapFallible;
use tokio::sync::oneshot;
use tracing::error;

pub mod helpers;

#[tokio::test(start_paused = true)]
async fn supervisor_disallows_downloads_if_signup_started_received() -> eyre::Result<()>
{
    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);

    let application = helpers::spawn_supervisor_service(settings.clone()).await?;
    let _application_handle = tokio::spawn(application.run());

    let update_agent_proxy =
        helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;

    // We want to ensure that downloads are allowed when the manager begins
    let downloads_allowed_initially =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(downloads_allowed_initially);

    // Now we check thaht after a signup, downloads are not allowed
    helpers::start_signup_service_and_send_signal(&settings, &dbus_instances).await?;
    let downloads_allowed_after_signal =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(!downloads_allowed_after_signal);

    // Then wait for the timeout duration to pass and ensure that the downloads
    // are once again allowed
    tokio::time::advance(
        orb_supervisor::interfaces::manager::DEFAULT_DURATION_TO_ALLOW_DOWNLOADS,
    )
    .await;

    let downloads_allowed_after_period =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(downloads_allowed_after_period);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn supervisor_stops_orb_core_when_update_permission_is_requested(
) -> eyre::Result<()> {
    // FIXME: This is a hack to inhibit tokio auto-advance functionality in tests;
    // See https://github.com/tokio-rs/tokio/pull/5200 for more info and rework this
    // once the necessary functionality is exposed in an API.
    let (inhibit_tx, inhibit_rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || inhibit_rx.blocking_recv());

    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);
    let application = helpers::spawn_supervisor_service(settings.clone()).await?;

    let _application_handle = tokio::spawn(application.run());

    tokio::time::advance(
        orb_supervisor::consts::DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP,
    )
    .await;

    let update_agent_proxy =
        helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;
    let system_conn = helpers::start_interfaces(&dbus_instances).await?;

    let request_update_permission_task = tokio::task::spawn(async move {
        update_agent_proxy.request_update_permission().await
    });
    // Switch active state to "inactive" after 300ms with forced synchronizatoin
    // through the oneshot channel because tasks are not scheduled immediately.
    let (active_task_tx, active_task_rx) = oneshot::channel();
    let system_conn_clone = system_conn.clone();
    let set_active_state_task = tokio::task::spawn(async move {
        let core_unit = system_conn_clone
            .object_server()
            .interface::<_, helpers::CoreUnit>(helpers::WORLDCOIN_CORE_SERVICE_OBJECT_PATH)
            .await
            .tap_err(|e| error!(error = ?e, "failed getting CoreUnit interface from object server"))
            .unwrap();
        active_task_tx
            .send(())
            .expect("oneshot channel should be open");
        tokio::time::sleep(Duration::from_millis(300)).await;
        core_unit
            .get_mut()
            .await
            .set_active_state("inactive".into())
            .await;
    });
    // Using the rx channel as a sync point to make sure time isn't advancing too quickly.
    active_task_rx
        .await
        .expect("oneshot channel should be open");
    tokio::time::advance(Duration::from_millis(500)).await;
    let (update_permission, active_state) =
        tokio::join!(request_update_permission_task, set_active_state_task);
    let update_permission = update_permission.expect(
        "the request update permissions task should not have panicked because we don't explicitly \
         panick in it",
    );
    assert!(matches!(update_permission, Ok(())));
    assert!(matches!(active_state, Ok(())));
    inhibit_tx.send(()).unwrap();

    Ok(())
}
