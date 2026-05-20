use std::{path::PathBuf, time::Duration};

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
    let zenorb = helpers::isolated_supervisor_zenorb().await?;
    let application =
        helpers::spawn_supervisor_service(settings.clone(), zenorb).await?;

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

    let mut settings = helpers::make_settings(&dbus_instances);
    settings.gondor_bin = PathBuf::from("/bin/true");

    let application =
        helpers::spawn_supervisor_service(settings, supervisor_zenorb).await?;
    let _application_handle = tokio::spawn(application.run());

    // Give Application::run a beat to register its zoci queryable on the router.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Each payload shape the handler must accept. /bin/true ignores argv, so
    // these only prove the parser/dispatcher didn't reject the payload — they
    // do NOT verify that --no-restart actually reaches the binary.
    let payloads = [
        ("plain-string", "v1.0.0"),
        (
            "json-with-no-restart-true",
            r#"{"version":"v1.0.0","no_restart":true}"#,
        ),
        ("json-default", r#"{"version":"v1.0.0"}"#),
        (
            "json-with-no-restart-false",
            r#"{"version":"v1.0.0","no_restart":false}"#,
        ),
        // Starts with `{` but isn't valid JSON — handler falls back to treating
        // the whole payload as the version string.
        ("malformed-json-falls-back", "{not-json"),
    ];

    for (label, payload) in payloads {
        let reply = client_zenorb
            .command_raw("supervisor/job/gondor", payload)
            .await?;

        if let Err(reply_err) = reply {
            let body =
                String::from_utf8_lossy(&reply_err.payload().to_bytes()).into_owned();
            panic!("{label}: expected success reply, got error: {body}");
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gondor_zoci_handler_reports_binary_failure() -> color_eyre::Result<()> {
    let dbus_instances = helpers::launch_dbuses().await??;

    let (_router, supervisor_zenorb, client_zenorb) =
        helpers::spawn_zenoh_router_and_clients("supervisor", "test-client").await?;

    // /bin/false always exits non-zero, so the handler should surface that as
    // an error reply regardless of payload shape.
    let mut settings = helpers::make_settings(&dbus_instances);
    settings.gondor_bin = PathBuf::from("/bin/false");

    let application =
        helpers::spawn_supervisor_service(settings, supervisor_zenorb).await?;
    let _application_handle = tokio::spawn(application.run());

    tokio::time::sleep(Duration::from_millis(300)).await;

    let reply = client_zenorb
        .command_raw(
            "supervisor/job/gondor",
            r#"{"version":"v1.0.0","no_restart":true}"#,
        )
        .await?;

    assert!(
        reply.is_err(),
        "expected error reply when gondor binary exits non-zero"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gondor_zoci_handler_passes_argv_through() -> color_eyre::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dbus_instances = helpers::launch_dbuses().await??;

    let (_router, supervisor_zenorb, client_zenorb) =
        helpers::spawn_zenoh_router_and_clients("supervisor", "test-client").await?;

    // Fake gondor that appends one line per argv element plus a "---"
    // record separator to a sibling log, so we can assert what argv each
    // payload shape produced.
    let tmp = tempfile::tempdir()?;
    let log_path = tmp.path().join("argv.log");
    let script_path = tmp.path().join("fake-gondor");
    let script = format!(
        "#!/usr/bin/env bash\nfor a in \"$@\"; do echo \"$a\" >> {p}; done\necho '---' >> {p}\n",
        p = log_path.display(),
    );
    std::fs::write(&script_path, script)?;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;

    let mut settings = helpers::make_settings(&dbus_instances);
    settings.gondor_bin = script_path;

    let application =
        helpers::spawn_supervisor_service(settings, supervisor_zenorb).await?;
    let _application_handle = tokio::spawn(application.run());

    tokio::time::sleep(Duration::from_millis(300)).await;

    let cases = [
        ("to-1.2.3", vec!["to-1.2.3"]),
        (
            r#"{"version":"to-4.5.6","no_restart":true}"#,
            vec!["to-4.5.6", "--no-restart"],
        ),
        (r#"{"version":"to-7.8.9"}"#, vec!["to-7.8.9"]),
        (
            r#"{"version":"to-0.0.1","no_restart":false}"#,
            vec!["to-0.0.1"],
        ),
    ];

    for (payload, _) in &cases {
        let reply = client_zenorb
            .command_raw("supervisor/job/gondor", payload)
            .await?;
        if let Err(e) = reply {
            let body = String::from_utf8_lossy(&e.payload().to_bytes()).into_owned();
            panic!("payload {payload:?} produced error reply: {body}");
        }
    }

    let log = std::fs::read_to_string(&log_path)?;
    let invocations: Vec<Vec<&str>> = log
        .split("---\n")
        .filter(|s| !s.is_empty())
        .map(|chunk| chunk.lines().collect())
        .collect();

    let expected: Vec<Vec<&str>> = cases.iter().map(|(_, argv)| argv.clone()).collect();
    assert_eq!(
        invocations, expected,
        "gondor invocation argv sequence didn't match"
    );

    Ok(())
}
