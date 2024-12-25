#![allow(dead_code)]

use clap::Parser;
use eyre::{Ok, Result};
use orb_relay_client::{client::Client, debug_any, PayloadMatcher};
use orb_relay_messages::{common, self_serve};
use rand::{distributions::Alphanumeric, Rng};
use std::{
    env,
    sync::LazyLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static BACKEND_URL: LazyLock<String> = LazyLock::new(|| {
    let backend = env::var("RELAY_TOOL_BACKEND").unwrap_or_else(|_| "stage".to_string());
    match backend.as_str() {
        "stage" => "https://relay.stage.orb.worldcoin.org",
        "prod" => "https://relay.orb.worldcoin.org",
        "local" => "http://127.0.0.1:8443",
        _ => panic!("Invalid backend option"),
    }
    .to_string()
});
static APP_KEY: LazyLock<String> = LazyLock::new(|| {
    env::var("RELAY_TOOL_APP_KEY")
        .unwrap_or_else(|_| "OTk3b3RGNTFYMnlYZ0dYODJlNkVZSTZqWlZnOHJUeDI=".to_string())
});
static ORB_KEY: LazyLock<String> = LazyLock::new(|| {
    env::var("RELAY_TOOL_ORB_KEY")
        .unwrap_or_else(|_| "NWZxTTZQRlBwMm15ODhxUjRCS283ZERFMTlzek1ZOTU=".to_string())
});

static ORB_ID: LazyLock<String> =
    LazyLock::new(|| env::var("RELAY_TOOL_ORB_ID").unwrap_or_else(|_| "b222b1a3".to_string()));
static SESSION_ID: LazyLock<String> = LazyLock::new(|| {
    env::var("RELAY_TOOL_SESSION_ID")
        .unwrap_or_else(|_| "6943c6d9-48bf-4f29-9b60-48c63222e3ea".to_string())
});

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Run only the stage_consumer_app function
    #[clap(short = 'c', long = "consume-only")]
    consume_only: bool,
    /// Run only the stage_producer_app function
    #[clap(short = 'p', long = "produce-only")]
    produce_only: bool,
    #[clap(short = 's', long = "start-orb-signup")]
    start_orb_signup: bool,
    #[clap(short = 'w', long = "slow-tests")]
    slow_tests: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();

    if args.consume_only {
        stage_consumer_app().await?;
    } else if args.start_orb_signup {
        stage_producer_from_app_start_orb_signup().await?;
    } else if args.produce_only {
        stage_producer_orb().await?;
    } else {
        app_to_orb().await?;
        orb_to_app().await?;
        orb_to_app_with_state_request().await?;
        orb_to_app_blocking_send().await?;
        if args.slow_tests {
            orb_to_app_with_clients_created_later_and_delay().await?;
        }
    }

    Ok(())
}

async fn app_to_orb() -> Result<()> {
    tracing::info!("== Running App to Orb ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    tracing::info!("Sending time now: {}", time_now);
    app_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now.clone(),
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    tracing::info!("Time took to send a message from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in orb_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(common::v1::AnnounceOrbId { orb_id, .. }) =
                common::v1::AnnounceOrbId::matches(msg.payload.as_ref().unwrap())
            {
                assert!(orb_id == time_now, "Received orb_id is not the same as sent orb_id");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    app_client
        .send(self_serve::orb::v1::SignupEnded { success: true, failure_feedback: [].to_vec() })
        .await?;
    tracing::info!("Time took to send a second message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in orb_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(self_serve::orb::v1::SignupEnded { success, .. }) =
                self_serve::orb::v1::SignupEnded::matches(msg.payload.as_ref().unwrap())
            {
                assert!(success, "Received: success is not true");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a second message: {}ms", now.elapsed().as_millis());

    orb_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;
    app_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;

    Ok(())
}

async fn orb_to_app() -> Result<()> {
    tracing::info!("== Running Orb to App ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    tracing::info!("Sending time now: {}", time_now);
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now.clone(),
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    tracing::info!("Time took to send a message from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(common::v1::AnnounceOrbId { orb_id, .. }) =
                common::v1::AnnounceOrbId::matches(msg.payload.as_ref().unwrap())
            {
                assert!(orb_id == time_now, "Received orb_id is not the same as sent orb_id");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    orb_client
        .send(self_serve::orb::v1::SignupEnded { success: true, failure_feedback: Vec::new() })
        .await?;
    tracing::info!("Time took to send a second message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(self_serve::orb::v1::SignupEnded { success, .. }) =
                self_serve::orb::v1::SignupEnded::matches(msg.payload.as_ref().unwrap())
            {
                assert!(success, "Received: success is not true");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a second message: {}ms", now.elapsed().as_millis());

    orb_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;
    app_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;

    Ok(())
}

async fn orb_to_app_with_state_request() -> Result<()> {
    tracing::info!("== Running Orb to App with state request ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    app_client.send(self_serve::app::v1::RequestState {}).await?;
    tracing::info!("Time took to send RequestState from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            break 'ext;
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    tracing::info!("Sending time now: {}", time_now);
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now,
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    tracing::info!("Time took to send a message from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            break 'ext;
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    app_client.send(self_serve::app::v1::RequestState {}).await?;
    tracing::info!("Time took to send RequestState from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            break 'ext;
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    orb_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;
    app_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;

    Ok(())
}

async fn orb_to_app_blocking_send() -> Result<()> {
    tracing::info!("== Running Orb to App blocking send ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    tracing::info!("Sending time now: {}", time_now);
    orb_client
        .send_blocking(
            common::v1::AnnounceOrbId {
                orb_id: time_now.clone(),
                mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
            },
            Duration::from_secs(5),
        )
        .await?;
    tracing::info!("Time took to send a message from the app: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(common::v1::AnnounceOrbId { orb_id, .. }) =
                common::v1::AnnounceOrbId::matches(msg.payload.as_ref().unwrap())
            {
                assert!(orb_id == time_now, "Received orb_id is not the same as sent orb_id");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    orb_client
        .send_blocking(
            self_serve::orb::v1::SignupEnded { success: true, failure_feedback: Vec::new() },
            Duration::from_secs(5),
        )
        .await?;
    tracing::info!("Time took to send a second message: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            if let Some(self_serve::orb::v1::SignupEnded { success, .. }) =
                self_serve::orb::v1::SignupEnded::matches(msg.payload.as_ref().unwrap())
            {
                assert!(success, "Received: success is not true");
                break 'ext;
            }
            unreachable!("Received unexpected message: {msg:?}");
        }
    }
    tracing::info!("Time took to receive a second message: {}ms", now.elapsed().as_millis());

    orb_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;
    app_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;

    Ok(())
}

async fn orb_to_app_with_clients_created_later_and_delay() -> Result<()> {
    let (orb_id, session_id) = get_ids();

    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    tracing::info!("Sending time now: {}", time_now);
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now,
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    tracing::info!("Time took to send a message from the app: {}ms", now.elapsed().as_millis());

    tracing::info!("Waiting for 60 seconds...");
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    'ext: loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
            break 'ext;
        }
    }
    tracing::info!("Time took to receive a message: {}ms", now.elapsed().as_millis());

    orb_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;
    app_client.graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000)).await;

    Ok(())
}

fn get_ids() -> (String, String) {
    let mut rng = rand::thread_rng();
    let orb_id: String = (&mut rng).sample_iter(Alphanumeric).take(10).map(char::from).collect();
    let session_id: String =
        (&mut rng).sample_iter(Alphanumeric).take(10).map(char::from).collect();
    tracing::info!("Orb ID: {orb_id}, Session ID: {session_id}");
    (orb_id, session_id)
}

fn time_now() -> Result<String> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos().to_string())
}

async fn stage_consumer_app() -> Result<()> {
    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        SESSION_ID.to_string(),
        ORB_ID.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to connect: {}ms", now.elapsed().as_millis());

    loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn stage_producer_orb() -> Result<()> {
    let mut orb_client = Client::new_as_orb(
        BACKEND_URL.to_string(),
        ORB_KEY.to_string(),
        ORB_ID.to_string(),
        SESSION_ID.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    loop {
        let time_now = time_now()?;
        tracing::info!("Sending time now: {}", time_now);
        orb_client
            .send(common::v1::AnnounceOrbId {
                orb_id: time_now,
                mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
            })
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(120)).await;
    }
}

async fn stage_producer_from_app_start_orb_signup() -> Result<()> {
    let mut app_client = Client::new_as_app(
        BACKEND_URL.to_string(),
        APP_KEY.to_string(),
        SESSION_ID.to_string(),
        ORB_ID.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    tracing::info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    tracing::info!("Sending StartCapture now");
    app_client.send(self_serve::app::v1::StartCapture {}).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    loop {
        #[allow(clippy::never_loop)]
        for msg in app_client.get_buffered_messages().await {
            tracing::info!(
                "Received message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                msg.src,
                msg.dst,
                msg.seq,
                debug_any(&msg.payload)
            );
        }
    }
}
