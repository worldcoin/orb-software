mod http_handler;
use color_eyre::eyre::{Context, Result};
use http_handler::{download, info, upload};
use iroh::Watcher;
use iroh_base::ticket::NodeTicket;
use orb_blob_p2p::PeerTracker;
use reqwest::Client;
use tokio::io::{AsyncBufReadExt, BufReader};

use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use orb_blob::cfg::Cfg;
use orb_blob::program;

#[derive(Parser, Debug)]
#[command(name = "orb-p2p-demo")]
#[command(about = "Run a demo of the Orb Blob P2P system", long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = 7777, required = true)]
    port: u16,

    #[arg(short, long, default_value = "./store")]
    store: PathBuf,

    #[arg(short, long, default_value = "./store.sqlite")]
    db: PathBuf,

    #[arg(long)]
    local: bool,

    #[arg(long)]
    secret: Option<String>,

    #[arg(long, num_args = 0.., value_name = "PUBKEY")]
    peer: Vec<String>,

    #[arg(long, default_value_t = 60)]
    peer_timeout_secs: u64,

    #[arg(long, default_value_t = 1)]
    min_peers: usize,

    #[arg(long)]
    bootstrap: bool,
}
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // Populate env vars used by Cfg::from_env
    unsafe {
        std::env::set_var("ORB_BLOB_PORT", args.port.to_string());
        std::env::set_var(
            "ORB_BLOB_STORE_PATH",
            args.store.to_string_lossy().into_owned(),
        );
        std::env::set_var(
            "ORB_BLOB_SQLITE_PATH",
            args.db.to_string_lossy().to_string(),
        );
        std::env::set_var(
            "ORB_BLOB_PEER_LISTEN_TIMEOUT",
            args.peer_timeout_secs.to_string(),
        );
        std::env::set_var("ORB_BLOB_MIN_PEER_REQ", args.min_peers.to_string());

        if args.local {
            std::env::set_var("ORB_BLOB_IROH_LOCAL", "1");
        }

        if let Some(secret) = args.secret {
            std::env::set_var("ORB_BLOB_SECRET_KEY", secret);
        }

        if !args.peer.is_empty() {
            std::env::set_var("ORB_BLOB_WELL_KNOWN_NODES", args.peer.join(","));
        }
    }

    let cfg = Cfg::from_env()?;

    let shutdown = CancellationToken::new();

    if args.bootstrap {
        println!("\n🪵 Bootstrap node running");

        let endpoint = iroh::Endpoint::builder().clear_discovery();
        let endpoint = if args.local {
            endpoint
                .relay_mode(iroh::RelayMode::Disabled)
                .bind_addr_v4("127.0.0.1:0".parse().unwrap())
                .bind_addr_v6("[::1]:0".parse().unwrap())
                .discovery_local_network()
        } else {
            endpoint.discovery_dht().discovery_n0()
        };
        let endpoint = endpoint.bind().await.unwrap();

        let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());
        let _router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(orb_blob_p2p::ALPN, gossip.clone())
            .spawn();

        // Instantiate the tracker
        let _tracker = PeerTracker::builder()
            .gossip(&gossip)
            .bootstrap_nodes(vec![])
            .endpoint(endpoint.clone())
            .build()
            .await
            .unwrap();

        println!(
            "Endpoint NodeTicket: {}",
            NodeTicket::from(endpoint.node_addr().initialized().await)
        );

        shutdown.cancelled().await;

        Ok(())
    } else {
        let addr = format!("http://127.0.0.1:{}", cfg.port);

        let socket_addr: SocketAddr = addr
            .strip_prefix("http://")
            .expect("Literally hardcoded above")
            .parse()?;
        let listener = TcpListener::bind(&socket_addr).await?;

        let client = Client::new();

        let server_shutdown = shutdown.clone();

        let server_fut = async move {
            tokio::spawn(program::run(cfg, listener, server_shutdown))
                .await
                .wrap_err("Server panicked!!!")?
        };

        let run_menu_fut = run_menu_loop(&client, &addr);
        let ((), ()) = tokio::try_join!(server_fut, run_menu_fut)?;

        println!("\nShutting down server...");
        shutdown.cancel();

        println!("Done!");
        Ok(())
    }
}

pub async fn run_menu_loop(client: &Client, addr: &str) -> Result<()> {
    loop {
        println!("\n=== Menu ===");
        println!("1. Upload file");
        println!("2. Download file");
        println!("3. Print info");
        println!("4. Exit");
        let choice = read_input("Command: ").await?;

        match dbg!(choice.trim()) {
            "1" => {
                let path = read_input("Enter file path to upload: ").await?;
                upload(&path, client, addr).await?;
            }
            "2" => {
                let hash = read_input("Enter hash to download: ").await?;
                let dest = read_input("Enter destination file path: ").await?;
                download(&hash, &dest, client, addr).await?;
            }
            "3" => {
                info(client, addr).await?;
            }
            "4" => break,
            _ => println!("Invalid choice"),
        }
    }

    Ok(())
}

async fn read_input(prompt: &str) -> Result<String> {
    println!("{prompt} ");
    let reader = BufReader::new(tokio::io::stdin());
    let mut lines = reader.lines();
    let line = lines.next_line().await.unwrap().unwrap();
    Ok(line.trim().to_string())
}
