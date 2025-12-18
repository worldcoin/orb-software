use std::{pin::pin, time::Duration};

use clap::Parser as _;
use color_eyre::Result;
use zenoh::handlers::DefaultHandler;

#[derive(clap::Parser)]
enum Args {
    Alice {
        payload_size: usize,
    },
    Bob {
        #[clap(long)]
        use_contiguous: bool,
    },
    Sub {
        #[clap(long)]
        key: String,
        #[clap(long)]
        connect: Option<String>,
    },
    Query {
        #[clap(long)]
        key: String,
        #[clap(long)]
        connect: Option<String>,
    },
    Pub {
        #[clap(long)]
        key: String,
        #[clap(long)]
        value: String,
        #[clap(long)]
        connect: Option<String>,
    },
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let telemetry = orb_telemetry::TelemetryConfig::new().init();
    tracing::debug!("debug logging is enabled");

    let args = Args::parse();

    let result = match args {
        Args::Alice { .. } => alice(args).await,
        Args::Bob { .. } => bob(args).await,
        Args::Sub { .. } => sub(args).await,
        Args::Query { .. } => query(args).await,
        Args::Pub { .. } => publish(args).await,
    };
    telemetry.flush().await;
    result
}

async fn sub(args: Args) -> color_eyre::Result<()> {
    let Args::Sub {
        key: zenoh_key,
        connect,
    } = args
    else {
        unreachable!()
    };

    let connect_config = connect
        .map(|addr| format!(r#"connect: {{ endpoints: ["tcp/{}"] }},"#, addr))
        .unwrap_or_default();

    let cfg = zenoh::Config::from_json5(&format!(
        r#"{{
            mode: "peer",
            {connect_config}
        }}"#
    ))
    .unwrap_or_else(|e| panic!("failed to parse config: {}", e));

    let session = zenoh::open(cfg)
        .await
        .unwrap_or_else(|e| panic!("failed to open session: {}", e));
    let sub = session
        .declare_subscriber(&zenoh_key)
        .await
        .unwrap_or_else(|e| panic!("failed to declare subscriber: {}", e));

    tracing::info!("Subscribed to {zenoh_key}");
    while let Ok(sample) = sub.recv_async().await {
        if let Ok(payload_str) = sample.payload().try_to_string() {
            tracing::info!("recv key={} payload={:?}", sample.key_expr(), payload_str);
        } else {
            tracing::info!(
                "recv key={} payload={:?}",
                sample.key_expr(),
                sample.payload()
            );
        }
    }

    Ok(())
}

async fn query(args: Args) -> color_eyre::Result<()> {
    let Args::Query {
        key: zenoh_key,
        connect,
    } = args
    else {
        unreachable!()
    };

    let connect_config = connect
        .map(|addr| format!(r#"connect: {{ endpoints: ["tcp/{}"] }},"#, addr))
        .unwrap_or_default();

    let cfg = zenoh::Config::from_json5(&format!(
        r#"{{
            mode: "peer",
            {connect_config}
        }}"#
    ))
    .unwrap_or_else(|e| panic!("failed to parse config: {}", e));

    let session = zenoh::open(cfg)
        .await
        .unwrap_or_else(|e| panic!("failed to open session: {}", e));

    tracing::info!("Querying key: {zenoh_key}");
    let replies = session
        .get(&zenoh_key)
        .await
        .unwrap_or_else(|e| panic!("failed to query: {}", e));

    while let Ok(reply) = replies.recv_async().await {
        match reply.result() {
            Ok(sample) => {
                if let Ok(payload_str) = sample.payload().try_to_string() {
                    println!("key={} payload={}", sample.key_expr(), payload_str);
                } else {
                    println!(
                        "key={} payload={:?}",
                        sample.key_expr(),
                        sample.payload()
                    );
                }
            }
            Err(err) => {
                tracing::warn!("Query error for key {}: {:?}", zenoh_key, err);
            }
        }
    }

    Ok(())
}

async fn publish(args: Args) -> color_eyre::Result<()> {
    let Args::Pub {
        key: zenoh_key,
        value,
        connect,
    } = args
    else {
        unreachable!()
    };

    let connect_config = connect
        .map(|addr| format!(r#"connect: {{ endpoints: ["tcp/{}"] }},"#, addr))
        .unwrap_or_default();

    let cfg = zenoh::Config::from_json5(&format!(
        r#"{{
            mode: "peer",
            {connect_config}
        }}"#
    ))
    .unwrap_or_else(|e| panic!("failed to parse config: {}", e));

    let session = zenoh::open(cfg)
        .await
        .unwrap_or_else(|e| panic!("failed to open session: {}", e));

    tracing::info!("Publishing to key: {zenoh_key}");
    session
        .put(&zenoh_key, value.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("failed to put: {}", e));

    tracing::info!("Published value: {value}");

    Ok(())
}

async fn alice(args: Args) -> Result<()> {
    let Args::Alice { payload_size } = args else {
        unreachable!()
    };

    let session = zenoh::open(zenoh::Config::default())
        .await
        .expect("failed to open zenoh session");

    let put_key = session.declare_keyexpr("alice/put/a").await.unwrap();
    let payload: Vec<u8> = (0..payload_size).map(|v| v as u8).collect();
    let publisher = session
        .declare_publisher(put_key)
        .congestion_control(zenoh::qos::CongestionControl::Block)
        .encoding(zenoh::bytes::Encoding::ZENOH_BYTES)
        .await
        .unwrap();

    let i = std::cell::Cell::new(0);
    let mut send_fut = pin!(async {
        loop {
            let () = publisher.put(&payload).await.expect("failed to put");
            i.set(i.get() + 1);
        }
    });
    let interval_duration = Duration::from_millis(1000);
    let mut interval = tokio::time::interval(interval_duration);
    loop {
        tokio::select! {
            biased; _ = interval.tick() => {
                let bytes_per_second = i.replace(0) * interval_duration.as_millis() as u64 * payload_size as u64 / 1000 ;
                let mib_per_second: u64 = bytes_per_second >> 20;
                tracing::info!("MiB/s: {mib_per_second}");
            },
            _ = &mut send_fut => break,
        }
    }

    Ok(())
}

async fn bob(args: Args) -> Result<()> {
    let Args::Bob { use_contiguous } = args else {
        unreachable!();
    };
    let session = zenoh::open(zenoh::Config::default())
        .await
        .expect("failed to open zenoh session");

    let get_key = session.declare_keyexpr("alice/put/a").await.unwrap();

    let subscriber = session
        .declare_subscriber(get_key)
        .with(DefaultHandler::default())
        .await
        .expect("failed to create subscriber");

    let byte_counter = std::cell::Cell::new(0u64);
    let mut recv_fut = pin!(async {
        loop {
            let sample = subscriber.recv_async().await.expect("failed to get");
            let nbytes = if use_contiguous {
                sample.payload().to_bytes().len()
            } else {
                sample.payload().slices().fold(0, |acc, s| acc + s.len())
            };
            byte_counter.set(byte_counter.get() + nbytes as u64);
        }
    });
    let interval_duration = Duration::from_millis(1000);
    let mut interval = tokio::time::interval(interval_duration);
    loop {
        tokio::select! {
            biased; _ = interval.tick() => {
                let bytes_per_second = byte_counter.replace(0) * interval_duration.as_millis() as u64 / 1000 ;
                let mib_per_second: u64 = bytes_per_second >> 20;
                tracing::info!("MiB/s: {mib_per_second}");
            },
            _ = &mut recv_fut => break,
        }
    }

    Ok(())
}
