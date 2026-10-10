//! LibraryThing Common Knowledge client; persistent normalized results and quota.
use crate::{error, Isbn13, MetadataError};
use futures_util::StreamExt;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
const ENDPOINT: &str = "https://www.librarything.com/services/rest/1.1/";
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct LibraryThingClient {
    client: reqwest::Client,
    key: Arc<String>,
    endpoint: String,
    cache: Arc<Mutex<Connection>>,
    requests: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Lookup {
    work_id: String,
    codes: Vec<String>,
    #[serde(default)]
    ddc: Vec<String>,
    #[serde(default)]
    bisac: Vec<String>,
}
impl LibraryThingClient {
    pub(crate) fn open(key_file: &Path, provider_file: &Path) -> Result<Self, MetadataError> {
        let key = std::fs::read_to_string(key_file).map_err(|_| MetadataError("could not read LibraryThing key file".into()))?;
        if key.trim().is_empty() {
            return Err(MetadataError("LibraryThing key is empty".into()));
        }
        let client = reqwest::Client::builder().user_agent("BokheimMetadata/1.0").redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15)).build().map_err(error)?;
        Ok(Self { client, key: Arc::new(key.trim().into()), endpoint: ENDPOINT.into(), cache: Arc::new(Mutex::new(crate::crawl::open_provider(provider_file)?)), requests: Arc::new(tokio::sync::Mutex::new(())) })
    }
    pub(crate) async fn lookup_isbn(&self, id: Isbn13) -> Result<(), MetadataError> {
        let isbn = id.to_string();
        let _guard = self.requests.lock().await;
        loop {
            let delay = {
                let c = self.cache.lock().map_err(error)?;
                if c.query_row("SELECT EXISTS(SELECT 1 FROM provider_result WHERE provider='librarything' AND isbn13=?1)", [id], |r| r.get::<_, bool>(0)).map_err(error)? {
                    return Ok(());
                }
                let now = crate::schema::now_ms();
                let next = c.query_row("SELECT next_ms FROM provider_rate WHERE provider='librarything'", [], |r| r.get::<_, i64>(0)).optional().map_err(error)?.unwrap_or(0);
                if next > now {
                    next - now
                } else {
                    crate::crawl::reserve_librarything(&c, now)?;
                    0
                }
            };
            if delay == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(delay as u64)).await;
        }
        let response = self.client.get(&self.endpoint).query(&[("method", "librarything.ck.getwork"), ("isbn", isbn.as_str()), ("apikey", self.key.as_str())]).send().await.map_err(|_| MetadataError("LibraryThing request failed".into()))?;
        if !response.status().is_success() {
            return Err(MetadataError(format!("LibraryThing HTTP {}", response.status().as_u16())));
        }
        if response.content_length().is_some_and(|size| size > MAX_BYTES as u64) {
            return Err(MetadataError("LibraryThing response too large".into()));
        }
        let mut bytes = vec![];
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| MetadataError("LibraryThing response failed".into()))?;
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err(MetadataError("LibraryThing response too large".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        let found = parse(&bytes)?;
        use metadata_contract::{Classification, ClassificationEvidence, ClassificationScheme, ClassificationSource};
        let codes = found
            .codes
            .into_iter()
            .map(|n| (ClassificationScheme::LibraryOfCongress, n))
            .chain(found.ddc.into_iter().map(|n| (ClassificationScheme::DeweyDecimal, n)))
            .chain(found.bisac.into_iter().map(|n| (ClassificationScheme::Bisac, n)))
            .map(|(scheme, notation)| Classification {
                scheme,
                notation,
                source: ClassificationSource::Work,
                evidence: vec![ClassificationEvidence { method: format!("librarything_isbn:{}", found.work_id), isbn13: isbn.clone(), open_library_work_id: String::new(), open_library_edition_id: String::new() }],
            })
            .collect::<Vec<_>>();
        crate::crawl::save_result(&*self.cache.lock().map_err(error)?, "librarything", id, if found.work_id.is_empty() { "no_match" } else { "matched" }, &codes, &[])
    }
}
fn parse(bytes: &[u8]) -> Result<Lookup, MetadataError> {
    let xml = std::str::from_utf8(bytes).map_err(|_| MetadataError("LibraryThing response is not UTF-8".into()))?;
    let doc = roxmltree::Document::parse(xml).map_err(|_| MetadataError("invalid LibraryThing XML".into()))?;
    let root = doc.root_element();
    if root.tag_name().name() == "response"
        && root.attribute("stat") == Some("fail")
        && root.children().any(|n| n.tag_name().name() == "err" && n.attribute("code") == Some("105") && n.text() == Some("Could not determine data ID to retrieve"))
    {
        // Verified response to a valid ISBN with no LibraryThing work.
        return Ok(Lookup { work_id: String::new(), codes: vec![], ddc: vec![], bisac: vec![] });
    }
    if root.tag_name().name() != "response" || root.attribute("stat") != Some("ok") {
        return Err(MetadataError("LibraryThing API did not report success".into()));
    }
    let items: Vec<_> = doc.descendants().filter(|n| n.has_tag_name(("http://www.librarything.com/", "item"))).collect();
    // Unexpected success envelopes are not authoritative empty lookups.
    if items.len() != 1 || items[0].attribute("type") != Some("work") {
        return Err(MetadataError("LibraryThing response has no unique work".into()));
    }
    let item = items[0];
    let id = item.attribute("id").filter(|s| !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_digit())).ok_or_else(|| MetadataError("invalid LibraryThing work ID".into()))?;
    let mut codes = BTreeSet::new();
    let mut ddc = BTreeSet::new();
    let mut bisac = BTreeSet::new();
    for field in item.descendants().filter(|n| n.tag_name().name() == "field") {
        let name = field.attribute("name").unwrap_or_default();
        if !matches!(name, "canonicallcc" | "canonicalddc" | "canonicaldewey" | "canonicalbisac") {
            continue;
        }
        for version in field.descendants().filter(|n| n.tag_name().name() == "version" && n.attribute("archived") == Some("0")) {
            for fact in version.descendants().filter(|n| n.tag_name().name() == "fact") {
                let raw = fact.text().unwrap_or_default().trim();
                if raw.is_empty() {
                    continue;
                }
                use metadata_contract::{classification_consensus_notation, ClassificationScheme};
                match name {
                    "canonicallcc" => {
                        codes.insert(subject_projection::canonical_lcc_notation(raw).ok_or_else(|| MetadataError("LibraryThing returned invalid LCC".into()))?);
                    }
                    "canonicalddc" | "canonicaldewey" => {
                        ddc.insert(classification_consensus_notation(ClassificationScheme::DeweyDecimal, raw).ok_or_else(|| MetadataError("LibraryThing returned invalid DDC".into()))?);
                    }
                    "canonicalbisac" => {
                        bisac.insert(classification_consensus_notation(ClassificationScheme::Bisac, raw).ok_or_else(|| MetadataError("LibraryThing returned invalid BISAC".into()))?);
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    Ok(Lookup { work_id: id.into(), codes: codes.into_iter().collect(), ddc: ddc.into_iter().collect(), bisac: bisac.into_iter().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(fields: &str) -> String {
        format!(r#"<response stat="ok"><ltml xmlns="http://www.librarything.com/"><item id="22550208" type="work"><commonknowledge><fieldList>{fields}</fieldList></commonknowledge></item></ltml></response>"#)
    }
    #[test]
    fn active_lcc_only_and_failures_are_not_misses() {
        let xml = response(
            r#"<field name="canonicallcc"><version archived="1"><fact>DC161</fact></version><version archived="0"><fact>PS3608.O623</fact></version></field><field name="canonicalddc"><version archived="0"><fact>813.6</fact></version></field>"#,
        );
        let result = parse(xml.as_bytes()).unwrap();
        assert_eq!(result.codes, vec!["PS3608.O623"]);
        assert_eq!(result.ddc, vec!["813.6"]);
        assert!(parse(response("").as_bytes()).unwrap().codes.is_empty());
        for xml in ["<html/>", "<response stat='fail'/>", "<response stat='ok'/>", "invalid"] {
            assert!(parse(xml.as_bytes()).is_err());
        }
        assert!(parse(response(r#"<field name="canonicallcc"><version archived="0"><fact>garbage</fact></version></field>"#).as_bytes()).is_err());
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    #[tokio::test]
    async fn completed_results_survive_restart_and_provider_failures_are_not_misses() {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/",get(||async{r#"<response stat="ok"><ltml xmlns="http://www.librarything.com/"><item id="7" type="work"><commonknowledge><fieldList><field name="canonicallcc"><version archived="0"><fact>QA76</fact></version></field></fieldList></commonknowledge></item></ltml></response>"#}))).await.unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key");
        let path = dir.path().join("provider.sqlite");
        std::fs::write(&key, "test-key").unwrap();
        let mut client = LibraryThingClient::open(&key, &path).unwrap();
        client.endpoint = endpoint.clone();
        client.lookup_isbn(Isbn13::parse("9780306406157").unwrap()).await.unwrap();
        let c = client.cache.lock().unwrap();
        let (status, raw): (String, Vec<u8>) = c.query_row("SELECT status,classifications FROM provider_result WHERE provider='librarything'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(status, "matched");
        let codes: Vec<metadata_contract::Classification> = serde_json::from_slice(&raw).unwrap();
        assert_eq!(codes[0].evidence[0].method, "librarything_isbn:7");
        assert_eq!(codes[0].notation, "QA76");
        drop(c);
        task.abort();
        drop(client);
        let mut client = LibraryThingClient::open(&key, &path).unwrap();
        client.endpoint = endpoint;
        client.lookup_isbn(Isbn13::parse("9780306406157").unwrap()).await.unwrap();
        assert!(client.lookup_isbn(Isbn13::parse("9780131103627").unwrap()).await.is_err());
        let c = client.cache.lock().unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM provider_result", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(c.query_row("SELECT used FROM provider_quota WHERE provider='librarything'", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    }
}
