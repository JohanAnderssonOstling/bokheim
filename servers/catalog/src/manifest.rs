//! Explicit acquisition manifests supply source metadata before enrichment.
//! Files stay in the acquisition directory so imports can be audited or resumed.
use crate::{curation, get, store_book, store_book_preserving_covers};
use book_model::{BookRecord, BookSubject, Contributor, Identifier, LanguageTag, Scheme};
use reqwest::blocking::Client;
use reqwest::Url;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceMetadata {
    title: String,
    description: String,
    languages: Vec<LanguageTag>,
    year: Option<i32>,
    identifiers: Vec<Identifier>,
    subjects: Vec<BookSubject>,
    contributors: Vec<Contributor>,
}

impl SourceMetadata {
    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn from_record(record: &BookRecord) -> Self {
        Self {
            title: record.title.clone(),
            description: record.description.clone(),
            languages: record.book.languages.clone(),
            year: record.book.year,
            identifiers: record.book.identifiers.clone(),
            subjects: record.book.subjects.clone(),
            contributors: record.contributors.clone(),
        }
    }

    pub fn apply(&self, record: &mut BookRecord) {
        if !self.title.trim().is_empty() {
            record.title.clone_from(&self.title);
        }
        if !self.description.trim().is_empty() {
            record.description.clone_from(&self.description);
        }
        if !self.languages.is_empty() {
            record.book.languages.clone_from(&self.languages);
        }
        if self.year.is_some() {
            record.book.year = self.year;
        }
        self.retain_evidence(record);
        for contributor in &self.contributors {
            if !record.contributors.iter().any(|existing| existing.name() == contributor.name() && existing.role_code() == contributor.role_code()) {
                record.contributors.push(contributor.clone());
            }
        }
    }

    /// Add source evidence and fill missing fields without replacing curated text.
    pub fn retain_evidence(&self, record: &mut BookRecord) {
        if record.description.trim().is_empty() && !self.description.trim().is_empty() {
            record.description.clone_from(&self.description);
        }
        if record.book.year.is_none() {
            record.book.year = self.year;
        }
        for identifier in &self.identifiers {
            let present = record.book.identifiers.iter().any(|existing| {
                existing == identifier || (identifier.scheme() == &Scheme::Isbn && existing.scheme() == &Scheme::Isbn && identifier.canonical_value().is_some_and(|value| existing.canonical_value().as_deref() == Some(value.as_str())))
            });
            if !present {
                record.book.identifiers.push(identifier.clone());
            }
        }
        for subject in &self.subjects {
            if !record.book.subjects.contains(subject) {
                record.book.subjects.push(subject.clone());
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: PathBuf,
    /// Read this EPUB member directly into memory, without an extraction inbox.
    #[serde(default)]
    archive_member: Option<String>,
    /// Fetch the acquisition URL into memory instead of reading a local file.
    #[serde(default)]
    download: bool,
    #[serde(default)]
    preserve_covers: bool,
    source: String,
    entry_id: String,
    entry_url: String,
    file_url: String,
    #[serde(default)]
    metadata: SourceMetadata,
    /// Source observations (counts, edition fields, related-site links) survive
    /// enrichment and publication without being mistaken for book metadata.
    #[serde(default)]
    source_evidence: Option<Value>,
}

pub fn import(dir: &Path, db: &Connection, http: &Client, path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    // Validate all provenance before adding any of the manifest's books.
    for entry in &entries {
        if entry.source.trim().is_empty() || entry.entry_id.trim().is_empty() {
            return Err("manifest source and entry_id must be nonempty".into());
        }
        for value in [&entry.entry_url, &entry.file_url] {
            let url = Url::parse(value).map_err(|e| format!("{value}: {e}"))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(format!("manifest URL must use HTTP or HTTPS: {value}"));
            }
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for entry in entries {
        let file = parent.join(&entry.file);
        let (name, bytes) = if entry.download {
            if entry.archive_member.is_some() {
                return Err("download and archive_member cannot be combined".into());
            }
            let url = Url::parse(&entry.file_url).map_err(|e| e.to_string())?;
            let name = file.file_name().and_then(|name| name.to_str()).ok_or("manifest file has no UTF-8 filename")?.to_owned();
            let bytes = get(http, &url)?;
            if entry.source == "standardebooks.org" {
                verify_standardebooks_identity(&bytes, &entry.entry_id)?;
            }
            (name, bytes)
        } else {
            read_epub(&file, entry.archive_member.as_deref())?
        };
        let hash = if entry.preserve_covers {
            store_book_preserving_covers(dir, db, http, &bytes, &name, Some(&entry.metadata))?
        } else {
            store_book(dir, db, http, &bytes, &name, Vec::new(), None, Some(&entry.metadata))?
        };
        curation::add_source(db, &hash, &entry.source, &entry.entry_id, Some(&entry.entry_url), Some(&entry.file_url))?;
        if let Some(evidence) = entry.source_evidence {
            retain_source_evidence(db, &entry.source, &entry.entry_id, &entry.entry_url, &hash, &evidence)?;
        }
    }
    Ok(())
}

fn verify_standardebooks_identity(bytes: &[u8], expected: &str) -> Result<(), String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut container = String::new();
    archive.by_name("META-INF/container.xml").map_err(|e| e.to_string())?.read_to_string(&mut container).map_err(|e| e.to_string())?;
    let container = roxmltree::Document::parse(&container).map_err(|e| e.to_string())?;
    let package = container.descendants().find(|node| node.has_tag_name("rootfile")).and_then(|node| node.attribute("full-path")).ok_or("EPUB has no package path")?;
    let mut xml = String::new();
    archive.by_name(package).map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    let package = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    if !package.descendants().filter(|node| node.has_tag_name(("http://purl.org/dc/elements/1.1/", "identifier"))).filter_map(|node| node.text()).any(|value| value.trim().trim_start_matches("url:").trim_end_matches('/') == expected.trim_end_matches('/')) {
        return Err(format!("downloaded EPUB does not match Standard Ebooks identity {expected}"));
    }
    Ok(())
}

fn read_epub(file: &Path, member: Option<&str>) -> Result<(String, Vec<u8>), String> {
    let Some(member) = member else {
        let name = file.file_name().and_then(|name| name.to_str()).ok_or("manifest file has no UTF-8 filename")?;
        return Ok((name.to_owned(), std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?));
    };
    if !member.to_ascii_lowercase().ends_with(".epub") {
        return Err("archive member must be an EPUB".into());
    }
    let mut archive = ZipArchive::new(File::open(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut entry = archive.by_name(member).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok((member.to_owned(), bytes))
}

fn retain_source_evidence(db: &Connection, source: &str, entry_id: &str, entry_url: &str, hash: &str, evidence: &Value) -> Result<(), String> {
    db.execute(
        "INSERT INTO source_entry(source,entry_id,entry_url,book_hash,metadata) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,book_hash=excluded.book_hash,metadata=CASE WHEN source_entry.book_hash=excluded.book_hash AND json_type(source_entry.metadata,'$.cover')='object' THEN json_set(excluded.metadata,'$.cover',json_extract(source_entry.metadata,'$.cover')) ELSE excluded.metadata END",
        params![source, entry_id, entry_url, hash, evidence.to_string()],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{BookDetails, BookMetadata, Scheme, Scope};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn downloaded_standardebooks_must_match_the_requested_identity() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("META-INF/container.xml", SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>"#).unwrap();
        zip.start_file("book.opf", SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:identifier>url:https://standardebooks.org/ebooks/an-author/a-book</dc:identifier></metadata></package>"#).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        verify_standardebooks_identity(&bytes, "https://standardebooks.org/ebooks/an-author/a-book").unwrap();
        assert!(verify_standardebooks_identity(&bytes, "https://standardebooks.org/ebooks/an-author/another-book").is_err());
        assert!(verify_standardebooks_identity(b"not an EPUB", "https://standardebooks.org/ebooks/an-author/a-book").is_err());
    }

    #[test]
    fn archive_member_is_read_without_extracting_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("books.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("nested/book.epub", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"epub bytes").unwrap();
        zip.finish().unwrap();
        let (name, bytes) = read_epub(&path, Some("nested/book.epub")).unwrap();
        assert_eq!(name, "nested/book.epub");
        assert_eq!(bytes, b"epub bytes");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(read_epub(&path, Some("missing.epub")).is_err());
        assert!(read_epub(&path, Some("not-a-book.txt")).is_err());
    }

    #[test]
    fn website_evidence_survives_reimport_without_becoming_book_facts() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        curation::set_record(&db, "book-hash", &record).unwrap();
        let evidence = serde_json::json!({"website_metadata": {"faves": 2, "downloads": 386,
            "provider_links": {"openlibrary": ["https://unglue.it/work/426628/openlibrary/"]},
            "identifiers": [{"scheme": "doi", "value": "10.1234/example"}]}});
        for _ in 0..2 {
            retain_source_evidence(&db, "unglue.it", "426628", "https://unglue.it/work/426628/", "book-hash", &evidence).unwrap();
        }
        let (count, stored): (i64, String) = db.query_row("SELECT count(*),metadata FROM source_entry WHERE source='unglue.it' AND entry_id='426628'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(count, 1);
        assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), evidence);
    }

    #[test]
    fn source_refresh_preserves_retained_cover_only_for_the_same_book() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        for hash in ["original-book", "changed-book"] {
            curation::set_record(&db, hash, &record).unwrap();
        }
        let original = serde_json::json!({"downloads":1,"cover":{"source_url":"https://books.google.com/cover","image_bytes":123}});
        retain_source_evidence(&db, "gutenberg.org", "1", "https://www.gutenberg.org/ebooks/1", "original-book", &original).unwrap();
        let fresh = serde_json::json!({"downloads":2});
        retain_source_evidence(&db, "gutenberg.org", "1", "https://www.gutenberg.org/ebooks/1", "original-book", &fresh).unwrap();
        let read = || -> Value {
            let raw: String = db.query_row("SELECT metadata FROM source_entry WHERE source='gutenberg.org' AND entry_id='1'", [], |row| row.get(0)).unwrap();
            serde_json::from_str(&raw).unwrap()
        };
        assert_eq!(read(), serde_json::json!({"downloads":2,"cover":original["cover"]}));
        retain_source_evidence(&db, "gutenberg.org", "1", "https://www.gutenberg.org/ebooks/1", "changed-book", &fresh).unwrap();
        assert_eq!(read(), fresh);
    }

    #[test]
    fn source_corrections_preserve_file_identity_and_other_evidence() {
        let identifier = Identifier::new("file-id", Scheme::Unspecified, Scope::Book).unwrap();
        let mut record = BookRecord {
            metadata: BookDetails {
                title: "File title".into(),
                book: BookMetadata { languages: vec![LanguageTag::parse("sv").unwrap()], year: Some(2023), identifiers: vec![identifier.clone()], ..Default::default() },
                ..Default::default()
            },
            description: String::new(),
        };
        let metadata: SourceMetadata = serde_json::from_str(r#"{"languages":["se"],"year":1915,"description":"Source introduction"}"#).unwrap();
        metadata.apply(&mut record);
        assert_eq!(record.title, "File title");
        assert_eq!(record.book.identifiers, [identifier.clone()]);
        assert_eq!(record.book.languages[0].as_str(), "se");
        assert_eq!(record.book.year, Some(1915));
        assert_eq!(record.description, "Source introduction");
        let corrected: SourceMetadata = serde_json::from_str(r#"{"title":"Website title"}"#).unwrap();
        corrected.apply(&mut record);
        assert_eq!(record.title, "Website title");
        assert_eq!(record.book.identifiers, [identifier]);
        let expected = record.clone();
        SourceMetadata::default().apply(&mut record);
        assert_eq!(record, expected);
        assert!(serde_json::from_str::<SourceMetadata>(r#"{"languages":["invalid language"]}"#).is_err());
    }
}
