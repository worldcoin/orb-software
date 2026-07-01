use std::time::Duration;

use tap::TapFallible;
use tracing::error;

pub mod helpers;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_disallows_downloads_if_signup_started_received(
) -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);
    let zenorb = helpers::isolated_supervisor_zenorb().await?;

    let application =
        helpers::spawn_supervisor_service(settings.clone(), zenorb).await?;
    let _application_handle =
        tokio::spawn(application.run(helpers::fixture_os_release()));

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
    let zenorb = helpers::isolated_supervisor_zenorb().await?;
    let application =
        helpers::spawn_supervisor_service(settings.clone(), zenorb).await?;

    let _application_handle =
        tokio::spawn(application.run(helpers::fixture_os_release()));

    // Let the stop-core deadline elapse so the shutdown task is willing to fire immediately.
    tokio::time::sleep(helpers::TEST_STOP_CORE_AFTER_SIGNUP * 2).await;

    let update_agent_proxy =
        helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;
    let (system_conn, _captured) = helpers::start_interfaces(&dbus_instances).await?;

    let request_update_permission_task = tokio::task::spawn(async move {
        update_agent_proxy.request_update_permission().await
    });

    let system_conn_clone = system_conn.clone();
    let set_active_state_task = tokio::task::spawn(async move {
        let core_unit_ref = system_conn_clone
            .object_server()
            .interface::<_, helpers::CoreUnit>(helpers::WORLDCOIN_CORE_SERVICE_OBJECT_PATH)
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
async fn application_serves_gondor_zoci_handler_end_to_end() -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let (_router, supervisor_zenorb, client_zenorb) =
        helpers::spawn_zenoh_router_and_clients("supervisor", "test-client").await?;

    let (_system_conn, captured) = helpers::start_interfaces(&dbus_instances).await?;

    let settings = helpers::make_settings(&dbus_instances);
    let application =
        helpers::spawn_supervisor_service(settings, supervisor_zenorb).await?;
    let _application_handle =
        tokio::spawn(application.run(helpers::fixture_os_release()));

    // Give Application::run a beat to register its zoci queryable on the router.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let reply = client_zenorb
        .command_raw(
            "supervisor/job/gondor",
            r#"{"version":"to-main","restart":true}"#,
        )
        .await?;
    if let Err(e) = reply {
        let body = String::from_utf8_lossy(&e.payload().to_bytes()).into_owned();
        panic!("expected success reply, got error: {body}");
    }

    let env = captured.set_environment.lock().unwrap();
    assert_eq!(
        *env,
        vec![vec![
            "ORB_UPDATE_AGENT_VERSION_OVERWRITE=to-main-diamond-prod".to_string()
        ]],
        "expected exactly one SetEnvironment call with the derived version"
    );

    let restarts = captured.restart_unit.lock().unwrap();
    assert_eq!(
        *restarts,
        vec!["worldcoin-update-agent.service".to_string()],
        "expected exactly one RestartUnit call for the update-agent service"
    );

    Ok(())
}
