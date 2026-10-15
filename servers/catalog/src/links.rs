//! Metadata-only catalog entries. Access remains on the named source page.
use crate::{curation, enrich, get};
use book_model::{BookFormat, BookRecord, BookTocEntry};
use reqwest::blocking::Client;
use reqwest::Url;
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub source: String,
    pub entry_id: String,
    pub entry_url: String,
    pub license: String,
    pub record: BookRecord,
    #[serde(default)]
    pub file_url: Option<String>,
    /// Source-declared media format, independent of redistribution rights.
    /// Older link manifests described EPUBs only.
    #[serde(default = "epub_format")]
    pub format: BookFormat,
    #[serde(default)]
    pub toc: Vec<BookTocEntry>,
    /// Website observations retained independently of enriched book facts.
    #[serde(default)]
    pub source_evidence: Option<Value>,
    /// The metadata server answered the analysis: the record's authors
    /// carry its identities. Not part of a manifest.
    #[serde(skip)]
    pub enriched: bool,
}

fn epub_format() -> BookFormat {
    BookFormat::Epub
}

pub fn import(db: &Connection, http: &Client, path: &Path, analyze: bool) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    validate(&entries)?;
    if analyze {
        if entries.iter().any(|entry| entry.file_url.is_none()) {
            return Err("analysis requires a file_url for every link entry".into());
        }
        let count = entries.len();
        let done = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let jobs = entries
                .chunks_mut(count.div_ceil(4).max(1))
                .map(|chunk| {
                    let done = &done;
                    scope.spawn(move || -> Result<(), String> {
                        for entry in chunk {
                            let url = Url::parse(entry.file_url.as_deref().unwrap()).map_err(|e| e.to_string())?;
                            let mut attempt = 0;
                            let bytes = loop {
                                match get(http, &url) {
                                    Ok(bytes) => break bytes,
                                    Err(error) if attempt < 2 => {
                                        attempt += 1;
                                        eprintln!("retry {attempt}: {error}");
                                    }
                                    Err(error) => return Err(error),
                                }
                            };
                            let bytes_len = bytes.len();
                            let file_name = url.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or("book.epub");
                            let (record, toc) = inspect(entry, file_name, bytes)?;
                            // Enrichment may add classification; any cover reference is ignored.
                            entry.record = match enrich(http, record.clone(), file_name, false) {
                                Ok((record, _, found)) => {
                                    entry.enriched = found;
                                    record
                                }
                                Err(error) => {
                                    eprintln!("{}: keeping inspected metadata: {error}", entry.entry_id);
                                    record
                                }
                            };
                            entry.toc = toc;
                            let done = done.fetch_add(1, Ordering::Relaxed) + 1;
                            eprintln!("analyzed {done}/{count}: {} ({bytes_len} bytes discarded)", entry.entry_id);
                        }
                        Ok(())
                    })
                })
                .collect::<Vec<_>>();
            for job in jobs {
                job.join().map_err(|_| "link analysis worker panicked".to_owned())??;
            }
            Ok::<_, String>(())
        })?;
    }
    import_entries(db, &entries)
}

/// The inspection owns the download buffer; dropping its reader discards the
/// EPUB before this function returns. No file or cover is written to disk.
fn inspect(entry: &Entry, file_name: &str, bytes: Vec<u8>) -> Result<(BookRecord, Vec<BookTocEntry>), String> {
    let inspected = book_inspection::inspect_book_reader(Path::new(file_name), entry.format, Cursor::new(bytes)).map_err(|e| format!("{}: {e}", entry.entry_id))?;
    let mut record = inspected.metadata;
    record.title.clone_from(&entry.record.title);
    if !entry.record.book.languages.is_empty() {
        record.book.languages.clone_from(&entry.record.book.languages);
    }
    if entry.record.book.year.is_some() {
        record.book.year = entry.record.book.year;
    }
    for identifier in &entry.record.book.identifiers {
        if !record.book.identifiers.contains(identifier) {
            record.book.identifiers.push(identifier.clone());
        }
    }
    for subject in &entry.record.book.subjects {
        if !record.book.subjects.contains(subject) {
            record.book.subjects.push(subject.clone());
        }
    }
    for contributor in &entry.record.contributors {
        if !record.contributors.iter().any(|existing| existing.name() == contributor.name() && existing.role_code() == contributor.role_code()) {
            record.contributors.push(contributor.clone());
        }
    }
    for publisher in &entry.record.book.publishers {
        if !record.book.publishers.contains(publisher) {
            record.book.publishers.push(publisher.clone());
        }
    }
    if !entry.record.description.trim().is_empty() {
        record.description.clone_from(&entry.record.description);
    }
    Ok((record, inspected.toc))
}

fn validate(entries: &[Entry]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for entry in entries {
        if entry.source.trim().is_empty() || entry.entry_id.trim().is_empty() || entry.record.title.trim().is_empty() || entry.license.trim().is_empty() {
            return Err("link manifest source, entry_id, title and license must be nonempty".into());
        }
        for value in std::iter::once(&entry.entry_url).chain(entry.file_url.iter()) {
            let url = Url::parse(value).map_err(|e| e.to_string())?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                return Err("link manifest URL must use HTTP or HTTPS without credentials".into());
            }
        }
        if !seen.insert((&entry.source, &entry.entry_id)) {
            return Err(format!("duplicate link manifest entry: {} {}", entry.source, entry.entry_id));
        }
    }
    Ok(())
}

fn import_entries(db: &Connection, entries: &[Entry]) -> Result<(), String> {
    validate(entries)?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    // Loaded once for the batch; each entry's authors join it as they arrive.
    let mut authors = curation::AuthorIds::load(&tx)?;
    for entry in entries {
        write_entry(&tx, entry, &mut authors)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    eprintln!("{} external catalog entries imported", entries.len());
    Ok(())
}

/// Writes one entry inside the caller's transaction without fetching its file.
pub fn store_entry(db: &Connection, entry: &Entry) -> Result<String, String> {
    validate(std::slice::from_ref(entry))?;
    write_entry(db, entry, &mut curation::AuthorIds::load(db)?)
}

pub fn entry_key(source: &str, entry_id: &str) -> Result<String, String> {
    // This digest identifies a source's metadata entry, never file bytes.
    let identity = serde_json::to_vec(&("bokheim:external-catalog:v1", source, entry_id)).map_err(|e| e.to_string())?;
    Ok(blake3::hash(&identity).to_hex().to_string())
}

fn write_entry(db: &Connection, entry: &Entry, authors: &mut curation::AuthorIds) -> Result<String, String> {
    let key = entry_key(&entry.source, &entry.entry_id)?;
    let mut record = entry.record.clone();
    if entry.enriched {
        authors.adopt(db, &record)?
    } else {
        authors.assign(&mut record)?
    }
    let json = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO external_book(entry_key,source,entry_id,entry_url,license,title,subtitle,author,record,file_url,toc,format) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,license=excluded.license,title=excluded.title,subtitle=excluded.subtitle,author=excluded.author,record=excluded.record,file_url=excluded.file_url,toc=excluded.toc,format=excluded.format",
        params![key, entry.source, entry.entry_id, entry.entry_url, entry.license, record.title, record.subtitle, curation::author(&record), json, entry.file_url, serde_json::to_string(&entry.toc).map_err(|e| e.to_string())?, entry.format as i32],
    ).map_err(|e| e.to_string())?;
    if let Some(evidence) = &entry.source_evidence {
        db.execute(
            "INSERT INTO source_entry(source,entry_id,entry_url,external_entry_key,metadata) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,external_entry_key=excluded.external_entry_key,metadata=excluded.metadata",
            params![entry.source, entry.entry_id, entry.entry_url, key, evidence.to_string()],
        ).map_err(|e| e.to_string())?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::BookDetails;

    fn entry(url: &str) -> Entry {
        Entry {
            source: "litteraturbanken.se".into(),
            entry_id: "lb-test".into(),
            entry_url: url.into(),
            license: "lb-assv".into(),
            file_url: None,
            format: BookFormat::Epub,
            toc: Vec::new(),
            source_evidence: None,
            record: BookRecord { metadata: BookDetails { title: "Source book".into(), ..Default::default() }, description: String::new() },
            enriched: false,
        }
    }

    #[test]
    fn pdf_links_preserve_website_evidence_on_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let mut source = entry("https://unglue.it/work/74326/");
        source.source = "unglue.it".into();
        source.entry_id = "74326".into();
        source.format = BookFormat::Pdf;
        source.source_evidence = Some(serde_json::json!({"website_metadata": {
            "faves": 47, "downloads": 200, "provider_links": {"librarything": ["https://unglue.it/work/74326/librarything/"]}
        }}));
        import_entries(&db, &[source]).unwrap();
        let (key, format, evidence): (String, i64, String) = db
            .query_row("SELECT external_book.entry_key,format,source_entry.metadata FROM external_book JOIN source_entry ON external_entry_key=external_book.entry_key WHERE source_entry.source='unglue.it'", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(key, entry_key("unglue.it", "74326").unwrap());
        assert_eq!(format, BookFormat::Pdf as i64);
        assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap()["website_metadata"]["faves"], 47);
    }

    #[test]
    fn ephemeral_inspection_retains_navigation_and_source_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let mut source = entry("https://litteraturbanken.se/book/info");
        source.record.book.languages = vec![book_model::LanguageTag::parse("se").unwrap()];
        source.record.book.year = Some(1915);
        let bytes = include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/ocr-image-priority.epub").to_vec();
        let (record, toc) = inspect(&source, "source.epub", bytes).unwrap();
        assert_eq!(record.title, "Source book");
        assert_eq!(record.book.languages, source.record.book.languages);
        assert_eq!(record.book.year, Some(1915));
        assert!(!toc.is_empty());
        source.record = record;
        source.toc = toc.clone();
        import_entries(&db, &[source]).unwrap();
        let stored: String = db.query_row("SELECT toc FROM external_book", [], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Vec<BookTocEntry>>(&stored).unwrap(), toc);
        assert_eq!(std::fs::read_dir(dir.path().join("books")).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(dir.path().join("covers")).unwrap().count(), 0);
    }

    #[test]
    fn links_are_idempotent_and_invalid_batches_leave_no_partial_entries() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = "https://litteraturbanken.se/forfattare/test/info";
        import_entries(&db, &[entry(url)]).unwrap();
        let key: String = db.query_row("SELECT entry_key FROM external_book", [], |row| row.get(0)).unwrap();
        let revised = entry("https://litteraturbanken.se/new/info");
        import_entries(&db, &[revised]).unwrap();
        let updated: (String, String) = db.query_row("SELECT entry_key,entry_url FROM external_book", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(updated, (key, "https://litteraturbanken.se/new/info".into()));
        let mut valid = entry(url);
        valid.entry_id = "another".into();
        assert!(import_entries(&db, &[valid, entry("javascript:alert(1)")]).is_err());
        assert_eq!(db.query_row("SELECT count(*) FROM external_book", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.query_row("SELECT count(*) FROM book", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(std::fs::read_dir(dir.path().join("books")).unwrap().count(), 0);
    }
}
