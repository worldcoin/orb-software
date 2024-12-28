use clap::Parser;
use eyre::Result;
use orb_endpoints::{Backend, Endpoints, OrbId};
use orb_relay_client::client::Client;
use orb_relay_messages::{common, self_serve};
use rand::{distributions::Alphanumeric, Rng};
use std::{
    str::FromStr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{error, info};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Run only the stage_consumer_app function
    #[clap(short = 'c', long = "consume-only")]
    consume_only: bool,
    /// Run only the stage_producer_app function
    #[clap(short = 'p', long = "produce-only", conflicts_with = "consume_only")]
    produce_only: bool,
    #[clap(short = 's', long = "start-orb-signup")]
    start_orb_signup: bool,
    #[clap(short = 'w', long = "slow-tests")]
    slow_tests: bool,

    #[clap(long, env = "RELAY_TOOL_ORB_ID", default_value = "b222b1a3")]
    orb_id: String,
    #[clap(
        long,
        env = "RELAY_TOOL_SESSION_ID",
        default_value = "6943c6d9-48bf-4f29-9b60-48c63222e3ea"
    )]
    session_id: String,
    #[clap(long, env = "RELAY_TOOL_BACKEND", default_value = "staging")]
    backend: String,
    #[clap(
        long,
        env = "RELAY_TOOL_APP_KEY",
        default_value = "OTk3b3RGNTFYMnlYZ0dYODJlNkVZSTZqWlZnOHJUeDI="
    )]
    app_key: String,
    #[clap(
        long,
        env = "RELAY_TOOL_ORB_KEY",
        default_value = "NWZxTTZQRlBwMm15ODhxUjRCS283ZERFMTlzek1ZOTU="
    )]
    orb_key: String,
    #[clap(long, env = "RELAY_TOOL_RELAY_NAMESPACE", default_value = "relay-tool")]
    relay_namespace: String,
}

fn backend_url(args: &Args) -> String {
    let backend = Backend::from_str(args.backend.as_str()).unwrap_or(Backend::Staging);
    if backend == Backend::Local {
        "http://127.0.0.1:8443".to_string()
    } else {
        let endpoints =
            Endpoints::new(backend, &OrbId::from_str(&args.orb_id.as_str()).unwrap());
        endpoints.relay.to_string()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    orb_telemetry::TelemetryConfig::new()
        .with_journald("worldcoin-relay-client")
        .init();

    let args = Args::parse();

    if args.consume_only {
        stage_consumer_app(&args).await?;
    } else if args.start_orb_signup {
        stage_producer_from_app_start_orb_signup(&args).await?;
    } else if args.produce_only {
        stage_producer_orb(&args).await?;
    } else {
        app_to_orb(&args).await?;
        orb_to_app(&args).await?;
        orb_to_app_with_state_request(&args).await?;
        orb_to_app_blocking_send(&args).await?;
        if args.slow_tests {
            orb_to_app_with_clients_created_later_and_delay(&args).await?;
        }
    }

    Ok(())
}

async fn app_to_orb(args: &Args) -> Result<()> {
    info!("== Running App to Orb ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    info!("Sending AnnounceOrbId");
    let now = Instant::now();
    let time_now = time_now()?;
    app_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now.clone(),
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    info!(
        "Time took to send a message from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match orb_client
        .wait_for_msg::<common::v1::AnnounceOrbId>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            assert!(
                msg.orb_id == time_now,
                "Received orb_id is not the same as sent orb_id"
            );
        }
        Err(e) => {
            error!("Failed to receive AnnounceOrbId: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    info!("Sending SignupEnded");
    let now = Instant::now();
    app_client
        .send(self_serve::orb::v1::SignupEnded {
            success: true,
            failure_feedback: [].to_vec(),
        })
        .await?;
    info!(
        "Time took to send a second message: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match orb_client
        .wait_for_msg::<self_serve::orb::v1::SignupEnded>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            assert!(msg.success, "Received: success is not true");
        }
        Err(e) => {
            error!("Failed to receive SignupEnded: {:?}", e);
        }
    }
    info!(
        "Time took to receive a second message: {}ms",
        now.elapsed().as_millis()
    );

    orb_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;
    app_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;

    Ok(())
}

async fn orb_to_app(args: &Args) -> Result<()> {
    info!("== Running Orb to App ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    info!("Sending AnnounceOrbId");
    let now = Instant::now();
    let time_now = time_now()?;
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now.clone(),
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    info!(
        "Time took to send a message from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_msg::<common::v1::AnnounceOrbId>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            assert!(
                msg.orb_id == time_now,
                "Received orb_id is not the same as sent orb_id"
            );
        }
        Err(e) => {
            error!("Failed to receive AnnounceOrbId: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    info!("Sending SignupEnded");
    let now = Instant::now();
    orb_client
        .send(self_serve::orb::v1::SignupEnded {
            success: true,
            failure_feedback: Vec::new(),
        })
        .await?;
    info!(
        "Time took to send a second message: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_msg::<self_serve::orb::v1::SignupEnded>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            assert!(msg.success, "Received: success is not true");
        }
        Err(e) => {
            error!("Failed to receive SignupEnded: {:?}", e);
        }
    }
    info!(
        "Time took to receive a second message: {}ms",
        now.elapsed().as_millis()
    );

    orb_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;
    app_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;

    Ok(())
}

async fn orb_to_app_with_state_request(args: &Args) -> Result<()> {
    info!("== Running Orb to App with state request ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    info!("Sending RequestState");
    let now = Instant::now();
    app_client
        .send(self_serve::app::v1::RequestState {})
        .await?;
    info!(
        "Time took to send RequestState from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_payload(Duration::from_millis(1000))
        .await
    {
        Ok(_) => {
            info!("Received RelayPayload");
        }
        Err(e) => {
            error!("Failed to receive RelayPayload: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    info!("Sending AnnounceOrbId");
    let now = Instant::now();
    let time_now = time_now()?;
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now.clone(),
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    info!(
        "Time took to send a message from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_msg::<common::v1::AnnounceOrbId>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            info!("Received AnnounceOrbId: {:?}", msg);
            assert!(
                msg.orb_id == time_now,
                "Received orb_id is not the same as sent orb_id"
            );
        }
        Err(e) => {
            error!("Failed to receive AnnounceOrbId: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    info!("Sending RequestState");
    let now = Instant::now();
    app_client
        .send(self_serve::app::v1::RequestState {})
        .await?;
    info!(
        "Time took to send RequestState from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_payload(Duration::from_millis(1000))
        .await
    {
        Ok(_) => {
            info!("Received RelayPayload");
        }
        Err(e) => {
            error!("Failed to receive RelayPayload: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    orb_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;
    app_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;

    Ok(())
}

async fn orb_to_app_blocking_send(args: &Args) -> Result<()> {
    info!("== Running Orb to App blocking send ==");
    let (orb_id, session_id) = get_ids();

    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    let time_now = time_now()?;
    info!("Sending AnnounceOrbId");
    orb_client
        .send_blocking(
            common::v1::AnnounceOrbId {
                orb_id: time_now.clone(),
                mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                hardware_type: common::v1::announce_orb_id::HardwareType::Diamond
                    .into(),
            },
            Duration::from_secs(5),
        )
        .await?;
    info!(
        "Time took to send a message from the app: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_msg::<common::v1::AnnounceOrbId>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            info!("Received AnnounceOrbId: {:?}", msg);
            assert!(
                msg.orb_id == time_now,
                "Received orb_id is not the same as sent orb_id"
            );
        }
        Err(e) => {
            error!("Failed to receive AnnounceOrbId: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    orb_client
        .send_blocking(
            self_serve::orb::v1::SignupEnded {
                success: true,
                failure_feedback: Vec::new(),
            },
            Duration::from_secs(5),
        )
        .await?;
    info!(
        "Time took to send a second message: {}ms",
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    match app_client
        .wait_for_msg::<self_serve::orb::v1::SignupEnded>(Duration::from_millis(1000))
        .await
    {
        Ok(msg) => {
            info!("Received SignupEnded: {:?}", msg);
            assert!(msg.success, "Received: success is not true");
        }
        Err(e) => {
            error!("Failed to receive SignupEnded: {:?}", e);
        }
    }
    info!(
        "Time took to receive a second message: {}ms",
        now.elapsed().as_millis()
    );

    orb_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;
    app_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;

    Ok(())
}

async fn orb_to_app_with_clients_created_later_and_delay(args: &Args) -> Result<()> {
    let (orb_id, session_id) = get_ids();

    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        orb_id.to_string(),
        session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    info!("Sending AnnounceOrbId");
    let now = Instant::now();
    let time_now = time_now()?;
    orb_client
        .send(common::v1::AnnounceOrbId {
            orb_id: time_now,
            mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
            hardware_type: common::v1::announce_orb_id::HardwareType::Diamond.into(),
        })
        .await?;
    info!(
        "Time took to send a message from the app: {}ms",
        now.elapsed().as_millis()
    );

    info!("Waiting for 60 seconds...");
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        session_id.to_string(),
        orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to app_connect: {}ms", now.elapsed().as_millis());

    let now = Instant::now();
    match app_client
        .wait_for_payload(Duration::from_millis(1000))
        .await
    {
        Ok(_) => {
            info!("Received AnnounceOrbId");
        }
        Err(e) => {
            error!("Failed to receive AnnounceOrbId: {:?}", e);
        }
    }
    info!(
        "Time took to receive a message: {}ms",
        now.elapsed().as_millis()
    );

    orb_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;
    app_client
        .graceful_shutdown(Duration::from_millis(500), Duration::from_millis(1000))
        .await;

    Ok(())
}

fn get_ids() -> (String, String) {
    let mut rng = rand::thread_rng();
    let orb_id: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    let session_id: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    info!("Orb ID: {orb_id}, Session ID: {session_id}");
    (orb_id, session_id)
}

fn time_now() -> Result<String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string())
}

async fn stage_consumer_app(args: &Args) -> Result<()> {
    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        args.session_id.to_string(),
        args.orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to connect: {}ms", now.elapsed().as_millis());

    loop {
        #[expect(clippy::never_loop)]
        match app_client
            .wait_for_payload(Duration::from_millis(1000))
            .await
        {
            Ok(_) => {
                info!("Received RelayPayload");
            }
            Err(e) => {
                error!("Failed to receive RelayPayload: {:?}", e);
            }
        }
    }
}

async fn stage_producer_orb(args: &Args) -> Result<()> {
    let mut orb_client = Client::new_as_orb(
        backend_url(&args),
        args.orb_key.to_string(),
        args.orb_id.to_string(),
        args.session_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    orb_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    loop {
        info!("Sending AnnounceOrbId");
        let time_now = time_now()?;
        orb_client
            .send(common::v1::AnnounceOrbId {
                orb_id: time_now,
                mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                hardware_type: common::v1::announce_orb_id::HardwareType::Diamond
                    .into(),
            })
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(120)).await;
    }
}

async fn stage_producer_from_app_start_orb_signup(args: &Args) -> Result<()> {
    let mut app_client = Client::new_as_app(
        backend_url(&args),
        args.app_key.to_string(),
        args.session_id.to_string(),
        args.orb_id.to_string(),
        args.relay_namespace.to_string(),
    );
    let now = Instant::now();
    app_client.connect().await?;
    info!("Time took to orb_connect: {}ms", now.elapsed().as_millis());

    info!("Sending StartCapture");
    app_client
        .send(self_serve::app::v1::StartCapture {})
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    loop {
        #[expect(clippy::never_loop)]
        match app_client
            .wait_for_payload(Duration::from_millis(1000))
            .await
        {
            Ok(_) => {
                info!("Received RelayPayload");
            }
            Err(e) => {
                error!("Failed to receive RelayPayload: {:?}", e);
            }
        }
    }
}
