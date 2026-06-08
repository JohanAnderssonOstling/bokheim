pub mod auth;
pub mod db;
pub mod routes;
pub mod state;

use axum::Router;
use sqlx::PgPool;

pub fn app(pool: PgPool) -> Router {
    routes::router(pool)
}

pub async fn reset_database(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("drop schema public cascade").execute(pool).await?;
    sqlx::query("create schema public").execute(pool).await?;
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
