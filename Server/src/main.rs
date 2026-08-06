use server_account_core::{AccountEmailOutbox, AccountEmailSender, AccountService};
use server_account_mail::SendmailEmailSender;
use server_account_postgres::{AccountPolicy, PostgresAccountService};
use server_asset_store::FsAssetStore;
use server_core::{AssetStore, GarbageCollector, MaintenanceCoordinator};
use server_postgres::PostgresSyncRepository;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
        Some(command) => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unknown sync-server command: {}; expected no command or 'gc'", command.to_string_lossy())).into()),
    }
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let storage = sync_server::storage_config::StoragePaths::from_env()?;
    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let database_pool = server_database::DatabasePoolConfig::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let pool = server_database::connect_configured(&database_url, database_pool).await?;
    server_database::migrate(&pool).await?;

    let public_registration = enabled("BOKHEIM_PUBLIC_REGISTRATION")?;
    let policy = if public_registration { AccountPolicy::with_blocked_email_domains(blocked_email_domains()?).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))? } else { AccountPolicy::default() };
    let auth = Arc::new(PostgresAccountService::with_policy(pool.clone(), policy));
    let admin_password = secret_from_environment_or_systemd_credential("BOKHEIM_ADMIN_PASSWORD", "admin_password")?;
    auth.bootstrap_admin(admin_password.as_deref()).await?;

    if public_registration {
        let from = std::env::var("BOKHEIM_MAIL_FROM").map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "BOKHEIM_MAIL_FROM is required when public registration is enabled"))?;
        let sendmail_path = PathBuf::from(std::env::var_os("BOKHEIM_SENDMAIL_PATH").unwrap_or_else(|| "/usr/sbin/sendmail".into()));
        if !std::fs::metadata(&sendmail_path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("local sendmail executable not found at {}", sendmail_path.display())).into());
        }
        let sender: Arc<dyn AccountEmailSender> = Arc::new(SendmailEmailSender::new(sendmail_path, from).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?);
        let outbox: Arc<dyn AccountEmailOutbox> = auth.clone();
        tokio::spawn(server_account_mail::run_dispatcher(outbox, sender, uuid::Uuid::new_v4().to_string()));
    }

    let server = ServerConfig::from_env()?;
    let transfer_limits = server_http::TransferLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let request_limits = server_http::RequestLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let notification_config = server_http::NotificationConfig::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let validation_limits = server_asset_store::ValidationLimits::from_env().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let (books, thumbnails) = storage.into_parts();
    let app = sync_server::app_with_all_operational_limits(pool, auth, public_registration, books, thumbnails, server.client_address_source(), transfer_limits, request_limits, validation_limits, notification_config).await?;
    sync_server::transport::serve(server, app).await?;
    Ok(())
}

async fn run_gc() -> Result<(), Box<dyn std::error::Error>> {
    let storage = sync_server::storage_config::StoragePaths::from_env()?;
    let database_url = secret_from_environment_or_systemd_credential("DATABASE_URL", "database_url")?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DATABASE_URL or the database_url systemd credential is required"))?;
    let database = server_database::connect(&database_url).await?;
    server_database::check_ready(&database).await?;
    let (books, thumbnails) = storage.into_parts();
    let assets: Arc<dyn AssetStore> = Arc::new(FsAssetStore::new(books, thumbnails));
    assets.initialize().await?;
    let coordinator: Arc<dyn MaintenanceCoordinator> = Arc::new(PostgresSyncRepository::new(database));
    let collector = GarbageCollector::new(coordinator, assets);
    let grace_hours = bounded_gc_grace_hours(std::env::var("BOKHEIM_GC_GRACE_HOURS").ok().as_deref())?;
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(grace_hours * 3600)).unwrap_or(UNIX_EPOCH);
    let report = collector.collect(cutoff).await?;
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
