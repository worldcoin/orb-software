#![cfg(feature = "testing")]

use faux::when;
use fixture::Fixture;
use orb_connd::systemd::Systemd;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_powers_on_adapter_if_off() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(false)
        .mock_adapter_set_powered(true);

    // Act
    let handle = fixture.run().await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_alternates_between_two_payloads() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(4)
        .mock_unregister_advertisements(3);

    let payloads = [
        (Uuid::from_u128(1).to_string(), vec![1_u8, 2, 3]),
        (Uuid::from_u128(2).to_string(), vec![4_u8, 5, 6]),
    ];

    // Act
    let handle = fixture.run().await;
    for (service_id, payload) in &payloads {
        handle.publish_ble(service_id, Some(payload)).await;
    }

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(5))
        .await
        .unwrap();

    let advertisements = handle.bluez.advertisements();

    assert_eq!(advertisements.len(), 4);
    assert!(advertisements
        .iter()
        .all(|advert| payloads.contains(advert)));
    assert!(advertisements.windows(2).all(|pair| pair[0] != pair[1]));

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_does_not_power_on_an_already_powered_adapter() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture.bluez.mock_adapter().mock_adapter_is_powered(true);

    // Act
    let handle = fixture.run().await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_keeps_one_unchanged_payload_advertising() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(1);

    let service_id = Uuid::from_u128(1).to_string();
    let payload = vec![1, 2, 3];

    // Act
    let handle = fixture.run().await;
    handle.publish_ble(&service_id, Some(&payload)).await;
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();
    handle.publish_ble(&service_id, Some(&payload)).await;

    // Assert
    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;

    assert_eq!(
        handle.bluez.advertisements(),
        vec![(service_id.clone(), payload.clone())]
    );
    assert_eq!(
        handle.bluez.active_advertisements(),
        vec![(service_id, payload)]
    );

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_ignores_payloads_that_exceed_the_legacy_advertising_budget() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture.bluez.mock_adapter().mock_adapter_is_powered(true);

    let service_id = Uuid::from_u128(1).to_string();

    // Act
    let handle = fixture.run().await;
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();
    handle.publish_ble(&service_id, Some(&[0; 11])).await;

    // Assert
    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;
    assert!(handle.bluez.advertisements().is_empty());

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_replaces_an_updated_payload_for_the_same_service() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(1);

    let service_id = Uuid::from_u128(1).to_string();
    let original = vec![1, 2, 3];
    let updated = vec![4, 5, 6];

    // Act
    let handle = fixture.run().await;

    handle.publish_ble(&service_id, Some(&original)).await;
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();
    handle
        .bluez
        .mock_unregister_advertisements(1)
        .mock_advertisements(1);

    handle.publish_ble(&service_id, Some(&updated)).await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    assert_eq!(
        handle.bluez.advertisements(),
        vec![
            (service_id.clone(), original),
            (service_id.clone(), updated.clone()),
        ]
    );
    assert_eq!(
        handle.bluez.active_advertisements(),
        vec![(service_id, updated)]
    );

    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_removes_one_service_and_keeps_the_other_advertising() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(4)
        .mock_unregister_advertisements(3);

    let payloads = [
        (Uuid::from_u128(1).to_string(), vec![1, 2, 3]),
        (Uuid::from_u128(2).to_string(), vec![4, 5, 6]),
    ];

    // Act
    let handle = fixture.run().await;
    for (service_id, payload) in &payloads {
        handle.publish_ble(service_id, Some(payload)).await;
    }

    handle
        .bluez
        .wait_all_called(Duration::from_secs(5))
        .await
        .unwrap();

    let active = handle.bluez.active_advertisements();
    assert_eq!(active.len(), 1);

    let removed = &active[0].0;
    let remaining = payloads
        .iter()
        .find(|(id, _)| id != removed)
        .unwrap()
        .clone();

    handle
        .bluez
        .mock_unregister_advertisements(1)
        .mock_advertisements(1);

    handle.publish_ble(removed, None).await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(handle.bluez.active_advertisements(), vec![remaining]);

    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_unregisters_the_last_removed_service() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(1);

    let service_id = Uuid::from_u128(1).to_string();

    // Act
    let handle = fixture.run().await;
    handle.publish_ble(&service_id, Some(&[1, 2, 3])).await;
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    handle.bluez.mock_unregister_advertisements(1);
    handle.publish_ble(&service_id, None).await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    assert!(handle.bluez.active_advertisements().is_empty());

    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;

    // Cleanup
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_ignores_malformed_json_and_accepts_the_next_payload() {
    // Arrange
    let mut fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .build()
        .await;

    fixture
        .bluez
        .mock_adapter()
        .mock_adapter_is_powered(true)
        .mock_advertisements(1);

    let service_id = Uuid::from_u128(1).to_string();
    let payload = vec![1, 2, 3];

    // Act
    let handle = fixture.run().await;
    {
        let publisher = handle.ble_publisher().await;
        publisher.put("{invalid json").await.unwrap();
    }
    handle.publish_ble(&service_id, Some(&payload)).await;

    // Assert
    handle
        .bluez
        .wait_all_called(Duration::from_secs(1))
        .await
        .unwrap();

    assert_eq!(handle.bluez.advertisements(), vec![(service_id, payload)]);

    // Cleanup
    handle.stop().await;
}

async fn interval_fixture(fail_first: bool) -> (Fixture, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let mut systemd = Systemd::faux();
    when!(systemd.restart_service).then(move |(unit, timeout)| {
        assert_eq!(timeout, Duration::from_secs(20));
        let mut calls = recorded.lock().unwrap();
        calls.push(unit.to_owned());
        if fail_first && calls.len() == 1 {
            return Err(color_eyre::eyre::eyre!("helper failed"));
        }
        Ok(())
    });
    when!(systemd.loaded_services).then(|_| Ok(Vec::new()));
    let fixture = Fixture::platform(OrbOsPlatform::Diamond)
        .bluetooth(true)
        .release(OrbRelease::Dev)
        .registry(crabwire::Registry::new().insert(systemd))
        .build()
        .await;
    fixture.bluez.mock_adapter().mock_adapter_is_powered(true);
    (fixture, calls)
}

async fn wait_for_ble(handle: &fixture::FxHandle, timeout_secs: u64) {
    handle
        .bluez
        .wait_all_called(Duration::from_secs(timeout_secs))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_applies_adapter_defaults_and_restores_the_same_beacon_after_restart() {
    let (mut fixture, calls) = interval_fixture(false).await;
    fixture.bluez.mock_advertisements(1);
    let service_id = Uuid::from_u128(1).to_string();
    let payload = vec![1, 2, 3];
    let beacon = vec![(service_id.clone(), payload.clone())];
    let handle = fixture.run().await;
    handle.publish_ble(&service_id, Some(&payload)).await;
    wait_for_ble(&handle, 3).await;
    assert!(calls.lock().unwrap().is_empty());

    let mut expected_units = Vec::new();
    for (interval, instance) in [
        (Some(20), "20"),
        (Some(100), "100"),
        (None, "default"),
        (Some(10_240), "10240"),
    ] {
        handle
            .bluez
            .mock_unregister_advertisements(1)
            .mock_no_adapter()
            .mock_adapter_after_restart()
            .mock_advertisements(1);
        handle
            .publish_ble_with_interval(&service_id, Some(&payload), interval)
            .await;
        wait_for_ble(&handle, 3).await;
        expected_units.push(format!("worldcoin-bluetooth-interval@{instance}.service"));
        assert_eq!(*calls.lock().unwrap(), expected_units);
        assert_eq!(handle.bluez.active_advertisements(), beacon);

        handle
            .publish_ble_with_interval(&service_id, Some(&payload), interval)
            .await;
        handle.publish_ble(&service_id, Some(&payload)).await;
        handle
            .bluez
            .assert_no_calls_for(Duration::from_millis(1_100))
            .await;
        assert_eq!(*calls.lock().unwrap(), expected_units);
    }
    handle
        .publish_ble_with_interval(&service_id, Some(&[4, 5, 6]), Some(19))
        .await;
    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;
    assert_eq!(handle.bluez.active_advertisements(), beacon);

    handle.bluez.mock_unregister_advertisements(1);
    handle
        .publish_ble_with_interval(&service_id, None, None)
        .await;
    wait_for_ble(&handle, 3).await;
    handle
        .bluez
        .assert_no_calls_for(Duration::from_millis(1_100))
        .await;
    assert!(handle.bluez.active_advertisements().is_empty());
    handle.bluez.mock_advertisements(1);
    handle.publish_ble(&service_id, Some(&payload)).await;
    wait_for_ble(&handle, 3).await;
    assert_eq!(*calls.lock().unwrap(), expected_units);
    assert_eq!(handle.bluez.active_advertisements(), beacon);
    handle.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_retries_helper_and_bluez_failures_without_a_new_message() {
    let (mut fixture, calls) = interval_fixture(true).await;
    fixture.bluez.mock_advertisements(1);
    let service_id = Uuid::from_u128(1).to_string();
    let payload = vec![1, 2, 3];
    let beacon = vec![(service_id.clone(), payload.clone())];
    let handle = fixture.run().await;
    handle.publish_ble(&service_id, Some(&payload)).await;
    wait_for_ble(&handle, 2).await;

    handle
        .bluez
        .mock_unregister_advertisements(1)
        .mock_adapter_after_restart()
        .mock_advertisements(1);
    handle
        .publish_ble_with_interval(&service_id, Some(&payload), Some(20))
        .await;
    wait_for_ble(&handle, 2).await;
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(handle.bluez.active_advertisements(), beacon);
    handle
        .bluez
        .mock_unregister_advertisements(1)
        .mock_adapter_after_restart()
        .mock_advertisements(1);
    wait_for_ble(&handle, 32).await;
    assert_eq!(handle.bluez.active_advertisements(), beacon);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            "worldcoin-bluetooth-interval@20.service",
            "worldcoin-bluetooth-interval@20.service",
        ]
    );

    handle
        .bluez
        .mock_unregister_advertisements(1)
        .mock_adapter()
        .mock_adapter_is_powered(false)
        .mock_adapter_set_powered_error(true, "org.bluez.Error.Failed");
    handle
        .publish_ble_with_interval(&service_id, Some(&payload), Some(100))
        .await;
    wait_for_ble(&handle, 2).await;
    assert_eq!(calls.lock().unwrap().len(), 3);
    handle
        .bluez
        .mock_adapter_after_restart()
        .mock_advertisements(1);
    wait_for_ble(&handle, 32).await;
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            "worldcoin-bluetooth-interval@20.service",
            "worldcoin-bluetooth-interval@20.service",
            "worldcoin-bluetooth-interval@100.service",
        ]
    );
    assert_eq!(handle.bluez.active_advertisements(), beacon);
    handle.stop().await;
}
