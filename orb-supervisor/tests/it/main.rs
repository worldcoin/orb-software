pub mod helpers;

#[tokio::test(start_paused = true)]
async fn supervisor_disallows_downloads_if_signup_started_received() -> eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);

    let application = helpers::spawn_supervisor_service(settings.clone()).await?;
    let _application_handle = tokio::spawn(application.run());

    let update_agent_proxy = helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;

    let downloads_allowed_initially = update_agent_proxy.background_downloads_allowed().await?;
    assert!(!downloads_allowed_initially);

    tokio::time::advance(orb_supervisor::interfaces::manager::DEFAULT_DURATION_TO_ALLOW_DOWNLOADS)
        .await;

    let downloads_allowed_after_period = update_agent_proxy.background_downloads_allowed().await?;
    assert!(downloads_allowed_after_period);

    helpers::start_signup_service_and_send_signal(&settings, &dbus_instances).await?;
    let downloads_allowed_after_signal = update_agent_proxy.background_downloads_allowed().await?;
    assert!(!downloads_allowed_after_signal);

    Ok(())
}

#[tokio::test]
async fn supervisor_stops_orb_core_when_update_permission_is_requested() -> eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let settings = helpers::make_settings(&dbus_instances);
    let application = helpers::spawn_supervisor_service(settings.clone()).await?;
    let _application_handle = tokio::spawn(application.run());

    let update_agent_proxy = helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;
    let (_systemd_conn, rx) = helpers::start_systemd_manager(&dbus_instances).await?;

    let (permission_given, stop_unit_args) =
        tokio::join!(update_agent_proxy.request_update_permission(), rx);

    assert!(permission_given?);

    let (name, replace) = stop_unit_args?;
    assert_eq!(name, "worldcoin-core.service");
    assert_eq!(replace, "replace");

    Ok(())
}
