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
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    orb_telemetry::TelemetryConfig::new().init();
    tracing::debug!("debug logging is enabled");

    let args = Args::parse();

    match args {
        Args::Alice { .. } => alice(args).await,
        Args::Bob { .. } => bob(args).await,
    }
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
