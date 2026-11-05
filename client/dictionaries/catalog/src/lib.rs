//! Reviewed, checksum-pinned StarDict releases. IDs identify immutable
//! editions; hosted archives use their content hash as the filename.
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CuratedDictionary {
    pub id: String,
    pub language: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub source_url: String,
    pub license: String,
    pub upstream_url: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<PreparedArchive>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparedArchive {
    pub bytes: u64,
    pub sha256: String,
}

pub fn dictionaries() -> &'static [CuratedDictionary] {
    static CATALOG: OnceLock<Vec<CuratedDictionary>> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(include_str!("../../../../shared/dictionary-catalog/data/curated.json")).expect("bundled dictionary catalog is validated by tests"))
}

pub fn find(id: &str) -> Option<&'static CuratedDictionary> {
    dictionaries().iter().find(|item| item.id == id)
}

/// BCP-47 locales, POSIX locales and common ISO-639-2 book tags all select
/// the same base-language pack. A region is never treated as a language.
pub fn language(tag: &str) -> Option<String> {
    let base = tag.trim().split(['-', '_', '.', '@']).next()?.to_ascii_lowercase();
    let base = match base.as_str() {
        "eng" => "en",
        "swe" => "sv",
        "deu" | "ger" => "de",
        "fra" | "fre" => "fr",
        "spa" => "es",
        "ita" => "it",
        "nld" | "dut" => "nl",
        "por" => "pt",
        "ara" => "ar",
        "ces" | "cze" => "cs",
        "ell" | "gre" => "el",
        "ind" => "id",
        "jpn" => "ja",
        "kor" => "ko",
        "pol" => "pl",
        "rus" => "ru",
        "tur" => "tr",
        "zho" | "chi" => "zh",
        _ => &base,
    };
    ((base.len() == 2 || base.len() == 3) && base.bytes().all(|byte| byte.is_ascii_lowercase()) && !matches!(base, "und" | "mul" | "zxx" | "posix")).then(|| base.to_owned())
}

pub fn for_language(tag: &str) -> Option<&'static CuratedDictionary> {
    let language = language(tag)?;
    dictionaries().iter().find(|item| item.language == language)
}

impl CuratedDictionary {
    /// Prepared packs are shared by desktop and web. Original upstream pins
    /// remain available for reproducible preparation and older clients.
    pub fn hosted_url(&self) -> String {
        format!("https://app.bokheim.se/dictionaries/{}.zip", self.prepared.as_ref().map_or(&self.sha256, |archive| &archive.sha256))
    }

    pub fn download_bytes(&self) -> u64 {
        self.prepared.as_ref().map_or(self.bytes, |archive| archive.bytes)
    }

    pub fn download_url(&self) -> String {
        if cfg!(target_arch = "wasm32") || self.prepared.is_some() { self.hosted_url() } else { self.upstream_url.clone() }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<(), String> {
        self.verify_hash(bytes.len(), &format!("{:x}", Sha256::digest(bytes)))
    }

    fn verify_hash(&self, size: usize, hash: &str) -> Result<(), String> {
        if size as u64 != self.download_bytes() {
            return Err("Dictionary download has an unexpected size".into());
        }
        let expected = self.prepared.as_ref().map_or(&self.sha256, |archive| &archive.sha256);
        if hash != expected.as_str() {
            return Err("Dictionary download failed checksum verification".into());
        }
        Ok(())
    }
}

/// Stream into a bounded buffer before verification. Failed downloads never
/// become installed packs, regardless of which host serves the archive.
#[cfg(feature = "download")]
pub async fn download(spec: &CuratedDictionary, url: &str) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;
    use std::time::Duration;
    let client = http_response::client_builder().build().map_err(|error| error.to_string())?;
    let response = client.get(url).timeout(Duration::from_secs(300)).send().await.and_then(reqwest::Response::error_for_status).map_err(|error| format!("Dictionary download failed: {error}"))?;
    if response.content_length().is_some_and(|size| size > spec.download_bytes()) {
        return Err("Dictionary download exceeds the expected size".into());
    }
    let mut stream = Box::pin(response.bytes_stream());
    let mut bytes = Vec::new();
    let mut hash = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Dictionary download interrupted: {error}"))?;
        if bytes.len() as u64 + chunk.len() as u64 > spec.download_bytes() {
            return Err("Dictionary download exceeds the expected size".into());
        }
        hash.update(&chunk);
        bytes.extend_from_slice(&chunk);
    }
    spec.verify_hash(bytes.len(), &format!("{:x}", hash.finalize()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_entries_are_unique_bounded_https_releases() {
        let mut ids = HashSet::new();
        let mut languages = HashSet::new();
        for item in dictionaries() {
            assert!(ids.insert(&item.id));
            assert!(languages.insert(&item.language));
            assert!(item.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.')));
            assert!(item.upstream_url.starts_with("https://"));
            assert!(item.source_url.starts_with("https://"));
            assert!(!item.license.is_empty());
            assert!(item.bytes > 0 && item.bytes <= 256 * 1024 * 1024);
            assert_eq!(item.sha256.len(), 64);
            assert!(item.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            if let Some(prepared) = &item.prepared {
                assert!(prepared.bytes > 0 && prepared.bytes <= 256 * 1024 * 1024);
                assert_eq!(prepared.sha256.len(), 64);
                assert!(prepared.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            }
        }
        assert!(for_language("de-DE").is_some());
        assert!(for_language("sv-SE").is_some());
    }

    #[test]
    fn normalizes_locales_and_book_tags_without_guessing_regions_or_unknown_languages() {
        for tag in ["de-DE", "de_DE.UTF-8", "deu", "ger", " DE-at "] {
            assert_eq!(for_language(tag).unwrap().language, "de");
        }
        assert_eq!(for_language("sv_SE@euro").unwrap().language, "sv");
        assert!(for_language("zz-DE").is_none());
        assert!(for_language("und").is_none());
        assert!(for_language("C.UTF-8").is_none());
    }

    #[test]
    fn verification_rejects_truncation_and_same_length_corruption() {
        let mut spec = dictionaries()[0].clone();
        spec.prepared = None;
        spec.bytes = 3;
        spec.sha256 = format!("{:x}", Sha256::digest(b"zip"));
        assert!(spec.verify(b"zip").is_ok());
        assert!(spec.verify(b"zi").is_err());
        assert!(spec.verify(b"bad").is_err());
        spec.prepared = Some(PreparedArchive { bytes: 8, sha256: format!("{:x}", Sha256::digest(b"prepared")) });
        assert_eq!(spec.download_bytes(), 8);
        assert!(spec.hosted_url().contains(&spec.prepared.as_ref().unwrap().sha256));
        assert_eq!(spec.download_url(), spec.hosted_url());
        assert!(spec.verify(b"prepared").is_ok());
        assert!(spec.verify(b"zip").is_err(), "upstream and prepared pins cannot be interchanged");
    }

    #[cfg(feature = "download")]
    #[tokio::test]
    #[ignore = "downloads a checksum-pinned public dictionary (about 1 MiB)"]
    async fn verified_curated_download() {
        let spec = for_language("ar").unwrap();
        let bytes = download(spec, &spec.download_url()).await.unwrap();
        assert!(bytes.starts_with(b"PK"));
        spec.verify(&bytes).unwrap();
    }
}
