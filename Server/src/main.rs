use server_account::{AccountPolicy, PostgresAccountService};
use server_postgres::{GarbageCollector, PostgresSyncRepository};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

fn enabled(name: &str) -> Result<bool, io::Error> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("cannot read {name}: {error}"))),
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{name} must be true or false"))),
        },
    }
}

fn blocked_email_domains() -> Result<Vec<String>, io::Error> {
    let path = std::env::var_os("BOKHEIM_DISPOSABLE_EMAIL_DOMAINS_FILE").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "BOKHEIM_DISPOSABLE_EMAIL_DOMAINS_FILE is required when public registration is enabled"))?;
    let path = PathBuf::from(path);
    let metadata = std::fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "disposable email domain list must be a regular file no larger than 4 MiB"));
    }
    let domains: Vec<_> = std::fs::read_to_string(path)?.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).map(ToOwned::to_owned).collect();
    if domains.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "disposable email domain list must contain at least one domain"));
    }
    Ok(domains)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let mut arguments = std::env::args_os().skip(1);
    match arguments.next() {
        None => run_server().await,
        Some(command) if command == "gc" && arguments.next().is_none() => run_gc().await,
        Some(command) if command == "migrate" && arguments.next().is_none() => run_migrate().await,
        Some(command) => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unknown sync-server command: {}; expected no command, 'gc', or 'migrate'", command.to_string_lossy())).into()),
    }
}

/// Applies the schema to an empty database. There is no migration history:
/// deploys drop and recreate the database, then run this once before
/// starting the server, which never applies schema itself -- restarting an
/// already-schema'd server must not attempt to recreate its own tables.
async fn run_migrate() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let database = server_database::connect(&database_url).await?;
    server_database::migrate(&database).await?;
    Ok(())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let storage = sync_server::storage_config::StoragePaths::from_env()?;
    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let database_pool = server_database::DatabasePoolConfig::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let pool = server_database::connect_configured(&database_url, database_pool).await?;

    let public_registration = enabled("BOKHEIM_PUBLIC_REGISTRATION")?;
    let policy = if public_registration { AccountPolicy::with_blocked_email_domains(blocked_email_domains()?).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))? } else { AccountPolicy::default() };
    let auth = Arc::new(PostgresAccountService::with_policy(pool.clone(), policy));
    let admin_password = secret_from_environment_or_systemd_credential("BOKHEIM_ADMIN_PASSWORD", "admin_password")?;
    auth.bootstrap_admin(admin_password.as_deref()).await?;

    let server = ServerConfig::from_env()?;
    let transfer_limits = server_http::TransferLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let request_limits = server_http::RequestLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let notification_config = server_http::NotificationConfig::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let upload_limits = server_asset_store::UploadLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let (books, thumbnails) = storage.into_parts();
    let app =
        sync_server::app(pool, auth, sync_server::AppConfig { public_registration, books, thumbnails, client_address_source: server.client_address_source(), transfer_limits, request_limits, upload_limits, notification_config }).await?;
    sync_server::transport::serve(server, app).await?;
    Ok(())
}

async fn run_gc() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let database = server_database::connect(&database_url).await?;
    server_database::check_ready(&database).await?;
    let coordinator = Arc::new(PostgresSyncRepository::new(database));
    let collector = GarbageCollector::new(coordinator);
    let grace_hours = bounded_gc_grace_hours(std::env::var("BOKHEIM_GC_GRACE_HOURS").ok().as_deref())?;
    let report = collector.collect().await?;
    tracing::info!(
        grace_hours,
        blobs_scanned = report.blobs_scanned,
        blobs_deleted = report.blobs_deleted,
        thumbnails_scanned = report.thumbnails_scanned,
        thumbnails_deleted = report.thumbnails_deleted,
        bytes_reclaimed = report.bytes_reclaimed,
        "autonomous garbage collection completed"
    );
    Ok(())
}

fn bounded_gc_grace_hours(raw: Option<&str>) -> Result<u64, io::Error> {
    let value = raw.unwrap_or("24").parse::<u64>().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "BOKHEIM_GC_GRACE_HOURS must be an integer"))?;
    (1..=24 * 30).contains(&value).then_some(value).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "BOKHEIM_GC_GRACE_HOURS must be between 1 and 720"))
}

#[cfg(test)]
mod tests {
    use super::bounded_gc_grace_hours;

    #[test]
    fn garbage_collection_grace_period_is_bounded() {
        assert_eq!(bounded_gc_grace_hours(None).unwrap(), 24);
        assert_eq!(bounded_gc_grace_hours(Some("72")).unwrap(), 72);
        assert!(bounded_gc_grace_hours(Some("0")).is_err());
        assert!(bounded_gc_grace_hours(Some("721")).is_err());
        assert!(bounded_gc_grace_hours(Some("not-a-number")).is_err());
    }
}
