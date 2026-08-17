use metadata_server::{add_popularity_to_snapshot, app, import_rich_snapshot_with_popularity, import_snapshot, import_snapshot_with_popularity, MetadataService};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_BIND: &str = "127.0.0.1:8091";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|argument| argument == "import") {
        if arguments.len() < 5 || arguments.len() > 6 {
            return Err(invalid_usage().into());
        }
        let limit = arguments.get(5).map(|value| value.to_string_lossy().parse::<usize>()).transpose().map_err(|_| invalid_usage())?;
        import_snapshot(Path::new(&arguments[1]), Path::new(&arguments[2]), Path::new(&arguments[3]), Path::new(&arguments[4]), limit)?;
        return Ok(());
    }
    if arguments.first().is_some_and(|argument| argument == "import-rich") {
        if arguments.len() < 8 || arguments.len() > 9 {
            return Err(invalid_usage().into());
        }
        let limit = arguments.get(8).map(|value| value.to_string_lossy().parse::<usize>()).transpose().map_err(|_| invalid_usage())?;
        import_rich_snapshot_with_popularity(Path::new(&arguments[1]), Path::new(&arguments[2]), Path::new(&arguments[3]), Path::new(&arguments[4]), Path::new(&arguments[5]), Path::new(&arguments[6]), Path::new(&arguments[7]), limit)?;
        return Ok(());
    }
    if arguments.first().is_some_and(|argument| argument == "import-popularity") {
        if arguments.len() < 7 || arguments.len() > 8 {
            return Err(invalid_usage().into());
        }
        let limit = arguments.get(7).map(|value| value.to_string_lossy().parse::<usize>()).transpose().map_err(|_| invalid_usage())?;
        import_snapshot_with_popularity(Path::new(&arguments[1]), Path::new(&arguments[2]), Path::new(&arguments[3]), Path::new(&arguments[4]), Path::new(&arguments[5]), Path::new(&arguments[6]), limit)?;
        return Ok(());
    }
    if arguments.first().is_some_and(|argument| argument == "add-popularity") {
        if arguments.len() < 5 || arguments.len() > 6 {
            return Err(invalid_usage().into());
        }
        let limit = arguments.get(5).map(|value| value.to_string_lossy().parse::<usize>()).transpose().map_err(|_| invalid_usage())?;
        add_popularity_to_snapshot(Path::new(&arguments[1]), Path::new(&arguments[2]), Path::new(&arguments[3]), Path::new(&arguments[4]), limit)?;
        return Ok(());
    }
    if !arguments.is_empty() {
        return Err(invalid_usage().into());
    }

    let database = std::env::var_os("BOKHEIM_METADATA_DATABASE").map(PathBuf::from).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "BOKHEIM_METADATA_DATABASE is required"))?;
    let bind: SocketAddr = std::env::var("BOKHEIM_METADATA_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_owned()).parse().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid BOKHEIM_METADATA_BIND: {error}")))?;
    if !bind.ip().is_loopback() && std::env::var("BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP").as_deref() != Ok("true") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "non-loopback plain HTTP requires BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP=true; use a TLS reverse proxy in production").into());
    }
    let service = MetadataService::open(database)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(address = %listener.local_addr()?, "metadata server listening");
    axum::serve(listener, app(service)).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

fn invalid_usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: metadata-server import EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz OUTPUT.sqlite [MAX_RECORDS]\n       metadata-server import-popularity EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz RATINGS.txt.gz READING-LOG.txt.gz OUTPUT.sqlite [MAX_RECORDS]\n       metadata-server add-popularity SOURCE.sqlite RATINGS.txt.gz READING-LOG.txt.gz OUTPUT.sqlite [MAX_RECORDS]\n       metadata-server import-rich EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz WIKIDATA.json.bz2 RATINGS.txt.gz READING-LOG.txt.gz OUTPUT.sqlite [MAX_RECORDS]\n       metadata-server  # configured with BOKHEIM_METADATA_DATABASE",
    )
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
