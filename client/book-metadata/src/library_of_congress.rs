//! Exact-ISBN access to the Library of Congress SRU catalog.
//!
//! The returned LCC, DDC, LCSH, and LCGFT values are transient provider data.
//! This module neither bundles classification schedules nor matches embedded
//! publication subjects against them.

use crate::external::{
    ExternalClassification, ExternalClassificationScheme, ExternalMetadataError, ExternalMetadataFuture, ExternalMetadataLookup, ExternalMetadataProvider, ExternalMetadataProviderId, ExternalMetadataRecord, ExternalSubject,
    ExternalSubjectScheme,
};
use library_model::canonical_isbn;
use reqwest::Url;
use std::collections::HashSet;
use std::time::Duration;

const PROVIDER_ID: &str = "library_of_congress";
const DEFAULT_SRU_ENDPOINT: &str = "http://lx2.loc.gov:210/lcdb";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECORDS_PER_IDENTIFIER: usize = 5;

#[derive(Clone)]
pub struct LibraryOfCongressProvider {
    client: reqwest::Client,
    endpoint: Url,
}

impl LibraryOfCongressProvider {
    pub fn new() -> Result<Self, ExternalMetadataError> {
        Self::with_endpoint(DEFAULT_SRU_ENDPOINT)
    }

    pub fn with_endpoint(endpoint: &str) -> Result<Self, ExternalMetadataError> {
        let endpoint = Url::parse(endpoint).map_err(|error| ExternalMetadataError::new(format!("invalid Library of Congress SRU endpoint: {error}")))?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent("Bokheim/0.1 (Library of Congress metadata lookup)")
            .build()
            .map_err(|error| ExternalMetadataError::new(format!("could not create Library of Congress HTTP client: {error}")))?;
        Ok(Self { client, endpoint })
    }

    /// Queries only checksum-valid ISBNs. Both ISBN-10 and ISBN-13 are tried
    /// when they identify the same 978 edition because LC may index either.
    pub async fn lookup_isbn(&self, isbn: &str) -> Result<ExternalMetadataLookup, ExternalMetadataError> {
        let queried_isbns = equivalent_isbns(isbn).ok_or_else(|| ExternalMetadataError::new("Library of Congress lookup requires a valid ISBN-10 or ISBN-13"))?;
        let mut records = Vec::new();
        let mut seen = HashSet::new();
        for isbn in &queried_isbns {
            let query = format!("bath.isbn=\"{isbn}\"");
            let mut response = self
                .client
                .get(self.endpoint.clone())
                .query(&[("version", "1.1"), ("operation", "searchRetrieve"), ("query", query.as_str()), ("maximumRecords", "5"), ("recordSchema", "marcxml")])
                .send()
                .await
                .map_err(|error| ExternalMetadataError::new(format!("Library of Congress request failed: {error}")))?
                .error_for_status()
                .map_err(|error| ExternalMetadataError::new(format!("Library of Congress returned an error: {error}")))?;
            if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
                return Err(ExternalMetadataError::new("Library of Congress response exceeded the size limit"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| ExternalMetadataError::new(format!("could not read Library of Congress response: {error}")))? {
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(ExternalMetadataError::new("Library of Congress response exceeded the size limit"));
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
        Ok(ExternalMetadataLookup { provider: self.id(), queried_isbns, records })
    }
}

impl ExternalMetadataProvider for LibraryOfCongressProvider {
    fn id(&self) -> ExternalMetadataProviderId {
        ExternalMetadataProviderId::new(PROVIDER_ID)
    }

    fn lookup_isbn<'a>(&'a self, isbn: &'a str) -> ExternalMetadataFuture<'a> {
        Box::pin(async move { LibraryOfCongressProvider::lookup_isbn(self, isbn).await })
    }
}

fn parse_sru_response(bytes: &[u8]) -> Result<Vec<ExternalMetadataRecord>, ExternalMetadataError> {
    let xml = std::str::from_utf8(bytes).map_err(|_| ExternalMetadataError::new("Library of Congress response was not UTF-8 XML"))?;
    let document = roxmltree::Document::parse(xml).map_err(|error| ExternalMetadataError::new(format!("invalid Library of Congress XML: {error}")))?;
    if let Some(diagnostic) = document.descendants().find(|node| node.tag_name().name() == "diagnostic") {
        let message = diagnostic.descendants().find(|node| node.tag_name().name() == "message").and_then(|node| node.text()).unwrap_or("unknown SRU diagnostic");
        return Err(ExternalMetadataError::new(format!("Library of Congress SRU diagnostic: {message}")));
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

fn parse_marc_record(record: roxmltree::Node<'_, '_>) -> ExternalMetadataRecord {
    let fields = record.children().filter(|node| node.tag_name().name() == "datafield").collect::<Vec<_>>();
    let first = |tag: &str, code: &str| fields.iter().find(|field| field.attribute("tag") == Some(tag)).and_then(|field| subfield(*field, code));
    let provider_record_id = first("010", "a").map(compact_whitespace);
    let title = fields.iter().find(|field| field.attribute("tag") == Some("245")).and_then(|field| {
        let value = [subfield(*field, "a"), subfield(*field, "b")].into_iter().flatten().map(compact_whitespace).collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    });
    let isbns = fields.iter().filter(|field| field.attribute("tag") == Some("020")).filter_map(|field| subfield(*field, "a")).filter_map(|value| value.split_whitespace().next()).filter_map(canonical_isbn).collect();
    let mut classifications = marc_classifications(&fields, "050", ExternalClassificationScheme::Lcc);
    classifications.extend(marc_classifications(&fields, "082", ExternalClassificationScheme::Ddc));
    let subjects = fields.iter().filter(|field| matches!(field.attribute("tag"), Some("600" | "610" | "611" | "630" | "648" | "650" | "651" | "655"))).filter_map(|field| marc_subject(*field)).collect();
    ExternalMetadataRecord { provider_record_id, title, isbns, classifications, subjects }
}

fn marc_classifications(fields: &[roxmltree::Node<'_, '_>], tag: &str, scheme: ExternalClassificationScheme) -> Vec<ExternalClassification> {
    fields
        .iter()
        .filter(|field| field.attribute("tag") == Some(tag))
        .filter_map(|field| {
            let code = [subfield(*field, "a"), subfield(*field, "b")].into_iter().flatten().map(compact_whitespace).collect::<Vec<_>>().join(" ");
            (!code.is_empty()).then(|| ExternalClassification { scheme: scheme.clone(), code, edition: subfield(*field, "2").map(compact_whitespace) })
        })
        .collect()
}

fn marc_subject(field: roxmltree::Node<'_, '_>) -> Option<ExternalSubject> {
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
    let scheme = match authority.to_ascii_lowercase().as_str() {
        "lcsh" => ExternalSubjectScheme::Lcsh,
        "lcgft" => ExternalSubjectScheme::Lcgft,
        _ => ExternalSubjectScheme::Other(authority.to_ascii_lowercase()),
    };
    let authority_uri = subfield(field, "0").map(compact_whitespace).filter(|value| value.starts_with("https://id.loc.gov/") || value.starts_with("http://id.loc.gov/"));
    Some(ExternalSubject { scheme, components, authority_uri })
}

fn subfield<'input>(field: roxmltree::Node<'input, 'input>, code: &str) -> Option<&'input str> {
    field.children().find(|node| node.tag_name().name() == "subfield" && node.attribute("code") == Some(code)).and_then(|node| node.text())
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['/', ':', ';', ',', '.']).to_owned()
}

fn equivalent_isbns(value: &str) -> Option<Vec<String>> {
    let canonical = canonical_isbn(value)?;
    let equivalent = match canonical.len() {
        10 => isbn10_to_isbn13(&canonical),
        13 => isbn13_to_isbn10(&canonical),
        _ => None,
    };
    let mut values = vec![canonical];
    if let Some(equivalent) = equivalent {
        if !values.contains(&equivalent) {
            values.push(equivalent);
        }
    }
    Some(values)
}

fn isbn10_to_isbn13(value: &str) -> Option<String> {
    let canonical = canonical_isbn(value)?;
    (canonical.len() == 10).then_some(())?;
    let mut digits = format!("978{}", &canonical[..9]);
    let sum = digits.bytes().enumerate().map(|(index, byte)| u32::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>();
    digits.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
    Some(digits)
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
