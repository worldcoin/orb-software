use std::time::Duration;

use tap::TapFallible;
use tracing::error;

pub mod helpers;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_disallows_downloads_if_signup_started_received(
) -> color_eyre::Result<()> {
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

    helpers::start_signup_service_and_send_signal(&settings, &dbus_instances).await?;
    // Give the signup-started task a beat to consume the signal and reset the timer.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let downloads_allowed_after_signal =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(!downloads_allowed_after_signal);

    // Wait past the throttle window and verify downloads become allowed again.
    tokio::time::sleep(helpers::TEST_DOWNLOAD_THROTTLE).await;

    let downloads_allowed_after_period =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(downloads_allowed_after_period);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_stops_orb_core_when_update_permission_is_requested(
) -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);
    let application = helpers::spawn_supervisor_service(settings.clone()).await?;

    let _application_handle = tokio::spawn(application.run());

    // Let the stop-core deadline elapse so the shutdown task is willing to fire immediately.
    tokio::time::sleep(helpers::TEST_STOP_CORE_AFTER_SIGNUP * 2).await;

    let update_agent_proxy =
        helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;
    let system_conn = helpers::start_interfaces(&dbus_instances).await?;

    let request_update_permission_task = tokio::task::spawn(async move {
        update_agent_proxy.request_update_permission().await
    });

    let system_conn_clone = system_conn.clone();
    let set_active_state_task = tokio::task::spawn(async move {
        let core_unit_ref = system_conn_clone
            .object_server()
            .interface::<_, helpers::Unit>(helpers::WORLDCOIN_CORE_SERVICE_OBJECT_PATH)
            .await
            .tap_err(|e| error!(error = ?e, "failed getting CoreUnit interface from object server"))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let signal_ctx = core_unit_ref.signal_context().clone();
        let mut iface = core_unit_ref.get_mut().await;
        iface.set_active_state("inactive".into()).await;
        iface
            .active_state_changed(&signal_ctx)
            .await
            .expect("active_state property changed signal");
    });

    let (update_permission, active_state) =
        tokio::join!(request_update_permission_task, set_active_state_task);
    let update_permission = update_permission.expect(
        "the request update permissions task should not have panicked because we don't explicitly \
         panick in it",
    );
    assert!(matches!(update_permission, Ok(())));
    assert!(matches!(active_state, Ok(())));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_waits_for_update_agent_exit_before_retrying(
) -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;
    let system_conn = helpers::start_interfaces(&dbus_instances).await?;
    let settings = helpers::make_settings(&dbus_instances);
    let application = helpers::spawn_supervisor_service(settings.clone()).await?;
    let _application_handle = tokio::spawn(application.run());
    let update_agent_proxy =
        helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;

    let permission = tokio::time::timeout(
        Duration::from_secs(3),
        update_agent_proxy.request_update_permission(),
    )
    .await?;
    assert!(
        permission.is_err(),
        "core is still active after the permission deadline"
    );

    let core = system_conn
        .object_server()
        .interface::<_, helpers::Unit>(helpers::WORLDCOIN_CORE_SERVICE_OBJECT_PATH)
        .await?;
    {
        let mut unit = core.get_mut().await;
        unit.set_active_state("inactive".into()).await;
        unit.active_state_changed(core.signal_context()).await?;
    }

    let manager = system_conn
        .object_server()
        .interface::<_, helpers::Manager>("/org/freedesktop/systemd1")
        .await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        manager.get().await.started_units.lock().unwrap().is_empty(),
        "starting the still-running update agent loses the retry"
    );

    let agent = system_conn
        .object_server()
        .interface::<_, helpers::Unit>(helpers::UPDATE_AGENT_SERVICE_OBJECT_PATH)
        .await?;
    {
        let mut unit = agent.get_mut().await;
        unit.set_active_state("failed".into()).await;
        unit.active_state_changed(agent.signal_context()).await?;
    }

    let started_units = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let started = manager.get().await.started_units.lock().unwrap().clone();
            if !started.is_empty() {
                break started;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(started_units, ["worldcoin-update-agent.service"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_retries_an_already_stopped_update_agent() -> color_eyre::Result<()>
{
    for state in ["inactive", "failed"] {
        let dbus_instances = helpers::launch_dbuses().await??;
        let system_conn = helpers::start_interfaces(&dbus_instances).await?;
        let agent = system_conn
            .object_server()
            .interface::<_, helpers::Unit>(helpers::UPDATE_AGENT_SERVICE_OBJECT_PATH)
            .await?;
        agent.get_mut().await.set_active_state(state.into()).await;
        let proxy = zbus_systemd::systemd1::ManagerProxy::new(&system_conn).await?;
        let task = orb_supervisor::tasks::update::spawn_start_update_agent_after_core_shutdown_task(
            proxy,
            tokio::spawn(async { Ok(()) }),
        );
        tokio::time::timeout(Duration::from_secs(3), task).await???;

        let manager = system_conn
            .object_server()
            .interface::<_, helpers::Manager>("/org/freedesktop/systemd1")
            .await?;
        assert_eq!(
            *manager.get().await.started_units.lock().unwrap(),
            ["worldcoin-update-agent.service"]
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_does_not_retry_if_update_agent_does_not_exit(
) -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;
    let system_conn = helpers::start_interfaces(&dbus_instances).await?;
    let agent_service = system_conn
        .object_server()
        .interface::<_, helpers::Service>(helpers::UPDATE_AGENT_SERVICE_OBJECT_PATH)
        .await?;
    agent_service.get_mut().await.timeout_stop_usec = 50_000;

    let proxy = zbus_systemd::systemd1::ManagerProxy::new(&system_conn).await?;
    let task = orb_supervisor::tasks::update::spawn_start_update_agent_after_core_shutdown_task(
        proxy,
        tokio::spawn(async { Ok(()) }),
    );
    let result = tokio::time::timeout(Duration::from_secs(3), task).await??;
    assert!(matches!(
        result,
        Err(orb_supervisor::tasks::update::Error::Elapsed {
            unit_name: "worldcoin-update-agent.service",
            ..
        })
    ));
    let manager = system_conn
        .object_server()
        .interface::<_, helpers::Manager>("/org/freedesktop/systemd1")
        .await?;
    assert!(manager.get().await.started_units.lock().unwrap().is_empty());
    Ok(())
}
