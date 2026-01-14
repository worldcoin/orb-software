use orb_info::{orb_os_release::OrbRelease, OrbId};
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use test_utils::async_bag::AsyncBag;
use tokio::time;
use zenoh::{bytes::Encoding, query::Query, sample::Sample};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Msg {
    keyexpr: String,
    bytes: Vec<u8>,
}

impl From<&Sample> for Msg {
    fn from(value: &Sample) -> Self {
        Msg {
            keyexpr: value.key_expr().to_string(),
            bytes: value.payload().to_bytes().to_vec(),
        }
    }
}

impl From<&Query> for Msg {
    fn from(value: &Query) -> Self {
        Msg {
            keyexpr: value.key_expr().to_string(),
            bytes: value
                .payload()
                .map(|p| p.to_bytes().to_vec())
                .unwrap_or_default(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_werks() {
    let port = portpicker::pick_unused_port().expect("No ports free");
    let _router = zenoh::open(zenorb::router_cfg(port)).await.unwrap();

    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let bananas = zenorb::Session::from_cfg(client_cfg.clone())
        .env(OrbRelease::Dev)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .for_service("bananasvc")
        .await
        .unwrap();

    let apples = zenorb::Session::from_cfg(client_cfg)
        .env(OrbRelease::Dev)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .for_service("applesvc")
        .await
        .unwrap();

    let sender = bananas
        .sender()
        .publisher("bytestopic")
        .publisher_with("texttopic", |p| p.encoding(Encoding::TEXT_PLAIN))
        .querier("applesvc/get_msgs")
        .build()
        .await
        .unwrap();

    let received_msgs: AsyncBag<Vec<Msg>> = AsyncBag::new(vec![]);

    apples
        .receiver(received_msgs)
        .subscriber("bananasvc/bytestopic", async |ctx, sample| {
            ctx.lock().await.push((&sample).into());
            Ok(())
        })
        .subscriber("bananasvc/texttopic", async |ctx, sample| {
            if sample.encoding() == &Encoding::TEXT_PLAIN {
                ctx.lock().await.push((&sample).into());
            }

            Ok(())
        })
        .queryable("get_msgs", async |ctx, query| {
            let mut msgs = ctx.lock().await;
            msgs.push((&query).into());

            query
                .reply(
                    query.key_expr().clone(),
                    serde_json::to_string(&(*msgs)).unwrap(),
                )
                .encoding(Encoding::TEXT_JAVASCRIPT)
                .await
                .unwrap();

            Ok(())
        })
        .run()
        .await
        .unwrap();

    // give it enough time for zenoh sessions to do their thing
    time::sleep(Duration::from_millis(500)).await;

    // Act
    sender
        .publisher("bytestopic")
        .unwrap()
        .put(b"bytespayload")
        .await
        .unwrap();

    sender
        .publisher("texttopic")
        .unwrap()
        .put(b"textpayload")
        .encoding(Encoding::TEXT_PLAIN)
        .await
        .unwrap();

    let res = sender
        .querier("applesvc/get_msgs")
        .unwrap()
        .get()
        .await
        .unwrap()
        .recv_async()
        .await
        .unwrap()
        .into_result()
        .unwrap();

    let actual: Vec<Msg> = serde_json::from_slice(&res.payload().to_bytes()).unwrap();

    // Assert
    let expected = vec![
        Msg {
            keyexpr: "dev/ea2ea744/bananasvc/bytestopic".to_string(),
            bytes: b"bytespayload".to_vec(),
        },
        Msg {
            keyexpr: "dev/ea2ea744/bananasvc/texttopic".to_string(),
            bytes: b"textpayload".to_vec(),
        },
        Msg {
            keyexpr: "dev/ea2ea744/applesvc/get_msgs".to_string(),
            bytes: vec![],
        },
    ];

    assert_eq!(actual, expected);
}
