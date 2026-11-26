use metadata_server::{
    app, build_answers, build_answers_incremental, extend_edition_index, import_authorities, import_description_snapshot, import_edition_descriptions, import_snapshot, import_subject_editions, index_work_classifications,
    index_work_editions, index_work_search, reclassify_answers, MetadataService, WorkLookup,
};
use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use subject_projection::installed;
use tracing_subscriber::{fmt, EnvFilter};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt().with_writer(io::stderr).with_env_filter(EnvFilter::from_default_env()).init();
    if let Some(path) = std::env::var_os("BOKHEIM_RUNTIME_TAXONOMY").map(PathBuf::from) {
        installed::validate(&path, installed::metadata(&path)?)?;
        installed::activate(&path)?;
    }
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let command = args.first().and_then(|a| a.to_str()).ok_or_else(usage)?;
    let works = std::env::var_os("BOKHEIM_METADATA_WORKS").map(PathBuf::from);
    match command {
        "import-edition-descriptions" if (3..=4).contains(&args.len()) => {
            let limit = args.get(3).map(|value| value.to_string_lossy().parse::<usize>()).transpose()?;
            println!("{}", import_edition_descriptions(Path::new(&args[1]), Path::new(&args[2]), limit)?);
        }
        "index-work-search" if args.len() == 2 => {
            println!("{}", index_work_search(Path::new(&args[1]))?);
        }
        "index-work-editions" if args.len() == 2 => {
            println!("{}", index_work_editions(Path::new(&args[1]))?);
        }
        "lookup-works" if args.len() == 3 => {
            let request: metadata_contract::works::WorkLookupRequest = serde_json::from_slice(&std::fs::read(&args[2])?)?;
            let index = WorkLookup::open(Path::new(&args[1]))?;
            println!("{}", serde_json::to_string(&metadata_contract::works::WorkLookupResponse { results: index.resolve(request.queries)? })?);
        }
        "index-work-classifications" if args.len() == 3 => {
            println!("{}", index_work_classifications(Path::new(&args[1]), Path::new(&args[2]))?);
        }
        "import-subject-editions" if (5..=6).contains(&args.len()) => {
            let limit = args.get(5).map(|value| value.to_string_lossy().parse::<usize>()).transpose()?;
            import_subject_editions(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4]), limit)?;
        }
        "extend-edition-index" if args.len() == 4 || (args.len() == 6 && args[4] == "--limit") => {
            let limit = args.get(5).map(|value| value.to_string_lossy().parse::<usize>()).transpose()?;
            println!("{}", serde_json::to_string(&extend_edition_index(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), limit)?)?);
        }
        "evaluate-subjects" if args.len() == 3 => {
            let request: metadata_contract::books::BookEnrichmentRequest = serde_json::from_slice(&std::fs::read(&args[2])?)?;
            let service = MetadataService::open_with_work_lookup(Path::new(&args[1]), &required_path("BOKHEIM_METADATA_DEMAND")?, works.as_deref())?;
            println!("{}", serde_json::to_string(&service.evaluate_subjects(request.books).await?)?);
        }
        "build" | "reclassify" if args.len() == 4 || (command == "build" && args.len() == 6 && args[4] == "--previous") => {
            let provider = (args[2] != "-").then(|| Path::new(&args[2]));
            let demand = std::env::var_os("BOKHEIM_METADATA_DEMAND").map(PathBuf::from);
            let descriptions = std::env::var_os("BOKHEIM_METADATA_DESCRIPTION_DATABASE").map(PathBuf::from);
            let report = if command == "reclassify" {
                reclassify_answers(Path::new(&args[1]), provider, demand.as_deref(), descriptions.as_deref(), Path::new(&args[3]))?
            } else if args.len() == 6 {
                build_answers_incremental(Path::new(&args[1]), provider, demand.as_deref(), descriptions.as_deref(), Path::new(&args[5]), Path::new(&args[3]))?
            } else {
                build_answers(Path::new(&args[1]), provider, demand.as_deref(), descriptions.as_deref(), Path::new(&args[3]))?
            };
            println!("{}", serde_json::to_string(&report)?);
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
            let service = MetadataService::open_with_work_lookup(&answers, &demand, works.as_deref())?;
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
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: metadata-server import-edition-descriptions EDITIONS.gz SNAPSHOT.sqlite [MAX_RECORDS]\n       metadata-server build SNAPSHOT.sqlite PROVIDER.sqlite|- OUT.sqlite [--previous ANSWERS.sqlite]\n       metadata-server lookup-works SNAPSHOT.sqlite REQUEST.json\n       metadata-server index-work-editions SNAPSHOT.sqlite\n       metadata-server index-work-search SNAPSHOT.sqlite\n       metadata-server import-subject-editions EDITIONS.gz WORKS.gz AUTHORS.gz OUT.sqlite [MAX_RECORDS]\n       metadata-server extend-edition-index SNAPSHOT.sqlite PREVIOUS.sqlite OUT.sqlite [--limit EDITIONS]\n       metadata-server reclassify SNAPSHOT.sqlite PROVIDER.sqlite|- ANSWERS.sqlite\n       metadata-server evaluate-subjects ANSWERS.sqlite REQUEST.json\n       metadata-server serve\n       metadata-server import EDITIONS.gz WORKS.gz AUTHORS.gz OUT.sqlite [MAX_RECORDS]\n       metadata-server import descriptions WORKS.gz OUT.sqlite DUMP_DATE [MAX_RECORDS]",
    )
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
