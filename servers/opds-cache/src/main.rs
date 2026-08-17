use catalog_server::{app, operations_app, CatalogService};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8090";
const DEFAULT_OPERATIONS_BIND_ADDRESS: &str = "127.0.0.1:8092";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The catalogue is latency-bound and deliberately offloads bounded SQLite
    // calls. Host-sized async and blocking pools only create allocator arenas
    // and stack working sets, so cap both independently of host CPU count.
    tokio::runtime::Builder::new_multi_thread().worker_threads(4).max_blocking_threads(8).enable_all().build()?.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let mut arguments = std::env::args_os().skip(1);
    if let Some(argument) = arguments.next() {
        if argument == "--check-catalog" {
            let path = arguments.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--check-catalog requires a path"))?;
            if arguments.next().is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "--check-catalog accepts exactly one path").into());
            }
            let registry = CatalogService::validate_file(Path::new(&path)).await?;
            println!("valid catalog registry revision {} with {} catalogs", registry.revision, registry.catalogs.len());
            return Ok(());
        }
        if argument == "--catalog-extraction-worker" {
            let input = arguments.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "extraction worker requires an input path"))?;
            let format = arguments.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "extraction worker requires a format"))?;
            let mode = arguments.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "extraction worker requires a mode"))?;
            let output = arguments.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "extraction worker requires an output path"))?;
            if arguments.next().is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "extraction worker accepts exactly four arguments").into());
            }
            catalog_server::run_catalog_extraction_worker(Path::new(&input), &format.to_string_lossy(), &mode.to_string_lossy(), Path::new(&output)).map_err(io::Error::other)?;
            return Ok(());
        }
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "usage: catalog-server [--check-catalog PATH | --catalog-extraction-worker INPUT FORMAT MODE OUTPUT]").into());
    }

    let bind_address = std::env::var("BOKHEIM_CATALOG_BIND").unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned());
    let bind_address: SocketAddr = bind_address.parse().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid BOKHEIM_CATALOG_BIND: {error}")))?;
    if !bind_address.ip().is_loopback() && std::env::var("BOKHEIM_CATALOG_ALLOW_PUBLIC_HTTP").as_deref() != Ok("true") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "non-loopback plain HTTP requires BOKHEIM_CATALOG_ALLOW_PUBLIC_HTTP=true; production should terminate HTTPS in a reverse proxy").into());
    }
    let operations_bind_address = std::env::var("BOKHEIM_CATALOG_OPERATIONS_BIND").unwrap_or_else(|_| DEFAULT_OPERATIONS_BIND_ADDRESS.to_owned());
    let operations_bind_address: SocketAddr = operations_bind_address.parse().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid BOKHEIM_CATALOG_OPERATIONS_BIND: {error}")))?;
    if !operations_bind_address.ip().is_loopback() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "BOKHEIM_CATALOG_OPERATIONS_BIND must be loopback").into());
    }
    let registry_path = std::env::var_os("BOKHEIM_CATALOG_FILE").map(PathBuf::from);
    let cache_path = std::env::var_os("BOKHEIM_CATALOG_CACHE_FILE").map(PathBuf::from);
    let cover_cache_directory = std::env::var_os("BOKHEIM_CATALOG_COVER_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| cache_path.as_deref().and_then(Path::parent).map(|parent| parent.join("covers")))
        .unwrap_or_else(|| std::env::temp_dir().join("catalog-server-covers"));
    let service = match registry_path {
        Some(path) => CatalogService::from_file_with_storage(path, cache_path, cover_cache_directory).await?,
        None => CatalogService::bundled_with_storage(cache_path, cover_cache_directory)?,
    };
    let refresh_service = service.clone();
    tokio::spawn(async move { refresh_service.run_refresh_loop().await });
    let registry_service = service.clone();
    tokio::spawn(async move { registry_service.run_registry_refresh_loop().await });
    let extraction_service = service.clone();
    tokio::spawn(async move { extraction_service.run_epub_extraction_loop().await });
    let listener = tokio::net::TcpListener::bind(bind_address).await?;
    let operations_listener = tokio::net::TcpListener::bind(operations_bind_address).await?;
    tracing::info!(address = %listener.local_addr()?, "public unauthenticated catalog server listening");
    tracing::info!(address = %operations_listener.local_addr()?, "loopback catalogue operations server listening");
    let public_server = axum::serve(listener, app(service.clone()));
    let operations_server = axum::serve(operations_listener, operations_app(service));
    tokio::select! {
        result = public_server => result?,
        result = operations_server => result?,
        _ = shutdown_signal() => {},
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl-C shutdown handler");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM shutdown handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
