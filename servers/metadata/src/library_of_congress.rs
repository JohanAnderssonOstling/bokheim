//! Server-owned exact-ISBN access to the Library of Congress SRU catalog.
//!
//! The returned LCC, DDC, LCSH, and LCGFT values are transient provider data.
//! This module neither bundles classification schedules nor matches embedded
//! publication subjects against them.

use crate::{canonical_isbn13, Classification, ClassificationScheme, ClassificationSource, MetadataError, SubjectHeading};
use futures_util::StreamExt;
use reqwest::Url;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const PROVIDER_ID: &str = "library_of_congress";
pub(crate) const DEFAULT_SRU_ENDPOINT: &str = "https://lx2.loc.gov/sru/lcdb";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECORDS_PER_IDENTIFIER: usize = 5;
const MAX_CONCURRENT_REQUESTS: usize = 4;

#[derive(Clone)]
pub(crate) struct LibraryOfCongressClient {
    client: reqwest::Client,
    endpoint: Url,
    request_slots: Arc<Semaphore>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LibraryOfCongressLookup {
    Matched { classifications: Vec<Classification>, subjects: Vec<SubjectHeading> },
    NoMatch,
    Ambiguous,
}

#[derive(Clone, Debug)]
struct LibraryOfCongressRecord {
    provider_record_id: Option<String>,
    title: Option<String>,
    isbns: Vec<String>,
    classifications: Vec<Classification>,
    subjects: Vec<SubjectHeading>,
}

impl LibraryOfCongressClient {
    pub(crate) fn new() -> Result<Self, MetadataError> {
        Self::with_endpoint(DEFAULT_SRU_ENDPOINT)
    }

    pub(crate) fn with_endpoint(endpoint: &str) -> Result<Self, MetadataError> {
        let endpoint = Url::parse(endpoint).map_err(|error| MetadataError(format!("invalid Library of Congress SRU endpoint: {error}")))?;
        if endpoint.scheme() != "https" || !endpoint.username().is_empty() || endpoint.password().is_some() || endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(MetadataError("Library of Congress SRU endpoint must be an HTTPS URL without credentials, query, or fragment".to_owned()));
        }
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15)).build().map_err(|error| MetadataError(format!("could not create Library of Congress HTTP client: {error}")))?;
        Ok(Self { client, endpoint, request_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)) })
    }

    /// Queries only checksum-valid ISBNs. Both ISBN-10 and ISBN-13 are tried
    /// when they identify the same 978 edition because LC may index either.
    pub(crate) async fn lookup_isbn(&self, isbn: &str) -> Result<LibraryOfCongressLookup, MetadataError> {
        let queried_isbns = equivalent_isbns(isbn).ok_or_else(|| MetadataError("Library of Congress lookup requires a valid ISBN-10 or ISBN-13".to_owned()))?;
        let _slot = self.request_slots.acquire().await.map_err(|_| MetadataError("Library of Congress request limiter closed".to_owned()))?;
        let mut records = Vec::new();
        let mut seen = HashSet::new();
        for isbn in &queried_isbns {
            let query = format!("bath.isbn=\"{isbn}\"");
            let response = tokio::time::timeout(
                Duration::from_secs(15),
                self.client.get(self.endpoint.clone()).query(&[("version", "1.1"), ("operation", "searchRetrieve"), ("query", query.as_str()), ("maximumRecords", "5"), ("recordSchema", "marcxml")]).send(),
            )
            .await
            .map_err(|_| MetadataError("Library of Congress request timed out".to_owned()))?
            .map_err(|error| MetadataError(format!("Library of Congress request failed: {error}")))?
            .error_for_status()
            .map_err(|error| MetadataError(format!("Library of Congress returned an error: {error}")))?;
            if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
                return Err(MetadataError("Library of Congress response exceeded the size limit".to_owned()));
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| MetadataError(format!("could not read Library of Congress response: {error}")))?;
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(MetadataError("Library of Congress response exceeded the size limit".to_owned()));
                }
                bytes.extend_from_slice(&chunk);
            }
            for record in parse_sru_response(&bytes)? {
                let identity = record.provider_record_id.clone().unwrap_or_else(|| format!("{}\u{1f}{}", record.title.as_deref().unwrap_or_default(), record.isbns.join(",")));
                if seen.insert(identity) {
                    records.push(record);
                }
            }
        }
        let mut matching = records.into_iter().filter(|record| record.isbns.iter().any(|isbn| queried_isbns.contains(isbn))).collect::<Vec<_>>();
        Ok(match matching.len() {
            0 => LibraryOfCongressLookup::NoMatch,
            1 => {
                let record = matching.pop().expect("one matching record");
                LibraryOfCongressLookup::Matched { classifications: record.classifications, subjects: record.subjects }
            }
            _ => LibraryOfCongressLookup::Ambiguous,
        })
    }
}

fn parse_sru_response(bytes: &[u8]) -> Result<Vec<LibraryOfCongressRecord>, MetadataError> {
    let xml = std::str::from_utf8(bytes).map_err(|_| MetadataError("Library of Congress response was not UTF-8 XML".to_owned()))?;
    let document = roxmltree::Document::parse(xml).map_err(|error| MetadataError(format!("invalid Library of Congress XML: {error}")))?;
    if let Some(diagnostic) = document.descendants().find(|node| node.tag_name().name() == "diagnostic") {
        let message = diagnostic.descendants().find(|node| node.tag_name().name() == "message").and_then(|node| node.text()).unwrap_or("unknown SRU diagnostic");
        return Err(MetadataError(format!("Library of Congress SRU diagnostic: {message}")));
    }
    let mut records =
        document.descendants().filter(|node| node.tag_name().name() == "record" && node.tag_name().namespace() == Some("http://www.loc.gov/MARC21/slim")).take(MAX_RECORDS_PER_IDENTIFIER).map(parse_marc_record).collect::<Vec<_>>();
    for record in &mut records {
        record.isbns.sort();
        record.isbns.dedup();
        record.classifications.sort();
        record.classifications.dedup();
        record.subjects.sort();
        record.subjects.dedup();
    }
    Ok(records)
}

fn parse_marc_record(record: roxmltree::Node<'_, '_>) -> LibraryOfCongressRecord {
    let fields = record.children().filter(|node| node.tag_name().name() == "datafield").collect::<Vec<_>>();
    let first = |tag: &str, code: &str| fields.iter().find(|field| field.attribute("tag") == Some(tag)).and_then(|field| subfield(*field, code));
    let provider_record_id = first("010", "a").map(compact_whitespace);
    let title = fields.iter().find(|field| field.attribute("tag") == Some("245")).and_then(|field| {
        let value = [subfield(*field, "a"), subfield(*field, "b")].into_iter().flatten().map(compact_whitespace).collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    });
    let isbns = fields.iter().filter(|field| field.attribute("tag") == Some("020")).filter_map(|field| subfield(*field, "a")).filter_map(|value| value.split_whitespace().next()).filter_map(canonical_isbn).collect();
    let mut classifications = marc_classifications(&fields, "050", ClassificationScheme::LibraryOfCongress);
    classifications.extend(marc_classifications(&fields, "082", ClassificationScheme::DeweyDecimal));
    let subjects = fields.iter().filter(|field| matches!(field.attribute("tag"), Some("600" | "610" | "611" | "630" | "648" | "650" | "651" | "655"))).filter_map(|field| marc_subject(*field)).collect();
    LibraryOfCongressRecord { provider_record_id, title, isbns, classifications, subjects }
}

fn marc_classifications(fields: &[roxmltree::Node<'_, '_>], tag: &str, scheme: ClassificationScheme) -> Vec<Classification> {
    fields
        .iter()
        .filter(|field| field.attribute("tag") == Some(tag))
        .filter_map(|field| {
            let code = [subfield(*field, "a"), subfield(*field, "b")].into_iter().flatten().map(compact_whitespace).collect::<Vec<_>>().join(" ");
            (!code.is_empty()).then(|| Classification { scheme, notation: code, source: ClassificationSource::ExactEdition })
        })
        .collect()
}

fn marc_subject(field: roxmltree::Node<'_, '_>) -> Option<SubjectHeading> {
    let components = field
        .children()
        .filter(|node| node.tag_name().name() == "subfield" && matches!(node.attribute("code"), Some("a" | "v" | "x" | "y" | "z")))
        .filter_map(|node| node.text())
        .map(compact_whitespace)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return None;
    }
    let authority = subfield(field, "2").map(compact_whitespace).or_else(|| (field.attribute("ind2") == Some("0")).then(|| "lcsh".to_owned())).unwrap_or_else(|| "unspecified".to_owned());
    let authority = authority.to_ascii_lowercase();
    let authority_uri = subfield(field, "0").map(compact_whitespace).filter(|value| value.starts_with("https://id.loc.gov/") || value.starts_with("http://id.loc.gov/"));
    Some(SubjectHeading { source: PROVIDER_ID.to_owned(), authority, components, authority_uri })
}

fn subfield<'input>(field: roxmltree::Node<'input, 'input>, code: &str) -> Option<&'input str> {
    field.children().find(|node| node.tag_name().name() == "subfield" && node.attribute("code") == Some(code)).and_then(|node| node.text())
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['/', ':', ';', ',', '.']).to_owned()
}

fn equivalent_isbns(value: &str) -> Option<Vec<String>> {
    let canonical = canonical_isbn(value)?;
    let mut values = vec![canonical.clone()];
    if let Some(equivalent) = isbn13_to_isbn10(&canonical) {
        values.push(equivalent);
    }
    Some(values)
}

fn canonical_isbn(value: &str) -> Option<String> {
    canonical_isbn13(value).map(|isbn| format!("{isbn:013}"))
}

fn isbn13_to_isbn10(value: &str) -> Option<String> {
    let canonical = canonical_isbn(value)?;
    if canonical.len() != 13 || !canonical.starts_with("978") {
        return None;
    }
    let mut digits = canonical[3..12].to_owned();
    let sum = digits.bytes().enumerate().map(|(index, byte)| u32::from(byte - b'0') * (10 - index as u32)).sum::<u32>();
    let check = (11 - sum % 11) % 11;
    digits.push(if check == 10 { 'X' } else { char::from(b'0' + check as u8) });
    Some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARC_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/">
  <records><record><recordData>
    <marc:record xmlns:marc="http://www.loc.gov/MARC21/slim">
      <marc:datafield tag="010"><marc:subfield code="a">  2019000001 </marc:subfield></marc:datafield>
      <marc:datafield tag="020"><marc:subfield code="a">978-0-306-40615-7 (hardcover)</marc:subfield></marc:datafield>
      <marc:datafield tag="245"><marc:subfield code="a">A test book :</marc:subfield><marc:subfield code="b">metadata.</marc:subfield></marc:datafield>
      <marc:datafield tag="050"><marc:subfield code="a">QA76.9</marc:subfield><marc:subfield code="b">.D3 2019</marc:subfield></marc:datafield>
      <marc:datafield tag="082"><marc:subfield code="a">005.1</marc:subfield></marc:datafield>
      <marc:datafield tag="650" ind2="0"><marc:subfield code="a">Computer software</marc:subfield><marc:subfield code="x">Development</marc:subfield><marc:subfield code="0">https://id.loc.gov/authorities/subjects/sh00000001</marc:subfield></marc:datafield>
      <marc:datafield tag="655"><marc:subfield code="a">Handbooks and manuals</marc:subfield><marc:subfield code="2">lcgft</marc:subfield></marc:datafield>
    </marc:record>
  </recordData></record></records>
</searchRetrieveResponse>"#;

    #[test]
    fn parses_lcc_ddc_and_authority_subjects() {
        let records = parse_sru_response(MARC_RESPONSE.as_bytes()).expect("valid MARC XML");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.isbns, ["9780306406157"]);
        assert!(record.classifications.contains(&Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.9 .D3 2019".to_owned(), source: ClassificationSource::ExactEdition }));
        assert!(record.classifications.contains(&Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "005.1".to_owned(), source: ClassificationSource::ExactEdition }));
        assert!(record.subjects.contains(&SubjectHeading {
            source: PROVIDER_ID.to_owned(),
            authority: "lcsh".to_owned(),
            components: vec!["Computer software".to_owned(), "Development".to_owned()],
            authority_uri: Some("https://id.loc.gov/authorities/subjects/sh00000001".to_owned()),
        }));
        assert!(record.subjects.iter().any(|subject| subject.authority == "lcgft" && subject.components == ["Handbooks and manuals"]));
    }

    #[test]
    fn derives_the_equivalent_isbn10_for_a_978_isbn() {
        assert_eq!(equivalent_isbns("978-0-306-40615-7"), Some(vec!["9780306406157".to_owned(), "0306406152".to_owned()]));
    }
}
