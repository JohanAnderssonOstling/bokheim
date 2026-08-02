use server_core::AuthService;
use server_postgres::PostgresAuthService;
use std::io;
use std::path::Path;
use sync_server::transport::ServerConfig;
use tracing_subscriber::{fmt, EnvFilter};

fn secret_from_environment_or_systemd_credential(environment_name: &str, credential_name: &str) -> Result<Option<String>, io::Error> {
    let environment = std::env::var(environment_name).ok().filter(|value| !value.is_empty());
    let credential = if let Some(directory) = std::env::var_os("CREDENTIALS_DIRECTORY") {
        match std::fs::read_to_string(Path::new(&directory).join(credential_name)) {
            Ok(value) => Some(value.trim_end_matches(['\r', '\n']).to_owned()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    match (environment, credential.filter(|value| !value.is_empty())) {
        (Some(_), Some(_)) => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("configure only one of {environment_name} or the {credential_name} systemd credential"))),
        (environment, credential) => Ok(environment.or(credential)),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let pool = server_postgres::connect(&database_url).await?;
    server_postgres::migrate(&pool).await?;

    let auth = PostgresAuthService::new(pool.clone());
    let admin_password = secret_from_environment_or_systemd_credential("BOKHEIM_ADMIN_PASSWORD", "admin_password")?;
    if let Err(error) = auth.bootstrap_admin(admin_password.as_deref()).await {
        tracing::error!("bootstrap_admin failed: {error}");
    }

    let server = ServerConfig::from_env()?;
    let transfer_limits = server_http::TransferLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let app = sync_server::app_with_client_address_source_and_transfer_limits(pool, server.client_address_source(), transfer_limits);
    sync_server::transport::serve(server, app).await?;
    Ok(())
}
