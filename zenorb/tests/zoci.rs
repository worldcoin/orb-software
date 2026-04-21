use orb_info::OrbId;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use tokio::time;
use zenorb::{zoci::{ReplyExt, ZociQueryExt}, Zenorb};

mod routerfx;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
struct StatusRequest {
    id: u64,
    label: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_raw_and_res_work_in_receiver_queryable() {
    let (_router, port) = routerfx::run().await;
    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let red = Zenorb::from_cfg(client_cfg.clone())
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("red")
        .await
        .unwrap();

    let blue = Zenorb::from_cfg(client_cfg)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("blue")
        .await
        .unwrap();

    let sender = red.sender().querier("blue/tuple").build().await.unwrap();

    blue.receiver(())
        .queryable("tuple", async |_ctx, query| {
            let actual: (String, String) = query.args()?;
            query.res(&actual).await?;

            Ok(())
        })
        .run()
        .await
        .unwrap();

    time::sleep(Duration::from_millis(300)).await;

    // Act
    let actual: Result<(String, String), StatusRequest> = sender
        .command_raw("blue/tuple", "one two")
        .await
        .unwrap()
        .json()
        .unwrap();

    // Assert
    assert_eq!(actual.unwrap(), ("one".to_string(), "two".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sender_command_serializes_payload_for_json() {
    let (_router, port) = routerfx::run().await;
    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let red = Zenorb::from_cfg(client_cfg.clone())
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("red")
        .await
        .unwrap();

    let blue = Zenorb::from_cfg(client_cfg)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("blue")
        .await
        .unwrap();

    let sender = red.sender().querier("blue/status").build().await.unwrap();

    blue.receiver(())
        .queryable("status", async |_ctx, query| {
            let actual: StatusRequest = query.json()?;
            query.res(&actual).await?;

            Ok(())
        })
        .run()
        .await
        .unwrap();

    time::sleep(Duration::from_millis(300)).await;

    let expected = StatusRequest {
        id: 7,
        label: "banana".to_string(),
    };

    // Act
    let actual: Result<StatusRequest, StatusRequest> = sender
        .command("blue/status", &expected)
        .await
        .unwrap()
        .json()
        .unwrap();

    // Assert
    assert_eq!(actual.unwrap(), expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zenorb_command_returns_reply_errors_from_declared_queryables() {
    let (_router, port) = routerfx::run().await;
    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let red = Zenorb::from_cfg(client_cfg.clone())
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("red")
        .await
        .unwrap();

    let blue = Zenorb::from_cfg(client_cfg)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("blue")
        .await
        .unwrap();

    let queryable = blue.declare_queryable("status").await.unwrap();

    let task = tokio::spawn(async move {
        let query = queryable.recv_async().await.unwrap();
        let actual: StatusRequest = query.json()?;
        query.res_err(&actual).await?;

        Ok::<(), color_eyre::Report>(())
    });

    time::sleep(Duration::from_millis(300)).await;

    let expected = StatusRequest {
        id: 9,
        label: "apple".to_string(),
    };

    // Act
    let actual: Result<StatusRequest, StatusRequest> =
        red.command("status", &expected).await.unwrap().json().unwrap();

    task.await.unwrap().unwrap();

    // Assert
    assert_eq!(actual.unwrap_err(), expected);
}
