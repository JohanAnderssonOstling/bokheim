//! Inspect raw metadata exposed by the configured OPDS catalogues.
//!
//! Example:
//! cargo run -p opds --example probe_metadata -- --pages 3

use futures_util::{stream, StreamExt};
use opds::{endpoints_from_registry, CatalogRegistry, OpdsClient, OpdsEndpoint, OpdsEndpointId, OpdsLinkTarget};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_PAGES: usize = 3;
const DEFAULT_CONCURRENCY: usize = 4;
const MAX_PAGES: usize = 20;
const MAX_CONCURRENCY: usize = 16;
const MAX_PROBE_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_SAMPLES_PER_FIELD: usize = 3;
const MAX_SAMPLE_CHARS: usize = 100;

struct Options {
    registry: PathBuf,
    pages: usize,
    concurrency: usize,
    field_filter: Option<String>,
    only_id: Option<String>,
    url: Option<String>,
}

#[derive(Default)]
struct FieldStats {
    occurrences: usize,
    samples: Vec<String>,
}

#[derive(Default)]
struct MetadataStats {
    fields: BTreeMap<String, FieldStats>,
}

struct CatalogReport {
    id: String,
    pages: usize,
    formats: HashSet<&'static str>,
    stats: MetadataStats,
    errors: Vec<String>,
}

#[tokio::main]
async fn main() {
    let options = options();
    let endpoints = endpoints(&options);
    let http = Arc::new(
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(format!("Bokheim/{} OPDS-metadata-probe", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client should initialize"),
    );
    let reports = stream::iter(endpoints)
        .map(|endpoint| {
            let http = http.clone();
            async move { inspect_catalog(http, endpoint, options.pages).await }
        })
        .buffer_unordered(options.concurrency)
        .collect::<Vec<_>>()
        .await;

    println!("catalog\tpages\tformats\tfield\toccurrences\tsamples");
    let mut reports = reports;
    reports.sort_by(|left, right| left.id.cmp(&right.id));
    for report in reports {
        let mut formats = report.formats.into_iter().collect::<Vec<_>>();
        formats.sort_unstable();
        for (field, stats) in report.stats.fields {
            if options.field_filter.as_deref().is_some_and(|filter| !field.to_lowercase().contains(filter)) {
                continue;
            }
            println!("{}\t{}\t{}\t{}\t{}\t{}", report.id, report.pages, formats.join(","), field, stats.occurrences, stats.samples.join(" | "));
        }
        for error in report.errors {
            eprintln!("{}\t{}", report.id, error);
        }
    }
}

async fn inspect_catalog(http: Arc<reqwest::Client>, endpoint: OpdsEndpoint, page_limit: usize) -> CatalogReport {
    let mut urls = vec![endpoint.url.clone()];
    let mut errors = Vec::new();
    let discovery = OpdsClient::new().expect("OPDS client should initialize");
    match discovery.fetch_catalog(&endpoint.url).await {
        Ok(root) => {
            for link in &root.navigation {
                if link.target == OpdsLinkTarget::Catalog {
                    push_unique(&mut urls, link.url.clone());
                }
            }
            for group in &root.groups {
                if let Some(url) = &group.self_url {
                    push_unique(&mut urls, url.clone());
                }
                for link in &group.navigation {
                    if link.target == OpdsLinkTarget::Catalog {
                        push_unique(&mut urls, link.url.clone());
                    }
                }
            }
        }
        Err(error) => errors.push(format!("catalog discovery failed: {}", clean(&error.to_string()))),
    }
    urls.truncate(page_limit);

    let mut stats = MetadataStats::default();
    let mut formats = HashSet::new();
    let mut pages = 0;
    for url in urls {
        match fetch_raw(&http, &url).await {
            Ok((media_type, body)) => {
                pages += 1;
                if looks_like_json(media_type.as_deref(), &body) {
                    formats.insert("opds2-json");
                    match serde_json::from_slice::<Value>(&body) {
                        Ok(value) => inspect_json(&value, &mut stats),
                        Err(error) => errors.push(format!("invalid JSON from {}: {}", clean(&url), clean(&error.to_string()))),
                    }
                } else {
                    formats.insert("opds1-xml");
                    if let Err(error) = inspect_xml(&body, &mut stats) {
                        errors.push(format!("invalid XML from {}: {}", clean(&url), clean(&error)));
                    }
                }
            }
            Err(error) => errors.push(format!("raw fetch failed for {}: {}", clean(&url), clean(&error))),
        }
    }
    CatalogReport { id: endpoint.id.as_str().to_owned(), pages, formats, stats, errors }
}

async fn fetch_raw(http: &reqwest::Client, url: &str) -> Result<(Option<String>, Vec<u8>), String> {
    let response = http.get(url).header(reqwest::header::ACCEPT, "application/opds+json, application/atom+xml;profile=opds-catalog, application/atom+xml, application/xml;q=0.8").send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|length| length > MAX_PROBE_BODY_BYTES as u64) {
        return Err(format!("body exceeds {} bytes", MAX_PROBE_BODY_BYTES));
    }
    let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).map(str::to_owned);
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_PROBE_BODY_BYTES {
            return Err(format!("body exceeds {} bytes", MAX_PROBE_BODY_BYTES));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((media_type, body))
}

fn inspect_json(root: &Value, stats: &mut MetadataStats) {
    if let Some(metadata) = root.get("metadata") {
        walk_json("feed.metadata", metadata, stats);
    }
    if let Some(links) = root.get("links") {
        walk_json("feed.links", links, stats);
    }
    inspect_json_publications(root.get("publications"), stats);
    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            if let Some(metadata) = group.get("metadata") {
                walk_json("group.metadata", metadata, stats);
            }
            inspect_json_publications(group.get("publications"), stats);
        }
    }
}

fn inspect_json_publications(publications: Option<&Value>, stats: &mut MetadataStats) {
    let Some(publications) = publications.and_then(Value::as_array) else { return };
    for publication in publications {
        if let Some(metadata) = publication.get("metadata") {
            walk_json("publication.metadata", metadata, stats);
        }
        if let Some(links) = publication.get("links") {
            walk_json("publication.links", links, stats);
        }
        if let Some(images) = publication.get("images") {
            walk_json("publication.images", images, stats);
        }
    }
}

fn walk_json(path: &str, value: &Value, stats: &mut MetadataStats) {
    match value {
        Value::Object(object) => {
            for (name, child) in object {
                let child_path = format!("{path}.{name}");
                stats.observe(&child_path, json_sample(child));
                walk_json(&child_path, child, stats);
            }
        }
        Value::Array(values) => {
            for child in values {
                walk_json(&format!("{path}[]"), child, stats);
            }
        }
        _ => {}
    }
}

fn json_sample(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some("null".to_owned()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn inspect_xml(body: &[u8], stats: &mut MetadataStats) -> Result<(), String> {
    let mut reader = Reader::from_reader(body);
    reader.config_mut().trim_text(true);
    let mut stack = Vec::<String>::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                stack.push(name.clone());
                observe_xml_element(&reader, &element, &stack, stats);
            }
            Ok(Event::Empty(element)) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                stack.push(name);
                observe_xml_element(&reader, &element, &stack, stats);
                stack.pop();
            }
            Ok(Event::Text(text)) => {
                if let Some(name) = stack.last() {
                    let scope = xml_scope(&stack);
                    stats.sample(&format!("{scope}.element.{name}"), &String::from_utf8_lossy(text.as_ref()));
                }
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn observe_xml_element(reader: &Reader<&[u8]>, element: &quick_xml::events::BytesStart<'_>, stack: &[String], stats: &mut MetadataStats) {
    let scope = xml_scope(stack);
    let name = stack.last().map(String::as_str).unwrap_or("unknown");
    stats.observe(&format!("{scope}.element.{name}"), None);
    for attribute in element.attributes().flatten() {
        let attribute_name = String::from_utf8_lossy(attribute.key.as_ref());
        let value = attribute.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder()).ok().map(|value| value.into_owned());
        stats.observe(&format!("{scope}.attribute.{name}@{attribute_name}"), value);
    }
}

fn xml_scope(stack: &[String]) -> &'static str {
    if stack.iter().any(|name| name.rsplit(':').next() == Some("entry")) {
        "entry"
    } else {
        "feed"
    }
}

impl MetadataStats {
    fn observe(&mut self, path: &str, sample: Option<String>) {
        let field = self.fields.entry(path.to_owned()).or_default();
        field.occurrences += 1;
        if let Some(sample) = sample {
            add_sample(&mut field.samples, &sample);
        }
    }

    fn sample(&mut self, path: &str, sample: &str) {
        let field = self.fields.entry(path.to_owned()).or_default();
        add_sample(&mut field.samples, sample);
    }
}

fn add_sample(samples: &mut Vec<String>, value: &str) {
    let value = clean(value);
    let value = value.chars().take(MAX_SAMPLE_CHARS).collect::<String>();
    if !value.is_empty() && samples.len() < MAX_SAMPLES_PER_FIELD && !samples.contains(&value) {
        samples.push(value);
    }
}

fn looks_like_json(media_type: Option<&str>, body: &[u8]) -> bool {
    media_type.is_some_and(|media_type| media_type == "application/opds+json" || media_type == "application/json") || body.iter().copied().find(|byte| !byte.is_ascii_whitespace()) == Some(b'{')
}

fn push_unique(urls: &mut Vec<String>, url: String) {
    if !urls.contains(&url) {
        urls.push(url);
    }
}

fn endpoints(options: &Options) -> Vec<OpdsEndpoint> {
    if let Some(url) = &options.url {
        return vec![OpdsEndpoint {
            id: OpdsEndpointId::parse(options.only_id.as_deref().unwrap_or("probe")).expect("--id must be a valid catalogue id"),
            title: "Custom URL".to_owned(),
            description: "Ad hoc metadata probe target".to_owned(),
            languages: vec!["Unspecified".to_owned()],
            url: url.clone(),
        }];
    }
    let bytes = std::fs::read(&options.registry).unwrap_or_else(|error| panic!("cannot read {}: {error}", options.registry.display()));
    let registry: CatalogRegistry = serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("invalid registry {}: {error}", options.registry.display()));
    endpoints_from_registry(registry).unwrap_or_else(|error| panic!("invalid catalog endpoints: {error}")).into_iter().filter(|endpoint| options.only_id.as_deref().is_none_or(|id| endpoint.id.as_str() == id)).collect()
}

fn options() -> Options {
    let mut registry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../servers/opds-cache/opds.json");
    let mut pages = DEFAULT_PAGES;
    let mut concurrency = DEFAULT_CONCURRENCY;
    let mut field_filter = None;
    let mut only_id = None;
    let mut url = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--registry" => registry = PathBuf::from(arguments.next().expect("--registry requires a path")),
            "--pages" => pages = parse_bounded("--pages", arguments.next(), MAX_PAGES),
            "--concurrency" => concurrency = parse_bounded("--concurrency", arguments.next(), MAX_CONCURRENCY),
            "--field" => field_filter = Some(arguments.next().expect("--field requires text").to_lowercase()),
            "--id" => only_id = Some(arguments.next().expect("--id requires a catalogue id")),
            "--url" => url = Some(arguments.next().expect("--url requires an HTTP URL")),
            _ => panic!("unknown argument {argument}"),
        }
    }
    Options { registry, pages, concurrency, field_filter, only_id, url }
}

fn parse_bounded(name: &str, value: Option<String>, maximum: usize) -> usize {
    let value = value.unwrap_or_else(|| panic!("{name} requires a positive integer"));
    value.parse::<usize>().ok().filter(|value| (1..=maximum).contains(value)).unwrap_or_else(|| panic!("{name} must be between 1 and {maximum}"))
}

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").replace('\t', " ")
}
