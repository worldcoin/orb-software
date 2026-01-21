use orb_info::OrbId;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use test_utils::async_bag::AsyncBag;
use tokio::time;
use zenoh::{bytes::Encoding, query::Query, sample::Sample};

mod routerfx;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_werks() {
    let (_router, port) = routerfx::run().await;
    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let bananas = zenorb::Session::from_cfg(client_cfg.clone())
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("bananasvc")
        .await
        .unwrap();

    let apples = zenorb::Session::from_cfg(client_cfg)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("applesvc")
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

    // give it enough time for subscriber session to subscribe
    // and receive messages
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
            keyexpr: "ea2ea744/bananasvc/bytestopic".to_string(),
            bytes: b"bytespayload".to_vec(),
        },
        Msg {
            keyexpr: "ea2ea744/bananasvc/texttopic".to_string(),
            bytes: b"textpayload".to_vec(),
        },
        Msg {
            keyexpr: "ea2ea744/applesvc/get_msgs".to_string(),
            bytes: vec![],
        },
    ];

    assert_eq!(actual, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn querying_subscriber_gets_cached_msg_from_router() {
    let (_router, port) = routerfx::run().await;
    let client_cfg = zenorb::client_cfg(port);

    // Arrange
    let red = zenorb::Session::from_cfg(client_cfg.clone())
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("red")
        .await
        .unwrap();

    let blue = zenorb::Session::from_cfg(client_cfg)
        .orb_id(OrbId::from_str("ea2ea744").unwrap())
        .with_name("blue")
        .await
        .unwrap();

    let sender = red
        .sender()
        .publisher_with("text", |p| p.encoding(Encoding::TEXT_PLAIN))
        .build()
        .await
        .unwrap();

    // Act: no subscriber except router, this should be cached
    sender
        .publisher("text")
        .unwrap()
        .put(b"vermelho")
        .await
        .unwrap();

    sender
        .publisher("text")
        .unwrap()
        .put(b"rouge")
        .await
        .unwrap();

    let received_msgs: AsyncBag<Vec<Msg>> = AsyncBag::new(vec![]);

    // make sure all messages are sent before we start the subscriber
    time::sleep(Duration::from_millis(300)).await;

    // Act: late subscription, message was already published at this point
    blue.receiver(received_msgs.clone())
        .querying_subscriber(
            "red/text",
            Duration::from_millis(100),
            async |ctx, sample| {
                if sample.encoding() == &Encoding::TEXT_PLAIN {
                    ctx.lock().await.push((&sample).into());
                }

                Ok(())
            },
        )
        .run()
        .await
        .unwrap();

    // give it enough time for subscriber session to subscribe
    // and receive messages. remember, querying subscriber will block
    // for the query timeout before handling subscription messages
    time::sleep(Duration::from_millis(200)).await;

    // Assert we only get the last value
    let actual = received_msgs.read().await;
    let expected = vec![Msg {
        keyexpr: "ea2ea744/red/text".to_string(),
        bytes: b"rouge".to_vec(),
    }];

    assert_eq!(actual, expected);
}
