use orb_ota_backend::app::create_app;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::SocketAddr;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // orb_telemetry::

    // Figure out where the DB lives.
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost/postgres".to_string()
    });

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
