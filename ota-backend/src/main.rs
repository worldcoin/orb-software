use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use orb_ota_backend::app::create_app;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::SocketAddr;
use tracing::{error, info};

const BUILD_INFO: orb_build_info::BuildInfo = orb_build_info::make_build_info!();

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[derive(Debug, clap::Parser)]
#[clap(
    author,
    version = BUILD_INFO.version,
    styles = clap_v3_styles(),
)]
struct Args {}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let _telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    let _args = Args::parse();

    // Figure out where the DB lives.
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@db/postgres".to_string());

    info!("connecting to database...");
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Run pending migrations before accepting traffic.
    if let Err(e) = sqlx::migrate!().run(&pool).await {
        error!(?e, "failed to run database migrations");
        return Err(e.into());
    }

    let app = create_app(pool);

    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
