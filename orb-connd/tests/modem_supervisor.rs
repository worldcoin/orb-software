#![cfg(feature = "testing")]

use color_eyre::{eyre::eyre, Report};
use faux::when;
use fixture::Fixture;
use flume::Receiver;
use orb_connd::{
    mcu_util::{McuUtil, Module},
    modem::{ModemConfig, Snapshot},
    modem_manager::{
        connection_state::ConnectionState, Location, Modem, ModemId, ModemInfo,
        ModemManager, Signal, SimId, SimInfo,
    },
    systemd::Systemd,
    OrbCapabilities,
};
use orb_dogd::test::agent::Agent;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use speare::mini::Ctx;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{fs, time};

mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_publishes_modem_snapshot_on_poll_interval() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let expected = expected_snapshot();
    let registry = crabwire::Registry::new()
        .insert(stable_modem_manager())
        .insert(ModemConfig {
            device_path: fx.container_tempdir.join("cdc-wdm0"),
            poll_interval: Duration::from_secs(1),
            powercycle_grace_period: Duration::ZERO,
        });

    // Act
    let handle = fx.run_with().registry(registry).call().await;
    let snapshots = subscribe_modem_snapshots(&handle.speare);
    time::sleep(Duration::from_millis(2300)).await;

    // Assert
    let snapshots = snapshots.try_iter().collect::<Vec<_>>();

    assert!(
        snapshots.len() >= 2,
        "expected at least 2 snapshots, got {}",
        snapshots.len()
    );

    assert!(snapshots.iter().all(|snapshot| snapshot == &expected));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_powercycles_modem_and_emits_metric_when_snapshot_fails() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let device_path = fx.container_tempdir.join("cdc-wdm0");
    let powercycle_calls = Arc::new(Mutex::new(Vec::new()));
    let restart_calls = Arc::new(Mutex::new(Vec::new()));

    let mut modem_manager = ModemManager::faux();
    when!(modem_manager.list_modems).then(|_| Err(eyre!("snapshot failed")));

    let mut mcu_util = McuUtil::faux();
    let powercycle_calls_cl = Arc::clone(&powercycle_calls);
    when!(mcu_util.powercycle).once().then(move |module| {
        powercycle_calls_cl.lock().unwrap().push(match module {
            Module::Modem => "modem",
        });

        Ok(())
    });

    let mut systemd = Systemd::faux();
    let restart_calls_cl = Arc::clone(&restart_calls);
    when!(systemd.restart_service)
        .once()
        .then(move |(unit, timeout)| {
            restart_calls_cl
                .lock()
                .unwrap()
                .push((unit.to_string(), timeout));

            Ok(())
        });
    when!(systemd.loaded_services).then(|_| Ok(Vec::new()));

    let registry = crabwire::Registry::new()
        .insert(modem_manager)
        .insert(mcu_util)
        .insert(systemd)
        .insert(ModemConfig {
            device_path: device_path.clone(),
            poll_interval: Duration::from_secs(1),
            powercycle_grace_period: Duration::ZERO,
        });

    // Act
    let handle = fx.run_with().registry(registry).call().await;
    fs::write(&device_path, []).await.unwrap();

    // Assert
    wait_for_occurrences(
        &handle.dogstatsd,
        "orb.platform.connd.modem_powercycle:1|c",
        1,
    )
    .await;

    time::sleep(Duration::from_secs(1)).await;

    assert_eq!(powercycle_calls.lock().unwrap().as_slice(), ["modem"]);
    assert_eq!(
        restart_calls.lock().unwrap().as_slice(),
        [("ModemManager.service".to_string(), Duration::from_secs(100))]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_retries_after_restart_service_fails_and_succeeds_on_second_attempt() {
    // Arrange
    let mut fx = Fixture::platform(OrbOsPlatform::Pearl)
        .cap(OrbCapabilities::CellularAndWifi)
        .release(OrbRelease::Dev)
        .build()
        .await;

    let device_path = fx.container_tempdir.join("cdc-wdm0");
    let restart_calls = Arc::new(Mutex::new(Vec::new()));
    let restart_attempt = Arc::new(Mutex::new(0usize));

    let mut modem_manager = ModemManager::faux();
    when!(modem_manager.list_modems).then(|_| Err(eyre!("snapshot failed")));

    let mut mcu_util = McuUtil::faux();
    when!(mcu_util.powercycle).then(|_| Ok(()));

    let mut systemd = Systemd::faux();
    let restart_calls_cl = Arc::clone(&restart_calls);
    let restart_attempt_cl = Arc::clone(&restart_attempt);
    when!(systemd.restart_service).then(move |(unit, timeout)| {
        restart_calls_cl
            .lock()
            .unwrap()
            .push((unit.to_string(), timeout));

        let mut attempt = restart_attempt_cl.lock().unwrap();
        *attempt += 1;

        if *attempt == 1 {
            return Err(eyre!("restart failed"));
        }

        Ok(())
    });
    when!(systemd.loaded_services).then(|_| Ok(Vec::new()));

    let registry = crabwire::Registry::new()
        .insert(modem_manager)
        .insert(mcu_util)
        .insert(systemd)
        .insert(ModemConfig {
            device_path: device_path.clone(),
            poll_interval: Duration::from_millis(1500),
            powercycle_grace_period: Duration::ZERO,
        });

    // Act
    let _handle = fx.run_with().registry(registry).call().await;
    fs::write(&device_path, []).await.unwrap();

    // Assert
    wait_for_restart_attempts(&restart_attempt, 2).await;

    assert_eq!(
        restart_calls.lock().unwrap().as_slice(),
        [
            ("ModemManager.service".to_string(), Duration::from_secs(100)),
            ("ModemManager.service".to_string(), Duration::from_secs(100)),
        ]
    );
}

fn stable_modem_manager() -> ModemManager {
    let mut modem_manager = ModemManager::faux();

    when!(modem_manager.list_modems).then(|_| {
        Ok(vec![Modem {
            id: ModemId::from(7),
            vendor: "telit".to_string(),
            model: "fn980".to_string(),
        }])
    });

    when!(modem_manager.modem_info).then(|_| {
        Ok(ModemInfo {
            imei: "123456789012345".to_string(),
            fw_revision: Some("m0.0.1".to_string()),
            operator_code: Some("26201".to_string()),
            operator_name: Some("Telekom".to_string()),
            access_tech: Some("lte".to_string()),
            state: ConnectionState::Connected,
            sim: Some(SimId::from(0)),
        })
    });

    when!(modem_manager.sim_info).then(|_| {
        Ok(SimInfo {
            iccid: "8988211000000000000".to_string(),
            imsi: "262010000000000".to_string(),
        })
    });

    when!(modem_manager.signal_get).then(|_| {
        Ok(Signal {
            rsrp: Some(-91.0),
            rsrq: Some(-8.0),
            rssi: Some(-63.0),
            snr: Some(18.0),
        })
    });

    when!(modem_manager.location_get).then(|_| {
        Ok(Location {
            cid: Some("100".to_string()),
            lac: Some("200".to_string()),
            mcc: Some("262".to_string()),
            mnc: Some("01".to_string()),
            tac: Some("300".to_string()),
        })
    });

    when!(modem_manager.signal_setup).then(|(_, _)| Ok(()));
    when!(modem_manager.set_current_bands).then(|(_, _)| Ok(()));
    when!(modem_manager.set_allowed_and_preferred_modes).then(|(_, _, _)| Ok(()));

    modem_manager
}

fn expected_snapshot() -> Snapshot {
    Snapshot {
        id: ModemId::from(7),
        fw_revision: Some("m0.0.1".to_string()),
        iccid: Some("8988211000000000000".to_string()),
        imei: "123456789012345".to_string(),
        rat: Some("lte".to_string()),
        operator: Some("Telekom".to_string()),
        state: ConnectionState::Connected,
        signal: Signal {
            rsrp: Some(-91.0),
            rsrq: Some(-8.0),
            rssi: Some(-63.0),
            snr: Some(18.0),
        },
        location: Location {
            cid: Some("100".to_string()),
            lac: Some("200".to_string()),
            mcc: Some("262".to_string()),
            mnc: Some("01".to_string()),
            tac: Some("300".to_string()),
        },
    }
}

fn subscribe_modem_snapshots(speare: &Ctx) -> Receiver<Snapshot> {
    let (tx, rx) = flume::unbounded();

    speare
        .oneshot(async move |ctx| {
            let snapshots = ctx.subscribe::<Snapshot>("modem-snapshot")?;

            while let Ok(snapshot) = snapshots.recv_async().await {
                let _ = tx.send_async(snapshot).await;
            }

            Ok::<(), Report>(())
        })
        .unwrap();

    rx
}

async fn wait_for_occurrences(agent: &Agent, needle: &str, expected: usize) {
    for _ in 0..100 {
        if agent.occurrences(needle) >= expected {
            return;
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(agent.occurrences(needle), expected);
}

async fn wait_for_restart_attempts(attempts: &Mutex<usize>, expected: usize) {
    for _ in 0..100 {
        if *attempts.lock().unwrap() >= expected {
            return;
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(*attempts.lock().unwrap(), expected);
}
