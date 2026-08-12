use futures_util::{stream, StreamExt};
use opds::{endpoints_from_registry, CatalogRegistry, OpdsClient, OpdsEndpoint, OpdsEndpointId};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Options {
    registry: PathBuf,
    runs: usize,
    concurrency: usize,
    only_id: Option<String>,
    url: Option<String>,
}

#[tokio::main]
async fn main() {
    let options = options();
    let bytes = std::fs::read(&options.registry).unwrap_or_else(|error| panic!("cannot read {}: {error}", options.registry.display()));
    let registry: CatalogRegistry = serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("invalid registry {}: {error}", options.registry.display()));
    let endpoints = match options.url.as_ref() {
        Some(url) => vec![OpdsEndpoint {
            id: OpdsEndpointId::parse(options.only_id.as_deref().unwrap_or("custom")).expect("--id must be a valid catalog id"),
            title: "Custom URL".to_owned(),
            description: "Ad hoc profiling target".to_owned(),
            languages: vec!["Unspecified".to_owned()],
            url: url.clone(),
        }],
        None => {
            endpoints_from_registry(registry).unwrap_or_else(|error| panic!("invalid catalog endpoints: {error}")).into_iter().filter(|endpoint| options.only_id.as_deref().is_none_or(|id| endpoint.id.as_str() == id)).collect::<Vec<_>>()
        }
    };
    let client = Arc::new(OpdsClient::new().expect("HTTP client should initialize"));
    println!("id\trun\tstatus\ttotal_ms\theaders_ms\tdownload_ms\tdiscovery_ms\tparse_ms\tbytes\thttp_fetches\titems\tversion\terror");
    stream::iter(endpoints)
        .for_each_concurrent(options.concurrency, |endpoint| {
            let client = client.clone();
            async move {
                for run in 1..=options.runs {
                    profile_endpoint(&client, &endpoint, run).await;
                }
            }
        })
        .await;
}

async fn profile_endpoint(client: &OpdsClient, endpoint: &OpdsEndpoint, run: usize) {
    let (result, profile) = client.profile_catalog_fetch(&endpoint.url).await;
    match result {
        Ok(catalog) => {
            let items = catalog.navigation.len() + catalog.publications.len() + catalog.groups.len();
            println!(
                "{}\t{}\tok\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{}\t{}\t{}\t{:?}\t",
                endpoint.id.as_str(),
                run,
                milliseconds(profile.total),
                milliseconds(profile.response_headers),
                milliseconds(profile.body_download),
                milliseconds(profile.discovery),
                milliseconds(profile.parsing),
                profile.response_bytes,
                profile.http_fetches,
                items,
                catalog.version
            );
        }
        Err(error) => println!(
            "{}\t{}\terror\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{}\t{}\t0\t-\t{}",
            endpoint.id.as_str(),
            run,
            milliseconds(profile.total),
            milliseconds(profile.response_headers),
            milliseconds(profile.body_download),
            milliseconds(profile.discovery),
            milliseconds(profile.parsing),
            profile.response_bytes,
            profile.http_fetches,
            clean_error(&error.to_string())
        ),
    }
}

fn options() -> Options {
    let mut registry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../servers/opds-cache/opds.json");
    let mut runs = 1;
    let mut concurrency = 4;
    let mut only_id = None;
    let mut url = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--registry" => registry = PathBuf::from(arguments.next().expect("--registry requires a path")),
            "--runs" => runs = parse_positive("--runs", arguments.next()),
            "--concurrency" => concurrency = parse_positive("--concurrency", arguments.next()),
            "--id" => only_id = Some(arguments.next().expect("--id requires a catalog id")),
            "--url" => url = Some(arguments.next().expect("--url requires an HTTP URL")),
            _ => panic!("unknown argument {argument}"),
        }
    }
    Options { registry, runs, concurrency, only_id, url }
}

fn parse_positive(name: &str, value: Option<String>) -> usize {
    let value = value.unwrap_or_else(|| panic!("{name} requires a positive integer"));
    value.parse::<usize>().ok().filter(|value| *value > 0).unwrap_or_else(|| panic!("{name} requires a positive integer"))
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn clean_error(error: &str) -> String {
    error.replace(['\t', '\n', '\r'], " ")
}
