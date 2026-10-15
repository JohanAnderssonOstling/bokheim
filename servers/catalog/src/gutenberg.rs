//! Gutenberg acquisitions use the EPUB3 advertised by the selected book page.
use crate::{curation, get, manifest::SourceMetadata, store_book};
use book_model::{from_text, unique_book_year, BookDetails, BookRecord, BookSubject, Identifier, Scheme, Scope};
use reqwest::{blocking::Client, Url};
use rusqlite::{params, Connection, OptionalExtension};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn is_gutenberg(url: &Url) -> bool {
    matches!(url.host_str(), Some("gutenberg.org" | "www.gutenberg.org"))
}

/// Recognizes book pages and legacy download links, keeping the same edition.
pub fn book_page(url: &Url) -> Option<Url> {
    if !is_gutenberg(url) || !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut parts = url.path_segments()?.collect::<Vec<_>>();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    let id = match parts.as_slice() {
        [id] => id,
        ["ebooks", file] => file.split('.').next()?,
        ["files", id, ..] => id,
        ["cache", "epub", id, ..] => id,
        ["dirs", rest @ ..] => rest.iter().rev().find(|part| part.bytes().all(|byte| byte.is_ascii_digit()) && !part.is_empty())?,
        _ => return None,
    };
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) || id.parse::<u64>().ok()? == 0 {
        return None;
    }
    Url::parse(&format!("https://www.gutenberg.org/ebooks/{id}")).ok()
}

fn page_epub3(page: &Url, raw: &str) -> Result<Url, String> {
    let html = Html::parse_document(raw);
    let links = Selector::parse("a[href]").expect("constant selector");
    let path = format!("{}.epub3.images", page.path());
    html.select(&links)
        .filter_map(|link| page.join(link.value().attr("href")?).ok())
        .find(|url| is_gutenberg(url) && url.scheme() == "https" && url.path() == path && url.query().is_none() && url.fragment().is_none())
        .ok_or_else(|| format!("{page}: no advertised EPUB3 download; book was not downloaded"))
}

pub struct Acquisition {
    pub epub: Url,
    pub metadata: SourceMetadata,
}

fn text(element: ElementRef<'_>) -> String {
    element.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn description_text(element: ElementRef<'_>) -> String {
    let mut parts = Vec::new();
    for node in element.descendants() {
        let Some(value) = node.value().as_text() else { continue };
        let control =
            node.ancestors().filter_map(ElementRef::wrap).any(|ancestor| matches!(ancestor.value().name(), "label" | "script" | "style") || ancestor.value().classes().any(|class| matches!(class, "readmore" | "showless" | "readmoredots")));
        if !control {
            parts.push(value.text.as_ref());
        }
    }
    parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn page_metadata(raw: &str) -> Result<SourceMetadata, String> {
    let html = Html::parse_document(raw);
    let rows = Selector::parse("table.bibrec tr").expect("constant selector");
    let headings = Selector::parse("th").expect("constant selector");
    let cells = Selector::parse("td").expect("constant selector");
    let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
    // Current pages put the synopsis above the download links, outside
    // table.bibrec. Include the initially hidden continuation and the site's
    // generated-summary attribution, leaving out its expand/collapse controls.
    let summaries = Selector::parse(".summary-text-container").expect("constant selector");
    if let Some(summary) = html.select(&summaries).next() {
        record.description = description_text(summary);
    }
    let mut original_year = None;
    let mut release_year = None;
    for row in html.select(&rows) {
        let (Some(heading), Some(cell)) = (row.select(&headings).next(), row.select(&cells).next()) else {
            continue;
        };
        let label = text(heading).trim_end_matches(':').to_ascii_lowercase();
        let value = text(cell);
        if label == "summary" && record.description.trim().is_empty() {
            // Older cached layouts kept the synopsis in the bibliography.
            record.description = description_text(cell);
        } else if matches!(label.as_str(), "original publication" | "original publication date") {
            original_year = unique_book_year(std::iter::once(value.as_str()));
        } else if label == "release date" {
            // Use the visible cell: some source pages swap their RDF date attributes.
            release_year = unique_book_year(std::iter::once(value.as_str()));
        } else if label == "isbn" || label.starts_with("isbn (") {
            for isbn in from_text(&value) {
                // The page's ISBN (HTML) is a book ISBN, alongside file ISBNs.
                record.book.identifiers.push(Identifier::new(isbn, Scheme::Isbn, Scope::Book).map_err(|error| error.to_string())?);
            }
        } else if label == "loc class" {
            let code = value.split(':').next().unwrap_or_default().trim();
            if !code.is_empty() {
                record.book.subjects.push(BookSubject::new(None, &value, "catalog:gutenberg.org", Some("lcc".into()), Some(code.into())).map_err(|error| error.to_string())?);
            }
        } else if label == "subject" && !value.is_empty() {
            record.book.subjects.push(BookSubject::new(None, &value, "catalog:gutenberg.org", Some("lcsh".into()), None).map_err(|error| error.to_string())?);
        }
    }
    record.book.year = original_year.or(release_year);
    Ok(SourceMetadata::from_record(&record))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptionPage {
    page_url: String,
    page_file: PathBuf,
}

#[derive(Default, Serialize)]
struct DescriptionBackfill {
    updated: usize,
    preserved_existing: usize,
    missing_summary: usize,
    no_catalog_entry: usize,
}

/// Recover descriptions from retained source pages without fetching or
/// re-enriching EPUBs, changing book identities, or replacing curated text.
pub fn backfill_descriptions(db: &Connection, path: &Path) -> Result<(), String> {
    let pages: Vec<DescriptionPage> = serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut parsed = Vec::new();
    // Parse and validate before taking the writer lock. A mismatched cached
    // page cannot leave an earlier part of the batch applied.
    for page in pages {
        let url = Url::parse(&page.page_url).map_err(|error| error.to_string())?;
        if book_page(&url).as_ref() != Some(&url) {
            return Err("description backfill requires canonical Gutenberg book URLs".into());
        }
        let file = parent.join(&page.page_file);
        let raw = fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        page_epub3(&url, &raw)?;
        let metadata = page_metadata(&raw)?;
        let evidence = json!({"source_url": page.page_url, "page_file": file,
            "page_blake3": blake3::hash(raw.as_bytes()).to_hex().to_string(),
            "automatically_generated": metadata.description().contains("(This is an automatically generated summary.)")});
        parsed.push((url, metadata, evidence));
    }
    let mut report = DescriptionBackfill::default();
    let tx = db.unchecked_transaction().map_err(|error| error.to_string())?;
    for (url, metadata, evidence) in parsed {
        let id = url.path().trim_start_matches("/ebooks/");
        let hash: Option<String> = tx
            .query_row("SELECT book_hash FROM source_entry WHERE source='gutenberg.org' AND entry_id=?1 AND entry_url=?2 AND book_hash IS NOT NULL", params![id, url.as_str()], |row| row.get(0))
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(hash) = hash else {
            report.no_catalog_entry += 1;
            continue;
        };
        let record = curation::record(&tx, &hash)?.ok_or("source entry has no book record")?;
        if !record.description.trim().is_empty() {
            report.preserved_existing += 1;
            continue;
        }
        if metadata.description().trim().is_empty() {
            report.missing_summary += 1;
            continue;
        }
        tx.execute("UPDATE book SET record=json_set(record,'$.description',?2) WHERE hash=?1", params![hash, metadata.description()]).map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE source_entry SET metadata=json_set(metadata,'$.source_metadata.description',?2,'$.description_source_url',?3,'$.description_evidence',json(?4)) WHERE source='gutenberg.org' AND entry_id=?1",
            params![id, metadata.description(), url.as_str(), evidence.to_string()],
        )
        .map_err(|error| error.to_string())?;
        report.updated += 1;
    }
    tx.commit().map_err(|error| error.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectPage {
    page_url: String,
    page_file: PathBuf,
    observed_at: String,
    targets: Vec<SubjectTarget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectTarget {
    table: String,
    key: String,
}

/// Append classifications and headings from the exact Gutenberg identifier.
/// Other record fields and source identities remain untouched.
pub fn backfill_subjects(db: &Connection, path: &Path) -> Result<(), String> {
    let pages: Vec<SubjectPage> = serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut parsed = Vec::new();
    let mut seen = BTreeSet::new();
    for page in pages {
        let url = Url::parse(&page.page_url).map_err(|error| error.to_string())?;
        if book_page(&url).as_ref() != Some(&url) {
            return Err("subject backfill requires canonical Gutenberg book URLs".into());
        }
        let file = parent.join(&page.page_file);
        let raw = fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        page_epub3(&url, &raw)?;
        let mut source_record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        page_metadata(&raw)?.apply(&mut source_record);
        let evidence = json!({"source_url":page.page_url,"page_file":file,"page_blake3":blake3::hash(raw.as_bytes()).to_hex().to_string(),"observed_at":page.observed_at,"subjects":source_record.book.subjects});
        for target in &page.targets {
            if !matches!(target.table.as_str(), "book" | "external_book") || !seen.insert((target.table.clone(), target.key.clone())) {
                return Err("subject manifest contains an invalid or duplicate target".into());
            }
        }
        parsed.push((url, source_record.book.subjects.clone(), evidence, page.targets));
    }
    let tx = db.unchecked_transaction().map_err(|error| error.to_string())?;
    let mut outcomes = Vec::new();
    for (url, subjects, evidence, targets) in parsed {
        for target in targets {
            let column = if target.table == "book" { "hash" } else { "entry_key" };
            let raw: Option<String> = tx.query_row(&format!("SELECT record FROM {} WHERE {column}=?1", target.table), [&target.key], |row| row.get(0)).optional().map_err(|error| error.to_string())?;
            let Some(raw) = raw else {
                outcomes.push(json!({"table":target.table,"key":target.key,"state":"removed"}));
                continue;
            };
            let mut record: BookRecord = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
            if !record.book.identifiers.iter().filter_map(|identifier| Url::parse(identifier.value()).ok()).any(|identifier| book_page(&identifier).as_ref() == Some(&url)) {
                return Err(format!("{}:{} no longer identifies {url}", target.table, target.key));
            }
            let additions = subjects.iter().filter(|subject| !record.book.subjects.contains(subject)).cloned().collect::<Vec<_>>();
            record.book.subjects.extend(additions.clone());
            if !additions.is_empty() {
                // Update only this JSON field, preserving all unrelated fields.
                tx.execute(&format!("UPDATE {} SET record=json_set(record,'$.book.subjects',json(?2)) WHERE {column}=?1", target.table), params![target.key, serde_json::to_string(&record.book.subjects).map_err(|error| error.to_string())?])
                    .map_err(|error| error.to_string())?;
            }
            let reference = if target.table == "book" { "book_hash" } else { "external_entry_key" };
            let mut statement = tx.prepare(&format!("SELECT source,entry_id,metadata FROM source_entry WHERE {reference}=?1")).map_err(|error| error.to_string())?;
            let sources = statement
                .query_map([&target.key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            drop(statement);
            if sources.is_empty() {
                return Err(format!("{}:{} has no source provenance", target.table, target.key));
            }
            for (source, id, raw) in sources {
                let mut metadata: Value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
                let object = metadata.as_object_mut().ok_or("source provenance must be a JSON object")?;
                let observations = object.entry("subject_backfills").or_insert_with(|| json!([])).as_array_mut().ok_or("subject backfills must be an array")?;
                if !observations.iter().any(|previous| previous["source_url"] == evidence["source_url"] && previous["page_blake3"] == evidence["page_blake3"]) {
                    observations.push(evidence.clone());
                    tx.execute("UPDATE source_entry SET metadata=?3 WHERE source=?1 AND entry_id=?2", params![source, id, metadata.to_string()]).map_err(|error| error.to_string())?;
                }
            }
            outcomes.push(json!({"table":target.table,"key":target.key,"title":record.title,"state":if additions.is_empty(){"unchanged"}else{"updated"},"added_subjects":additions,"source_url":url.as_str()}));
        }
    }
    tx.commit().map_err(|error| error.to_string())?;
    println!("{}", serde_json::to_string_pretty(&outcomes).map_err(|error| error.to_string())?);
    Ok(())
}

/// Other sources keep their acquisition URLs. Gutenberg links always resolve
/// through the book page, including when an OPDS feed offers an older EPUB.
pub fn resolve_acquisition(http: &Client, url: &Url) -> Result<Acquisition, String> {
    if !is_gutenberg(url) {
        return Ok(Acquisition { epub: url.clone(), metadata: SourceMetadata::default() });
    }
    let page = book_page(url).ok_or_else(|| format!("cannot identify the Gutenberg book page for {url}"))?;
    let raw = String::from_utf8(get(http, &page)?).map_err(|error| format!("{page}: {error}"))?;
    Ok(Acquisition { epub: page_epub3(&page, &raw)?, metadata: page_metadata(&raw)? })
}

pub fn add(dir: &Path, db: &Connection, http: &Client, value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|error| error.to_string())?;
    let page = book_page(&url).filter(|page| url.path().trim_end_matches('/') == page.path()).ok_or("add-gutenberg needs a Gutenberg book-page URL, such as https://www.gutenberg.org/ebooks/1497")?;
    let Acquisition { epub, metadata } = resolve_acquisition(http, &page)?;
    let bytes = get(http, &epub)?;
    let id = page.path().trim_start_matches("/ebooks/");
    let hash = store_book(dir, db, http, &bytes, &format!("pg{id}.epub"), Vec::new(), None, Some(&metadata))?;
    curation::add_source(db, &hash, "gutenberg.org", id, Some(page.as_str()), Some(epub.as_str()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedEntry {
    page_url: String,
    page_file: PathBuf,
    file: PathBuf,
    file_url: String,
    downloads: u64,
    listing_url: String,
    observed_at: String,
    #[serde(default)]
    download_metadata: Value,
    #[serde(default)]
    delivery_url: Option<String>,
}

fn retain_download_observations(previous: Option<&Value>, evidence: &mut Value) {
    let mut observations =
        previous.and_then(|value| value.get("download_observations").or_else(|| value.get("bookshelf_rankings"))).and_then(Value::as_array).map(|values| values.iter().map(download_observation).collect::<Vec<_>>()).unwrap_or_default();
    if observations.is_empty() {
        if let Some(previous) = previous.filter(|value| value["downloads"].is_number() && value["listing_url"].is_string()) {
            observations.push(download_observation(previous));
        }
    }
    let bookshelf = |value: &Value| value["listing_url"].as_str().and_then(|url| Url::parse(url).ok()).map(|url| url.path().to_owned());
    observations.retain(|value| bookshelf(value) != bookshelf(evidence));
    observations.push(download_observation(evidence));
    evidence.as_object_mut().expect("source evidence object").remove("rank");
    evidence.as_object_mut().expect("source evidence object").remove("bookshelf_rankings");
    evidence["download_observations"] = json!(observations);
}

fn download_observation(evidence: &Value) -> Value {
    let metadata = evidence.get("download_metadata").or_else(|| evidence.get("ranking_metadata")).unwrap_or(&Value::Null);
    json!({"downloads":evidence["downloads"], "scope":evidence["scope"], "listing_url":evidence["listing_url"], "observed_at":evidence["observed_at"], "download_metadata":metadata})
}

/// An explicit acquisition list supplies cached source pages and validated
/// EPUB3 files. Download observations stay separate from enriched book facts.
pub fn import(dir: &Path, db: &Connection, http: &Client, path: &Path) -> Result<(), String> {
    let entries: Vec<CachedEntry> = serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for entry in entries {
        let page = Url::parse(&entry.page_url).map_err(|error| error.to_string())?;
        if book_page(&page).as_ref() != Some(&page) || entry.observed_at.trim().is_empty() {
            return Err("Gutenberg acquisition needs a canonical book page and observation date".into());
        }
        let listing = Url::parse(&entry.listing_url).map_err(|error| error.to_string())?;
        if !is_gutenberg(&listing) || !listing.path().starts_with("/ebooks/bookshelf/") {
            return Err("Gutenberg popularity must identify a source bookshelf listing".into());
        }
        let raw = fs::read_to_string(parent.join(&entry.page_file)).map_err(|error| error.to_string())?;
        let epub = page_epub3(&page, &raw)?;
        if epub.as_str() != entry.file_url {
            return Err(format!("{}: acquisition URL differs from the page's advertised EPUB3", entry.page_url));
        }
        let metadata = page_metadata(&raw)?;
        let bytes = fs::read(parent.join(&entry.file)).map_err(|error| error.to_string())?;
        let id = page.path().trim_start_matches("/ebooks/");
        let hash = store_book(dir, db, http, &bytes, &format!("pg{id}.epub"), Vec::new(), None, Some(&metadata))?;
        curation::add_source(db, &hash, "gutenberg.org", id, Some(page.as_str()), Some(epub.as_str()))?;
        let html = Html::parse_document(&raw);
        let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let headings = Selector::parse("th").expect("constant selector");
        let cells = Selector::parse("td").expect("constant selector");
        for row in html.select(&Selector::parse("table.bibrec tr").expect("constant selector")) {
            if let (Some(heading), Some(cell)) = (row.select(&headings).next(), row.select(&cells).next()) {
                fields.entry(text(heading)).or_default().push(text(cell));
            }
        }
        let mut evidence = json!({"downloads":entry.downloads, "scope":"bookshelf", "listing_url":entry.listing_url, "observed_at":entry.observed_at, "download_metadata":entry.download_metadata, "epub_url":epub.as_str(), "delivery_url":entry.delivery_url, "source_metadata":metadata, "page_fields":fields});
        let previous: Option<String> = db.query_row("SELECT metadata FROM source_entry WHERE source='gutenberg.org' AND entry_id=?1", [id], |row| row.get(0)).optional().map_err(|error| error.to_string())?;
        let previous = previous.map(|value| serde_json::from_str::<Value>(&value)).transpose().map_err(|error| error.to_string())?;
        retain_download_observations(previous.as_ref(), &mut evidence);
        // A repeated source observation must retain a curator's chosen cover
        // for this same file. Merge at the write boundary so a concurrent
        // retain-covers operation is preserved too.
        db.execute("INSERT INTO source_entry(source,entry_id,entry_url,book_hash,metadata) VALUES('gutenberg.org',?1,?2,?3,?4) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,book_hash=excluded.book_hash,metadata=CASE WHEN source_entry.book_hash=excluded.book_hash AND json_type(source_entry.metadata,'$.cover')='object' THEN json_set(excluded.metadata,'$.cover',json_extract(source_entry.metadata,'$.cover')) ELSE excluded.metadata END", params![id, page.as_str(), hash, evidence.to_string()]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_books_retain_download_counts_and_discard_legacy_positions() {
        let legacy = json!({"bookshelf_rankings":[{"rank":8,"downloads":28375,"scope":"bookshelf","listing_url":"https://www.gutenberg.org/ebooks/bookshelf/691?sort_order=downloads","observed_at":"2026-10-12"}]});
        let mut politics = json!({"downloads":28375,"scope":"bookshelf","listing_url":"https://www.gutenberg.org/ebooks/bookshelf/694?sort_order=downloads","observed_at":"2026-10-12"});
        retain_download_observations(Some(&legacy), &mut politics);
        assert_eq!(politics["download_observations"].as_array().unwrap().len(), 2);
        assert_eq!(politics["download_observations"][0]["downloads"], 28375);
        assert!(politics["download_observations"][0].get("rank").is_none());
        let mut refreshed = json!({"downloads":30000,"scope":"bookshelf","listing_url":"https://www.gutenberg.org/ebooks/bookshelf/694","observed_at":"2026-10-13"});
        retain_download_observations(Some(&politics), &mut refreshed);
        assert_eq!(refreshed["download_observations"].as_array().unwrap().len(), 2);
        assert_eq!(refreshed["download_observations"][0]["downloads"], 28375);
        assert_eq!(refreshed["download_observations"][1]["downloads"], 30000);
        assert!(refreshed.get("rank").is_none());
        assert!(refreshed.get("bookshelf_rankings").is_none());
    }

    const BIBREC: &str = r#"<table class="bibrec">
        <tr property="dcterms:subject" datatype="dcterms:LCC" content="JC"><th>LoC Class</th><td><a>JC: Political science: Political theory</a></td></tr>
        <tr property="dcterms:subject" datatype="dcterms:LCC" content="PA"><th>LoC Class</th><td><a>PA: Language and Literatures: Classical Languages and Literature</a></td></tr>
        <tr><th>Subject</th><td property="dcterms:subject" datatype="dcterms:LCSH"><a>Justice -- Early works to 1800</a></td></tr>
        <tr><th>ISBN (HTML)</th><td>979-8-238-01497-5</td></tr>
        <tr><th>ISBN</th><td>978-0-393-24327-7</td></tr>
        <tr><th>ISBN (HTML)</th><td>9798238014975</td></tr>
        <tr><th>ISBN</th><td>0000000000000</td></tr>
        </table><aside>Other book ISBN 978-0-8213-3827-8</aside>"#;

    #[test]
    fn summaries_include_hidden_continuations_and_exclude_page_controls() {
        let raw = r#"<meta name="description" content="Free eBook digitized by volunteers.">
            <div class="summary-text-container"><span class="readmore-container">
            A study of justice &amp; government. <span><input type="checkbox">
            <span class="toggle-content">The final sentence. (This is an automatically generated summary.)</span>
            <label><span class="readmoredots">...</span><span class="readmore">Read more</span><span class="showless">Show less</span></label>
            </span></span></div>"#;
        assert_eq!(page_metadata(raw).unwrap().description(), "A study of justice & government. The final sentence. (This is an automatically generated summary.)");
        assert_eq!(page_metadata("<table class='bibrec'><tr><th>Summary</th><td>Older page summary.<label>Read more</label></td></tr></table>").unwrap().description(), "Older page summary.");
        assert!(page_metadata("<meta name='description' content='Free eBook'><table class='bibrec'><tr><th>Source Description</th><td>with 8 diagrams</td></tr></table>").unwrap().description().is_empty());
    }

    #[test]
    fn cached_description_backfill_preserves_curated_text_and_other_source_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let hash = "cached-book";
        let record = BookRecord { metadata: BookDetails { title: "Cached book".into(), ..Default::default() }, description: String::new() };
        curation::set_record(&db, hash, &record).unwrap();
        curation::add_source(&db, hash, "gutenberg.org", "1", Some("https://www.gutenberg.org/ebooks/1"), None).unwrap();
        db.execute(
            "INSERT INTO source_entry(source,entry_id,entry_url,book_hash,metadata) VALUES('gutenberg.org','1','https://www.gutenberg.org/ebooks/1',?1,?2)",
            params![hash, r#"{"downloads":123,"cover":{"source_url":"https://example.org/chosen.jpg"}}"#],
        )
        .unwrap();
        let page = dir.path().join("page.html");
        fs::write(&page, "<a href='/ebooks/1.epub3.images'>EPUB3</a><div class='summary-text-container'>Saved source summary.</div>").unwrap();
        let manifest = dir.path().join("descriptions.json");
        fs::write(&manifest, json!([{"page_url":"https://www.gutenberg.org/ebooks/1","page_file":"page.html"}]).to_string()).unwrap();
        backfill_descriptions(&db, &manifest).unwrap();
        let filled = curation::record(&db, hash).unwrap().unwrap();
        assert_eq!(filled.description, "Saved source summary.");
        assert_eq!(filled.metadata, record.metadata);
        let evidence: String = db.query_row("SELECT metadata FROM source_entry", [], |row| row.get(0)).unwrap();
        let evidence: Value = serde_json::from_str(&evidence).unwrap();
        assert_eq!(evidence["downloads"], 123);
        assert_eq!(evidence["cover"]["source_url"], "https://example.org/chosen.jpg");
        assert_eq!(evidence["source_metadata"]["description"], filled.description);
        assert_eq!(evidence["description_source_url"], "https://www.gutenberg.org/ebooks/1");
        assert_eq!(evidence["description_evidence"]["page_blake3"], blake3::hash(&fs::read(&page).unwrap()).to_hex().as_str());
        let mut curated = filled;
        curated.description = "Manually reviewed description".into();
        curation::set_record(&db, hash, &curated).unwrap();
        backfill_descriptions(&db, &manifest).unwrap();
        assert_eq!(curation::record(&db, hash).unwrap().unwrap(), curated);
        // Validate the complete batch before writing any missing description.
        curated.description.clear();
        curation::set_record(&db, hash, &curated).unwrap();
        fs::write(dir.path().join("wrong.html"), "<a href='/ebooks/2.epub3.images'>EPUB3</a><div class='summary-text-container'>Wrong book.</div>").unwrap();
        fs::write(
            &manifest,
            json!([
                {"page_url":"https://www.gutenberg.org/ebooks/1","page_file":"page.html"},
                {"page_url":"https://www.gutenberg.org/ebooks/1","page_file":"wrong.html"}
            ])
            .to_string(),
        )
        .unwrap();
        assert!(backfill_descriptions(&db, &manifest).is_err());
        assert!(curation::record(&db, hash).unwrap().unwrap().description.is_empty());
    }

    #[test]
    fn original_publication_year_ignores_publisher_text_and_precedes_release_year() {
        for (original, expected) in [("United States: W. W. Norton &amp; Company, Inc., 1927.", Some(1927)), ("London: Publisher, [1890?].", Some(1890)), ("No date supplied", Some(2024)), ("", Some(2024))] {
            let raw = format!("<table class='bibrec'><tr><th>Release Date</th><td>Feb 18, 2024</td></tr><tr><th>Original Publication</th><td>{original}</td></tr><tr><th>Last Update</th><td>2026</td></tr></table>");
            let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
            page_metadata(&raw).unwrap().apply(&mut record);
            assert_eq!(record.book.year, expected);
        }
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        page_metadata("<table class='bibrec'><tr><th>Last Update</th><td>2026</td></tr></table>").unwrap().apply(&mut record);
        assert!(record.book.year.is_none());
    }

    #[test]
    fn cached_acquisitions_keep_page_metadata_and_popularity_but_reject_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let bytes = include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        let hash = content_address::hash_reader(&mut &bytes[..]).unwrap().to_hex();
        let mut record = BookRecord { metadata: BookDetails::default(), description: "Curated description".into() };
        record.title = "The Republic".into();
        curation::set_record(&db, &hash, &record).unwrap();
        fs::write(dir.path().join("book.epub"), bytes).unwrap();
        let page = format!("<a href='/ebooks/1497.epub3.images'>EPUB3</a><div class='summary-text-container'>Source summary.</div>{}", include_str!("../tests/fixtures/gutenberg-republic-bibrec.html"));
        fs::write(dir.path().join("page.html"), page).unwrap();
        let mut entry = json!({"page_url":"https://www.gutenberg.org/ebooks/1497", "page_file":"page.html", "file":"book.epub", "file_url":"https://www.gutenberg.org/ebooks/1497.epub.images", "downloads":28375, "listing_url":"https://www.gutenberg.org/ebooks/bookshelf/691?sort_order=downloads", "observed_at":"2026-10-12T12:00:00Z"});
        let path = dir.path().join("manifest.json");
        fs::write(&path, json!([entry.clone()]).to_string()).unwrap();
        let http = Client::new();
        assert!(import(dir.path(), &db, &http, &path).is_err());
        assert_eq!(db.query_row("SELECT count(*) FROM source_entry", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        entry["file_url"] = json!("https://www.gutenberg.org/ebooks/1497.epub3.images");
        entry["delivery_url"] = json!("https://mirror.cs.odu.edu/gutenberg-epub/1497/pg1497-images-3.epub");
        fs::write(&path, json!([entry]).to_string()).unwrap();
        import(dir.path(), &db, &http, &path).unwrap();
        let chosen_cover = json!({"source_url":"https://books.google.com/books/content?id=reviewed&img=1", "image_blake3":"retained-artwork"});
        db.execute("UPDATE source_entry SET metadata=json_set(metadata,'$.cover',json(?1)) WHERE source='gutenberg.org' AND entry_id='1497'", [chosen_cover.to_string()]).unwrap();
        import(dir.path(), &db, &http, &path).unwrap();
        let stored = curation::record(&db, &hash).unwrap().unwrap();
        assert_eq!(stored.book.year, Some(1998));
        assert_eq!(stored.book.identifiers[0].value(), "9798238014975");
        assert_eq!(stored.book.subjects.len(), 6);
        assert_eq!(stored.description, record.description);
        // Re-importing a previously stored EPUB also fills a missing synopsis.
        let mut empty = stored;
        empty.description.clear();
        curation::set_record(&db, &hash, &empty).unwrap();
        import(dir.path(), &db, &http, &path).unwrap();
        assert_eq!(curation::record(&db, &hash).unwrap().unwrap().description, "Source summary.");
        let raw: String = db.query_row("SELECT metadata FROM source_entry WHERE entry_id='1497'", [], |row| row.get(0)).unwrap();
        let evidence: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(evidence.get("rank").is_none());
        assert_eq!(evidence["downloads"], 28375);
        assert_eq!(evidence["epub_url"], "https://www.gutenberg.org/ebooks/1497.epub3.images");
        assert_eq!(evidence["delivery_url"], "https://mirror.cs.odu.edu/gutenberg-epub/1497/pg1497-images-3.epub");
        assert_eq!(evidence["page_fields"]["Release Date"][0], "Oct 1, 1998");
        assert_eq!(evidence["cover"], chosen_cover);
        // A new acquisition must not inherit artwork evidence for an old hash.
        db.execute("UPDATE source_entry SET book_hash=NULL WHERE source='gutenberg.org' AND entry_id='1497'", []).unwrap();
        import(dir.path(), &db, &http, &path).unwrap();
        let raw: String = db.query_row("SELECT metadata FROM source_entry WHERE entry_id='1497'", [], |row| row.get(0)).unwrap();
        assert!(serde_json::from_str::<Value>(&raw).unwrap().get("cover").is_none());
    }

    #[test]
    fn actual_gutenberg_bibliographic_table_retains_isbn_loc_and_subject_headings() {
        let metadata = page_metadata(include_str!("../tests/fixtures/gutenberg-republic-bibrec.html")).unwrap();
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        metadata.apply(&mut record);
        assert_eq!(record.book.year, Some(1998));
        assert_eq!(record.book.identifiers[0].value(), "9798238014975");
        assert_eq!(record.book.identifiers[0].scheme(), &Scheme::Isbn);
        assert_eq!(record.book.identifiers[0].scope(), Scope::Book);
        let codes = record.book.subjects.iter().filter_map(BookSubject::code).collect::<Vec<_>>();
        assert_eq!(codes, ["JC", "PA"]);
        let headings = record.book.subjects.iter().filter(|subject| subject.authority() == Some("lcsh")).map(BookSubject::name).collect::<Vec<_>>();
        assert_eq!(headings, ["Classical literature", "Political science -- Early works to 1800", "Utopias -- Early works to 1800", "Justice -- Early works to 1800"]);
    }

    #[test]
    fn page_isbns_are_book_identifiers_and_original_subjects_keep_their_codes() {
        let metadata = page_metadata(BIBREC).unwrap();
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        record.book.identifiers.push(Identifier::new("9780821338278", Scheme::Isbn, Scope::Book).unwrap());
        metadata.apply(&mut record);
        metadata.apply(&mut record);
        assert_eq!(record.book.identifiers.iter().map(Identifier::value).collect::<Vec<_>>(), ["9780821338278", "9798238014975", "9780393243277"]);
        assert!(record.book.identifiers.iter().all(|id| id.scheme() == &Scheme::Isbn && id.scope() == Scope::Book));
        assert_eq!(record.book.subjects.len(), 3);
        assert_eq!(record.book.subjects[0].code(), Some("JC"));
        assert_eq!(record.book.subjects[1].code(), Some("PA"));
        assert_eq!(record.book.subjects[2].name(), "Justice -- Early works to 1800");
        assert_eq!(record.book.subjects[2].authority(), Some("lcsh"));
        assert_eq!(record.book.subjects[2].source(), "catalog:gutenberg.org");
        let json = serde_json::to_vec(&record).unwrap();
        assert_eq!(serde_json::from_slice::<BookRecord>(&json).unwrap(), record);
    }

    #[test]
    fn repeat_import_adds_page_isbns_without_overwriting_curated_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let bytes = include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        let hash = content_address::hash_reader(&mut &bytes[..]).unwrap().to_hex();
        let mut record = BookRecord { metadata: BookDetails::default(), description: "Curated description".into() };
        record.title = "Curated title".into();
        record.book.year = Some(1915);
        record.book.identifiers.push(Identifier::new("978-0-393-24327-7", Scheme::Isbn, Scope::Edition).unwrap());
        record.book.identifiers.push(Identifier::new("source-id", Scheme::Unspecified, Scope::Book).unwrap());
        record.book.subjects.push(BookSubject::new(None, "Curated subject", "manual:subject", None, None).unwrap());
        curation::set_record(&db, &hash, &record).unwrap();
        let metadata = page_metadata(BIBREC).unwrap();
        let http = Client::new();
        for _ in 0..2 {
            assert_eq!(store_book(dir.path(), &db, &http, bytes, "book.epub", Vec::new(), None, Some(&metadata)).unwrap(), hash);
        }
        let stored = curation::record(&db, &hash).unwrap().unwrap();
        assert_eq!(stored.title, record.title);
        assert_eq!(stored.description, record.description);
        assert_eq!(stored.book.year, record.book.year);
        assert_eq!(stored.book.identifiers.len(), 3);
        assert!(stored.book.identifiers.starts_with(&record.book.identifiers));
        assert_eq!(stored.book.identifiers[2].value(), "9798238014975");
        assert_eq!(stored.book.subjects.len(), 4);
        assert_eq!(stored.book.subjects[0], record.book.subjects[0]);
    }

    #[test]
    fn selects_advertised_epub3_even_when_older_formats_come_first() {
        let page = Url::parse("https://www.gutenberg.org/ebooks/1497").unwrap();
        let raw = r#"<a href="/ebooks/1497.epub.noimages">EPUB</a>
            <a href="/ebooks/1497.kf8.images">Kindle</a>
            <a href="/ebooks/1497.epub3.images">EPUB3 ★ Recommended</a>"#;
        assert_eq!(page_epub3(&page, raw).unwrap().as_str(), "https://www.gutenberg.org/ebooks/1497.epub3.images");
    }

    #[test]
    fn never_substitutes_an_older_format_another_book_or_another_host() {
        let page = Url::parse("https://www.gutenberg.org/ebooks/1497").unwrap();
        for href in ["/ebooks/1497.epub.images", "/ebooks/55201.epub3.images", "https://foreign.test/ebooks/1497.epub3.images", "http://www.gutenberg.org/ebooks/1497.epub3.images"] {
            assert!(page_epub3(&page, &format!("<a href='{href}'>EPUB3</a>")).is_err());
        }
        assert!(page_epub3(&page, "<h1>Gateway error</h1>").is_err());
    }

    #[test]
    fn legacy_links_resolve_to_their_own_book_page() {
        for value in [
            "https://www.gutenberg.org/ebooks/5921",
            "https://www.gutenberg.org/ebooks/5921/",
            "https://www.gutenberg.org/ebooks/5921.epub.noimages",
            "http://www.gutenberg.org/cache/epub/5921/pg5921-images.epub",
            "http://www.gutenberg.org/dirs/5/9/2/5921/5921-h/5921-h.htm",
        ] {
            assert_eq!(book_page(&Url::parse(value).unwrap()).unwrap().as_str(), "https://www.gutenberg.org/ebooks/5921");
        }
        for value in ["https://foreign.test/ebooks/5921", "https://www.gutenberg.org/ebooks/", "https://www.gutenberg.org/ebooks/0", "https://www.gutenberg.org/ebooks/search"] {
            assert!(book_page(&Url::parse(value).unwrap()).is_none());
        }
    }
}
