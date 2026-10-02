use metadata_server::{app, build_answers_with_ratings, crawl, import_authorities, import_description_snapshot, import_snapshot, MetadataService};
use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing_subscriber::{fmt, EnvFilter};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let command = args.first().and_then(|a| a.to_str()).ok_or_else(usage)?;
    match command {
        "build" if args.len() == 4 => {
            let provider = (args[2] != "-").then(|| Path::new(&args[2]));
            let demand = std::env::var_os("BOKHEIM_METADATA_DEMAND").map(PathBuf::from);
            let descriptions = std::env::var_os("BOKHEIM_METADATA_DESCRIPTION_DATABASE").map(PathBuf::from);
            let ratings = std::env::var_os("BOKHEIM_LIBRARYTHING_RATINGS_DUMP").map(PathBuf::from);
            let report = build_answers_with_ratings(Path::new(&args[1]), provider, demand.as_deref(), descriptions.as_deref(), ratings.as_deref(), Path::new(&args[3]))?;
            println!("{}", serde_json::to_string(&report)?);
        }
        "crawl" if (args.len() == 3 || (args.len() == 4 && args[3] == "--once")) => {
            let key = std::env::var_os("BOKHEIM_LIBRARYTHING_KEY_FILE").map(PathBuf::from);
            crawl(Path::new(&args[1]), Path::new(&args[2]), key.as_deref(), args.len() == 4).await?;
        }
        "import" if args.len() == 5 && args[1] == "authorities" => {
            import_authorities(Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4]))?;
        }
        "import" if !args.get(1).is_some_and(|a| a == "descriptions") && (5..=6).contains(&args.len()) => {
            let limit = args.get(5).map(|a| a.to_string_lossy().parse::<usize>()).transpose()?;
            import_snapshot(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4]), limit)?;
        }
        "import" if args.get(1).is_some_and(|a| a == "descriptions") && (5..=6).contains(&args.len()) => {
            let limit = args.get(5).map(|a| a.to_string_lossy().parse::<usize>()).transpose()?;
            import_description_snapshot(Path::new(&args[2]), Path::new(&args[3]), &args[4].to_string_lossy(), limit)?;
        }
        "serve" if args.len() == 1 => {
            let answers = required_path("BOKHEIM_METADATA_ANSWERS")?;
            let demand = required_path("BOKHEIM_METADATA_DEMAND")?;
            let bind: SocketAddr = std::env::var("BOKHEIM_METADATA_BIND").unwrap_or_else(|_| "127.0.0.1:8091".into()).parse()?;
            if !bind.ip().is_loopback() && std::env::var("BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP").as_deref() != Ok("true") {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "non-loopback plain HTTP requires BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP=true; use a TLS reverse proxy in production").into());
            }
            let service = MetadataService::open(&answers, &demand)?;
            let listener = tokio::net::TcpListener::bind(bind).await?;
            tracing::info!(address=%listener.local_addr()?,"metadata server listening");
            axum::serve(listener, app(service)).with_graceful_shutdown(shutdown_signal()).await?;
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}
fn required_path(name: &str) -> Result<PathBuf, io::Error> {
    std::env::var_os(name).map(PathBuf::from).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}
fn usage() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput,"usage: metadata-server build SNAPSHOT.sqlite PROVIDER.sqlite|- OUT.sqlite\n       metadata-server serve\n       metadata-server crawl ANSWERS.sqlite PROVIDER.sqlite [--once]\n       metadata-server import EDITIONS.gz WORKS.gz AUTHORS.gz OUT.sqlite [MAX_RECORDS]\n       metadata-server import descriptions WORKS.gz OUT.sqlite DUMP_DATE [MAX_RECORDS]")
}
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {_=ctrl_c=>{},_=terminate=>{}}
}
