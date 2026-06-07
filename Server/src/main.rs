use std::net::SocketAddr;

use sqlx::PgPool;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/sync".to_string());

    let pool = PgPool::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    if let Err(err) = sync_server::auth::bootstrap_admin(&pool).await {
        tracing::error!("bootstrap_admin failed: {err}");
    }

    let app = sync_server::app(pool);
    let addr_str = std::env::var("SYNC_SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = addr_str.parse()?;
    tracing::info!("listening on {}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
