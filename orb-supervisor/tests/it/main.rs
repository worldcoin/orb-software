use tokio::time::{
    Duration,
    Instant,
};

pub mod helpers;

#[tokio::test(start_paused = true)]
async fn supervisor_disallows_downloads_if_signup_started_received() -> eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let mut settings = helpers::make_settings(&dbus_instances);
    settings
        .manager_last_event
        .replace(Instant::now() - Duration::from_secs(3700));

    let application = helpers::spawn_supervisor_service(settings.clone()).await?;
    let _application_handle = tokio::spawn(application.run());

    let update_agent_proxy = helpers::make_update_agent_proxy(&settings, &dbus_instances).await?;

    let downloads_are_allowed_initially = update_agent_proxy.background_downloads_allowed().await?;
    assert!(downloads_are_allowed_initially);

    let _signup_task_handle = helpers::spawn_signup_start_task(&settings, &dbus_instances).await?;

    // FIXME: We want to use `#[tokio::test(start_paused = true)]` and `tokio::time::advance` here
    // instead. Unfortunately, this does not currently play nicely with zbus, probably because of
    // its internal executor. Once that is fixed upstream this should be changed to the mocked time
    // API.
    tokio::time::sleep(Duration::from_millis(500)).await;
    // tokio::time::advance(Duration::from_millis(500)).await;

    let downloads_are_allowed_after_signal =
        update_agent_proxy.background_downloads_allowed().await?;
    assert!(!downloads_are_allowed_after_signal);

    Ok(())
}
