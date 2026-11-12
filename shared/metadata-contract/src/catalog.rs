//! Source attribution and audiobook evidence for published catalogs.
use serde_json::Value;

pub struct CatalogEvidence {
    pub sources: Vec<String>,
    pub external_audio: bool,
    pub audiobook_duration_ms: Option<u64>,
}

pub fn source_evidence(source: &str, metadata: &Value) -> Result<CatalogEvidence, String> {
    let mut sources = Vec::new();
    if source == "bokheim:combined-volumes" {
        for component in metadata["components"].as_array().into_iter().flatten() {
            for entry in component["source_entries"].as_array().into_iter().flatten() {
                let Some(original) = entry["source"].as_str() else { continue };
                if !sources.iter().any(|name| name == original) {
                    sources.push(original.to_owned());
                }
            }
        }
    }
    Ok(CatalogEvidence {
        sources,
        external_audio: metadata["audiobook_download_url"].as_str().is_some() || metadata["audiobook_delivery"] == "external_download",
        audiobook_duration_ms: metadata["audiobook_duration_ms"].as_u64().filter(|duration| *duration > 0 && *duration <= i64::MAX as u64),
    })
}

pub fn source_priority(source: &str) -> u8 {
    match source {
        "publicdomainlibrary.org" => 0,
        "standardebooks.org" => 1,
        "litteraturbanken.se" => 2,
        "unglue.it" => 3,
        "loyalbooks.com" => 4,
        "gutenberg.org" => 5,
        _ => 6,
    }
}
