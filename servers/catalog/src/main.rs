//! Curates public-domain books into a catalog snapshot from named entries,
//! inbox files, or explicitly invoked source crawlers. Adding a book inspects and
//! enriches it, so every book in the database is in the catalog. See
//! docs/public-domain-catalog-plan.md.
mod classification;
mod covers;
mod curation;
mod gutenberg;
mod links;
mod loyalbooks;
mod manifest;
mod opds;
mod popularity;
mod publish;
mod timing;

use book_model::{BookFormat, BookRecord, BookSubject};
use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentResponse, BookEnrichmentStatus, BookFacts};
use reqwest::blocking::Client;
use reqwest::Url;
use rusqlite::Connection;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: catalog DIR search FEED_URL
       catalog DIR subject-audit
       catalog DIR resolve-subject-codes
       catalog DIR recover-source-headings
       catalog DIR add FEED_URL [ENTRY_ID]
       catalog DIR add-gutenberg BOOK_PAGE_URL
       catalog DIR import-gutenberg MANIFEST_JSON
       catalog DIR backfill-gutenberg-descriptions MANIFEST_JSON
       catalog DIR backfill-gutenberg-subjects MANIFEST_JSON
       catalog DIR add-loyalbooks BOOK_URL [POPULARITY_PAGE_URL]
       catalog DIR add-loyalbooks BOOK_URL --epub-url EPUB_URL
       catalog DIR crawl-loyalbooks [--retry-failures]
       catalog DIR loyalbooks-status
       catalog DIR import
       catalog DIR import-manifest MANIFEST_JSON
       catalog DIR link-manifest MANIFEST_JSON
       catalog DIR analyze-link-manifest MANIFEST_JSON
       catalog DIR remove-link SOURCE ENTRY_ID
       catalog DIR remove HASH
       catalog DIR litteraturbanken-popularity INDEX_JSON OBSERVED_AT
       catalog DIR review
       catalog DIR subject|unsubject HASH lcc|ddc|bisac|unified CODE
       catalog DIR cover HASH IMAGE_URL
       catalog DIR retain-covers MANIFEST_JSON
       catalog DIR extract-standardebooks-covers OUT_DIR
       catalog DIR publish OUT_DIR RELEASE_ID
       catalog compact-snapshot INPUT OUTPUT
       catalog compress-snapshot INPUT

DIR holds curation.sqlite3 and the books and covers. HASH may be a prefix.
import adds the EPUBs dropped in DIR/inbox/SOURCE/, SOURCE naming where they
came from (for example DIR/inbox/standardebooks.org/).
import-manifest reads an explicit JSON acquisition list, with file paths relative
to the manifest and source metadata applied before enrichment.
add-gutenberg downloads only the EPUB3 advertised on the selected book page.
Gutenberg acquisitions through add also use the book page's EPUB3 link.
backfill-gutenberg-descriptions reads cached page_url/page_file pairs and fills
missing summaries without downloading books or replacing existing descriptions.
backfill-gutenberg-subjects appends subjects from cached source pages to explicit
book/external_book targets verified against their Gutenberg identifiers.
add-loyalbooks fetches a free, rated book directly from its source page. EPUBs
are retained; audio rated 4+ is linked without downloading audio. An optional
Top_100 page supplies popularity rank. Source metadata goes directly into SQLite.
crawl-loyalbooks scans every sitemap book across all languages, respects the
source crawl delay, resumes its SQLite queue and installs local snapshots as it
progresses. --retry-failures requeues exhausted failures. loyalbooks-status
prints queue progress without fetching source pages.
litteraturbanken-popularity retains reading and EPUB popularity from an observed
index response and combines their tied-rank percentiles with equal weight.
retain-covers downloads images, or reads image paths supplied by a manifest,
and retains card/detail versions for hosted or source-linked catalog entries.
link-manifest stores bibliographic records and source-page links without fetching
books or covers. analyze-link-manifest inspects ephemeral in-memory downloads
and enriches metadata, then discards the EPUB bytes. remove-link removes one such entry.add and import ask BOKHEIM_METADATA_URL (default https://meta.bokheim.se/).
publish writes the tree the catalog origin serves (releases/N/catalog.sqlite3,
books/, covers/) and prints the manifest's catalog entry; the URL uses
BOKHEIM_CATALOG_ORIGIN (default https://catalogs.bokheim.se).";

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if let [command, input] = args {
        if command == "compress-snapshot" {
            return publish::compress_snapshot(Path::new(input));
        }
    }
    if let [command, input, output] = args {
        if command == "compact-snapshot" {
            return publish::compact_snapshot(Path::new(input), Path::new(output));
        }
    }
    if let [dir, command, output] = args {
        if command == "extract-standardebooks-covers" {
            return covers::extract_standardebooks(Path::new(dir), Path::new(output));
        }
    }
    let [dir, command, rest @ ..] = args else { return Err(USAGE.into()) };
    let dir = PathBuf::from(dir);
    let db = curation::open(&dir)?;
    let http = Client::builder().user_agent("bokheim-catalog").timeout(std::time::Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
    match (command.as_str(), rest) {
        ("subject-audit", []) => classification::audit(&db),
        ("resolve-subject-codes", []) => classification::resolve_codes(),
        ("recover-source-headings", []) => classification::recover_headings(),
        ("search", [feed]) => search(&http, feed),
        ("retain-covers", [manifest]) => covers::retain(&dir, &db, &http, Path::new(manifest)),
        ("add", [feed]) => add(&dir, &db, &http, feed, None),
        ("add", [feed, entry]) => add(&dir, &db, &http, feed, Some(entry)),
        ("add-gutenberg", [page]) => gutenberg::add(&dir, &db, &http, page),
        ("import-gutenberg", [path]) => gutenberg::import(&dir, &db, &http, Path::new(path)),
        ("backfill-gutenberg-descriptions", [path]) => gutenberg::backfill_descriptions(&db, Path::new(path)),
        ("backfill-gutenberg-subjects", [path]) => gutenberg::backfill_subjects(&db, Path::new(path)),
        ("add-loyalbooks", [page]) => loyalbooks::add(&dir, &db, &http, page, None),
        ("add-loyalbooks", [page, popularity]) => loyalbooks::add(&dir, &db, &http, page, Some(popularity)),
        ("add-loyalbooks", [page, flag, epub]) if flag == "--epub-url" => loyalbooks::add_with_epub(&dir, &db, &http, page, None, Some(epub)),
        ("crawl-loyalbooks", []) => loyalbooks::crawl::run(&dir, &db, &http, false),
        ("crawl-loyalbooks", [flag]) if flag == "--retry-failures" => loyalbooks::crawl::run(&dir, &db, &http, true),
        ("loyalbooks-status", []) => loyalbooks::crawl::status(&db),
        ("litteraturbanken-popularity", [index, observed_at]) => popularity::import(&db, Path::new(index), observed_at),
        ("import", []) => import(&dir, &db, &http),
        ("import-manifest", [path]) => manifest::import(&dir, &db, &http, Path::new(path)),
        ("link-manifest", [path]) => links::import(&db, &http, Path::new(path), false),
        ("analyze-link-manifest", [path]) => links::import(&db, &http, Path::new(path), true),
        ("remove-link", [source, entry]) => db.execute("DELETE FROM external_book WHERE source=?1 AND entry_id=?2", rusqlite::params![source, entry]).map(|_| ()).map_err(|e| e.to_string()),
        ("remove", [hash]) => {
            let hash = curation::resolve(&db, hash)?;
            curation::remove(&db, &hash)?;
            for file in [format!("books/{hash}.epub"), format!("covers/{hash}-card.jpg"), format!("covers/{hash}-detail.jpg")] {
                let _ = std::fs::remove_file(dir.join(file));
            }
            Ok(())
        }
        ("review", []) => review(&dir, &db),
        ("subject", [hash, system, code]) if matches!(system.as_str(), "lcc" | "ddc" | "bisac" | "unified") => edit_subjects(&db, hash, |record| {
            // A manual assignment, as a library user's would be.
            let subject = BookSubject::new(None, code, "manual:subject", Some(system.clone()), Some(code.clone())).map_err(|e| e.to_string())?;
            record.book.subjects.push(subject);
            Ok(())
        }),
        ("unsubject", [hash, system, code]) => edit_subjects(&db, hash, |record| {
            record.book.subjects.retain(|subject| !(subject.authority() == Some(system.as_str()) && subject.code() == Some(code.as_str())));
            Ok(())
        }),
        ("cover", [hash, url]) => {
            let hash = curation::resolve(&db, hash)?;
            cover_from_image(&dir, &hash, &get(&http, &Url::parse(url).map_err(|e| e.to_string())?)?)
        }
        ("publish", [out, release]) => publish::publish(&dir, &db, Path::new(out), release.parse().map_err(|_| "RELEASE_ID must be a positive integer")?),
        _ => Err(USAGE.into()),
    }
}

fn get(http: &Client, url: &Url) -> Result<Vec<u8>, String> {
    let _timing = timing::measure("http_get", url.as_str());
    {
        let _wait = timing::measure("request_wait", url.as_str());
        loyalbooks::wait_for_request(url);
    }
    let response = http.get(url.clone()).send().and_then(|r| r.error_for_status()).map_err(|e| format!("{url}: {e}"))?;
    Ok(response.bytes().map_err(|e| format!("{url}: {e}"))?.to_vec())
}

fn feed_entries(http: &Client, feed: &str) -> Result<(Url, Vec<opds::Entry>), String> {
    let base = Url::parse(feed).map_err(|e| e.to_string())?;
    let xml = String::from_utf8(get(http, &base)?).map_err(|e| e.to_string())?;
    let entries = opds::entries(&xml, &base)?;
    Ok((base, entries))
}

fn listing(entries: &[opds::Entry]) -> String {
    entries.iter().map(|entry| format!("{}  {} — {}{}", entry.id, entry.title, entry.authors.join(", "), if entry.epub.is_some() { "" } else { "  (no EPUB)" })).collect::<Vec<_>>().join("\n")
}

fn search(http: &Client, feed: &str) -> Result<(), String> {
    let (_, entries) = feed_entries(http, feed)?;
    println!("{}", listing(&entries));
    Ok(())
}

fn add(dir: &Path, db: &Connection, http: &Client, feed: &str, entry_id: Option<&str>) -> Result<(), String> {
    let (base, entries) = feed_entries(http, feed)?;
    let entry = match entry_id {
        Some(id) => entries.iter().find(|entry| entry.id == id),
        None if entries.len() == 1 => entries.first(),
        None => None,
    };
    let entry = entry.ok_or_else(|| format!("name one of the entries:\n{}", listing(&entries)))?;
    let gutenberg::Acquisition { epub, metadata } = gutenberg::resolve_acquisition(http, entry.epub.as_ref().ok_or("the entry offers no EPUB")?)?;
    let bytes = get(http, &epub)?;
    let file_name = epub.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or("book.epub").to_owned();
    let source = base.host_str().unwrap_or_default();
    // The source's own classification is evidence for enrichment.
    let evidence = format!("catalog:{source}");
    let lcsh = entry.lcsh.iter().filter_map(|name| BookSubject::new(None, name, &evidence, Some("lcsh".into()), None).ok());
    let lcc = entry.lcc.iter().filter_map(|code| BookSubject::new(None, code, &evidence, Some("lcc".into()), Some(code.clone())).ok());
    let cover_policy = if source == "standardebooks.org" { CoverPolicy::Embedded } else { CoverPolicy::Automatic };
    let hash = store_book_with_cover_policy(dir, db, http, &bytes, &file_name, lcsh.chain(lcc).collect(), entry.image.as_ref(), Some(&metadata), cover_policy)?;
    curation::add_source(db, &hash, source, &entry.id, Some(base.as_str()), Some(epub.as_str()))
}

/// Adds every EPUB dropped in DIR/inbox/SOURCE/, recording SOURCE as where it
/// came from, and takes it out of the inbox. A book that fails stays there.
fn import(dir: &Path, db: &Connection, http: &Client) -> Result<(), String> {
    let mut files = Vec::new();
    for source in std::fs::read_dir(dir.join("inbox")).map_err(|e| e.to_string())? {
        let source = source.map_err(|e| e.to_string())?.path();
        if source.is_dir() {
            for file in std::fs::read_dir(&source).map_err(|e| e.to_string())? {
                let file = file.map_err(|e| e.to_string())?.path();
                if file.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("epub")) {
                    files.push(file);
                }
            }
        }
    }
    files.sort();
    for file in files {
        let source = file.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let added = std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|bytes| if source == "standardebooks.org" { store_book_with_embedded_cover(dir, db, http, &bytes, &name, None) } else { store_book(dir, db, http, &bytes, &name, Vec::new(), None, None) })
            .and_then(|hash| curation::add_source(db, &hash, &source, &name, None, None));
        match added {
            Ok(()) => std::fs::remove_file(&file).map_err(|e| e.to_string())?,
            Err(error) => eprintln!("{}: {error}", file.display()),
        }
    }
    Ok(())
}

/// Inspects, enriches and stores one EPUB unless the catalog already has it,
/// and returns its hash. `evidence` is the source's own classification;
/// `image` its cover, used when the file has none.
fn store_book(dir: &Path, db: &Connection, http: &Client, bytes: &[u8], file_name: &str, evidence: Vec<BookSubject>, image: Option<&Url>, metadata: Option<&manifest::SourceMetadata>) -> Result<String, String> {
    store_book_with_cover_policy(dir, db, http, bytes, file_name, evidence, image, metadata, CoverPolicy::Automatic)
}

fn store_book_preserving_covers(dir: &Path, db: &Connection, http: &Client, bytes: &[u8], file_name: &str, metadata: Option<&manifest::SourceMetadata>) -> Result<String, String> {
    store_book_with_cover_policy(dir, db, http, bytes, file_name, Vec::new(), None, metadata, CoverPolicy::Preserve)
}

fn store_book_with_embedded_cover(dir: &Path, db: &Connection, http: &Client, bytes: &[u8], file_name: &str, metadata: Option<&manifest::SourceMetadata>) -> Result<String, String> {
    store_book_with_cover_policy(dir, db, http, bytes, file_name, Vec::new(), None, metadata, CoverPolicy::Embedded)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoverPolicy {
    Automatic,
    Preserve,
    Embedded,
}

fn store_book_with_cover_policy(
    dir: &Path, db: &Connection, http: &Client, bytes: &[u8], file_name: &str, evidence: Vec<BookSubject>, image: Option<&Url>, metadata: Option<&manifest::SourceMetadata>, cover_policy: CoverPolicy,
) -> Result<String, String> {
    let _timing = timing::measure("epub_total", file_name);
    let content_hash = content_address::hash_reader(&mut &bytes[..]).map_err(|e| e.to_string())?;
    let hash = content_hash.to_hex();
    // A source-owned cover must also replace artwork copied by an older import.
    // Do this before the existing-record shortcut, without requesting cover enrichment.
    if cover_policy == CoverPolicy::Embedded && !cover_from_book(dir, &hash, bytes)? {
        return Err(format!("{file_name}: EPUB has no embedded cover"));
    }
    if let Some(mut record) = curation::record(db, &hash)? {
        if let Some(metadata) = metadata {
            let previous = record.clone();
            metadata.retain_evidence(&mut record);
            if record != previous {
                curation::set_record(db, &hash, &record)?;
            }
        }
        if cover_policy != CoverPolicy::Preserve {
            println!("{hash}  already in the catalog");
            return Ok(hash);
        }
    }
    let inspected = {
        let _timing = timing::measure("epub_inspect", file_name);
        book_inspection::inspect_book_reader_with_hash(Path::new(file_name), BookFormat::Epub, Cursor::new(bytes.to_vec()), Some(content_hash)).map_err(|e| e.to_string())?
    };
    let mut record = inspected.metadata;
    if let Some(metadata) = metadata {
        metadata.apply(&mut record);
    }
    for subject in evidence {
        if !record.book.subjects.iter().any(|existing| existing.name() == subject.name() && existing.authority() == subject.authority()) {
            record.book.subjects.push(subject);
        }
    }
    let covers_timing = timing::measure("epub_covers", file_name);
    let automatic_cover = cover_policy == CoverPolicy::Automatic;
    let local_cover = automatic_cover && (has_cover(dir, &hash) || cover_from_book(dir, &hash, bytes)? || source_cover(dir, &hash, http, image));
    drop(covers_timing);
    let original_record = record.clone();
    let (record, cover_id, found) = enrich(http, record, file_name, automatic_cover && !local_cover)?;
    if cover_policy == CoverPolicy::Preserve {
        preserve_matching_cover(dir, db, &hash, &record, &original_record)?;
        if !has_cover(dir, &hash) {
            cover_from_book(dir, &hash, bytes)?;
        }
    }
    if automatic_cover && !local_cover {
        let _timing = timing::measure("epub_cover_fallback", file_name);
        if let Some(image) = cover_id.map(|id| metadata_url()?.join(&format!("v3/covers/{id}")).map_err(|e| e.to_string())).transpose()? {
            if let Err(error) = get(http, &image).and_then(|bytes| cover_from_image(dir, &hash, &bytes)) {
                eprintln!("optional cover unavailable for {file_name}: {error}");
            }
        }
    }
    let _store = timing::measure("epub_store", file_name);
    std::fs::write(dir.join("books").join(format!("{hash}.epub")), bytes).map_err(|e| e.to_string())?;
    // The server's author identities win over the ones the catalog has.
    if found {
        curation::set_enriched_record(db, &hash, &record)?
    } else {
        curation::set_record(db, &hash, &record)?
    }
    curation::set_toc(db, &hash, &inspected.toc)?;
    println!("{hash}  {}", record.title);
    Ok(hash)
}

/// Reuse the catalog's chosen artwork when the EPUB identity changes. Existing
/// image files are immutable; only missing images for the new hash are copied.
fn preserve_matching_cover(dir: &Path, db: &Connection, hash: &str, record: &BookRecord, original: &BookRecord) -> Result<(), String> {
    if has_cover(dir, hash) {
        return Ok(());
    }
    let mut statement = db.prepare("SELECT hash FROM book WHERE (title=?1 AND author=?2) OR (title=?3 AND author=?4) ORDER BY rowid").map_err(|e| e.to_string())?;
    let matches = statement
        .query_map(rusqlite::params![record.title, curation::author(record), original.title, curation::author(original)], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for previous in matches {
        if previous == hash || !has_cover(dir, &previous) {
            continue;
        }
        for suffix in ["card".to_owned(), "detail".to_owned(), thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH), thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH)] {
            let from = dir.join(format!("covers/{previous}-{suffix}.jpg"));
            let to = dir.join(format!("covers/{hash}-{suffix}.jpg"));
            if from.exists() && !to.exists() {
                std::fs::copy(from, to).map_err(|e| e.to_string())?;
            }
        }
        break;
    }
    Ok(())
}

fn metadata_url() -> Result<Url, String> {
    Url::parse(&std::env::var("BOKHEIM_METADATA_URL").unwrap_or_else(|_| "https://meta.bokheim.se/".into())).map_err(|e| e.to_string())
}

/// The book's record merged with the metadata server's answer, as the
/// library merges it, the answer's cover reference, and whether the server
/// found the book: only then do the record's authors carry its identities.
fn enrich(http: &Client, record: BookRecord, file_name: &str, needs_cover: bool) -> Result<(BookRecord, Option<String>, bool), String> {
    let _timing = timing::measure("metadata_enrich", file_name);
    // The file's own description always wins, so the server needs none.
    let mut facts = record.clone();
    facts.description.clear();
    let readings = book_inspection::title_readings(&record, &[file_name.to_owned()]);
    let body = serde_json::to_vec(&BookEnrichmentRequest { books: vec![BookFacts { key: String::new(), record: facts, audiobook: false, needs_cover, readings, recording: None }] }).map_err(|e| e.to_string())?;
    let url = metadata_url()?.join("v3/books").map_err(|e| e.to_string())?;
    let response = http.post(url.clone()).header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().and_then(|r| r.error_for_status()).map_err(|e| format!("{url}: {e}"))?;
    let response: BookEnrichmentResponse = serde_json::from_slice(&response.bytes().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let answer = response.results.into_iter().next().ok_or("the metadata server omitted the book")?;
    enrichment_result(record, file_name, answer)
}

fn enrichment_result(record: BookRecord, file_name: &str, answer: BookEnrichment) -> Result<(BookRecord, Option<String>, bool), String> {
    match (answer.status, answer.record) {
        (BookEnrichmentStatus::Failed, _) if answer.detail.contains("for url (https://covers.openlibrary.org/") => {
            eprintln!("metadata cover unavailable for {file_name}; retaining inspected metadata: {}", answer.detail);
            Ok((record, None, false))
        }
        (BookEnrichmentStatus::Failed, _) => Err(format!("enrichment failed, add again later: {}", answer.detail)),
        (BookEnrichmentStatus::Found, Some(mut found)) => {
            manifest::SourceMetadata::from_record(&record).retain_evidence(&mut found);
            if !record.description.is_empty() {
                found.description = record.description;
            }
            Ok((found, answer.cover.map(|cover| cover.id), true))
        }
        _ => Ok((record, None, false)),
    }
}

#[cfg(test)]
mod enrichment_tests {
    use super::*;
    use book_model::BookDetails;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::spawn;
    use std::time::Duration;

    #[test]
    fn standardebooks_new_import_uses_embedded_art_and_never_requests_an_enriched_cover() {
        struct RestoreEnvironment(Option<std::ffi::OsString>);
        impl Drop for RestoreEnvironment {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("BOKHEIM_METADATA_URL", value),
                    None => std::env::remove_var("BOKHEIM_METADATA_URL"),
                }
            }
        }
        let _restore = RestoreEnvironment(std::env::var_os("BOKHEIM_METADATA_URL"));
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let bytes = crate::covers::tests::standardebooks_epub("https://standardebooks.org/ebooks/a-writer/a-source-work");
        let hash = content_address::hash_reader(&mut &bytes[..]).unwrap().to_hex();
        let inspected = book_inspection::inspect_book_reader_with_hash(Path::new("book.epub"), BookFormat::Epub, Cursor::new(bytes.clone()), None).unwrap();
        curation::set_record(&db, "matching-old-edition", &inspected.metadata).unwrap();
        for key in ["matching-old-edition", &hash] {
            for suffix in ["card", "detail"] {
                std::fs::write(dir.path().join(format!("covers/{key}-{suffix}.jpg")), b"another edition's cover").unwrap();
            }
        }
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        std::env::set_var("BOKHEIM_METADATA_URL", format!("http://{}/", server.local_addr().unwrap()));
        let serving = spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap();
            assert!(header.starts_with("POST /v3/books"));
            let length = header.lines().find_map(|line| line.split_once(':').filter(|(key, _)| key.eq_ignore_ascii_case("content-length")).map(|(_, size)| size.trim().parse::<usize>().unwrap())).unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let request: BookEnrichmentRequest = serde_json::from_slice(&body).unwrap();
            assert!(!request.books[0].needs_cover);
            let response = serde_json::to_vec(&serde_json::json!({"policy":"books_v5","results":[{"key":"","status":"found","record":request.books[0].record,"cover":{"id":"unwanted-cover"}}]})).unwrap();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).unwrap();
            stream.write_all(&response).unwrap();
        });
        let http = Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
        assert_eq!(store_book_with_embedded_cover(dir.path(), &db, &http, &bytes, "book.epub", None).unwrap(), hash);
        serving.join().unwrap();
        let expected = thumbnail::generate_thumbnail_versions_with_pdf_renderer("epub", Cursor::new(bytes.clone()), thumbnail::render_pdf_cover).unwrap().unwrap();
        assert_eq!(std::fs::read(dir.path().join(format!("covers/{hash}-card.jpg"))).unwrap(), expected.browse);
        assert_eq!(std::fs::read(dir.path().join(format!("covers/{hash}-detail.jpg"))).unwrap(), expected.high_density);
        assert_eq!(std::fs::read(dir.path().join(format!("books/{hash}.epub"))).unwrap(), bytes);
        assert_eq!(std::fs::read(dir.path().join("covers/matching-old-edition-card.jpg")).unwrap(), b"another edition's cover");
    }

    #[test]
    fn replacement_reuses_existing_cover_without_modifying_old_images() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        record.title = "A retained work".into();
        curation::set_record(&db, "old", &record).unwrap();
        for suffix in ["card", "detail"] {
            std::fs::write(dir.path().join(format!("covers/old-{suffix}.jpg")), suffix).unwrap();
        }
        preserve_matching_cover(dir.path(), &db, "new", &record, &record).unwrap();
        for suffix in ["card", "detail"] {
            assert_eq!(std::fs::read(dir.path().join(format!("covers/new-{suffix}.jpg"))).unwrap(), suffix.as_bytes());
            assert_eq!(std::fs::read(dir.path().join(format!("covers/old-{suffix}.jpg"))).unwrap(), suffix.as_bytes());
        }
        std::fs::write(dir.path().join("covers/new-card.jpg"), b"another chosen cover").unwrap();
        preserve_matching_cover(dir.path(), &db, "new", &record, &record).unwrap();
        assert_eq!(std::fs::read(dir.path().join("covers/new-card.jpg")).unwrap(), b"another chosen cover");
    }

    #[test]
    fn enrichment_preserves_all_source_isbns_and_raw_subjects() {
        use book_model::{Identifier, Scheme, Scope};
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        record.book.identifiers = ["9798238014975", "9780393243277"].into_iter().map(|isbn| Identifier::new(isbn, Scheme::Isbn, Scope::Book).unwrap()).collect();
        record.book.subjects.push(BookSubject::new(None, "Justice -- Early works to 1800", "catalog:gutenberg.org", Some("lcsh".into()), None).unwrap());
        let mut found = BookRecord { metadata: BookDetails::default(), description: String::new() };
        found.title = "Enriched title".into();
        found.book.identifiers.push(Identifier::new("9780821338278", Scheme::Isbn, Scope::Edition).unwrap());
        let answer: BookEnrichment = serde_json::from_value(serde_json::json!({"key":"", "status":"found", "record":found})).unwrap();
        let (enriched, _, matched) = enrichment_result(record.clone(), "book.epub", answer).unwrap();
        assert!(matched);
        assert_eq!(enriched.title, "Enriched title");
        assert_eq!(enriched.book.identifiers.len(), 3);
        assert!(record.book.identifiers.iter().all(|id| enriched.book.identifiers.contains(id)));
        assert_eq!(enriched.book.subjects, record.book.subjects);
    }

    #[test]
    fn local_and_source_covers_are_checked_before_metadata_and_missing_art_still_falls_back() {
        struct RestoreEnvironment(Option<std::ffi::OsString>);
        impl Drop for RestoreEnvironment {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("BOKHEIM_METADATA_URL", value),
                    None => std::env::remove_var("BOKHEIM_METADATA_URL"),
                }
            }
        }
        let _restore = RestoreEnvironment(std::env::var_os("BOKHEIM_METADATA_URL"));
        let bytes = include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        let hash = content_address::hash_reader(&mut &bytes[..]).unwrap().to_hex();
        let versions = thumbnail::ThumbnailVersions::from_image(&image::RgbImage::new(8, 8)).unwrap();
        for scenario in ["retained", "source", "missing", "broken_source"] {
            let dir = tempfile::tempdir().unwrap();
            let db = curation::open(dir.path()).unwrap();
            if scenario == "retained" {
                store_cover(dir.path(), &hash, thumbnail::ThumbnailVersions { browse: versions.browse.clone(), card_high_density: versions.card_high_density.clone(), high_density: versions.high_density.clone() }).unwrap();
            }
            let server = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}/", server.local_addr().unwrap());
            std::env::set_var("BOKHEIM_METADATA_URL", &base);
            let source_image = matches!(scenario, "source" | "broken_source").then(|| Url::parse(&format!("{base}source.jpg")).unwrap());
            let jpeg = versions.browse.clone();
            let key = hash.clone();
            let cover_dir = dir.path().to_owned();
            let serving = spawn(move || {
                let paths = match scenario {
                    "source" => vec!["GET /source.jpg", "POST /v3/books"],
                    "broken_source" => vec!["GET /source.jpg", "POST /v3/books", "GET /v3/covers/fallback"],
                    "missing" => vec!["POST /v3/books", "GET /v3/covers/fallback"],
                    _ => vec!["POST /v3/books"],
                };
                for path in paths {
                    let (mut stream, _) = server.accept().unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        header.push(byte[0]);
                    }
                    let header = String::from_utf8(header).unwrap();
                    assert!(header.starts_with(path), "{scenario}: unexpected request {header}");
                    let (status, response) = if path.starts_with("POST") {
                        let length = header.lines().find_map(|line| line.split_once(':').filter(|(key, _)| key.eq_ignore_ascii_case("content-length")).map(|(_, size)| size.trim().parse::<usize>().unwrap())).unwrap();
                        let mut body = vec![0; length];
                        stream.read_exact(&mut body).unwrap();
                        let request: BookEnrichmentRequest = serde_json::from_slice(&body).unwrap();
                        let needs_cover = matches!(scenario, "missing" | "broken_source");
                        assert_eq!(request.books[0].needs_cover, needs_cover, "{scenario}");
                        assert_eq!(has_cover(&cover_dir, &key), !needs_cover, "{scenario}: local art must exist before enrichment");
                        let response = if needs_cover {
                            serde_json::json!({"policy":"books_v5","results":[{"key":"","status":"found","record":request.books[0].record,"cover":{"id":"fallback"}}]})
                        } else {
                            serde_json::json!({"policy":"books_v5","results":[{"key":"","status":"no_match","record":null}]})
                        };
                        ("200 OK", serde_json::to_vec(&response).unwrap())
                    } else if path == "GET /source.jpg" && scenario == "broken_source" {
                        ("404 Not Found", Vec::new())
                    } else {
                        ("200 OK", jpeg.clone())
                    };
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).unwrap();
                    stream.write_all(&response).unwrap();
                }
            });
            let http = Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
            let stored = store_book(dir.path(), &db, &http, bytes, "test.epub", Vec::new(), source_image.as_ref(), None).unwrap();
            serving.join().unwrap();
            assert_eq!(stored, hash);
            assert!(has_cover(dir.path(), &hash), "{scenario}");
            assert_eq!(std::fs::read(dir.path().join(format!("covers/{hash}-card.jpg"))).unwrap(), versions.browse);
        }
    }

    #[test]
    fn an_unavailable_cover_preserves_metadata_but_other_enrichment_failures_remain_errors() {
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        record.title = "Source title".into();
        record.description = "Source description".into();
        let mut answer: BookEnrichment = serde_json::from_value(serde_json::json!({
            "key":"", "status":"failed", "record":null,
            "detail":"HTTP status server error (502 Bad Gateway) for url (https://covers.openlibrary.org/b/isbn/123-L.jpg?default=false)"
        }))
        .unwrap();
        let (retained, cover, enriched) = enrichment_result(record.clone(), "source.epub", answer.clone()).unwrap();
        assert_eq!(retained.title, record.title);
        assert_eq!(retained.description, record.description);
        assert!(cover.is_none());
        assert!(!enriched);
        answer.detail = "metadata database is unavailable".into();
        assert!(enrichment_result(record, "source.epub", answer).is_err());
    }
}

fn edit_subjects(db: &Connection, hash: &str, edit: impl FnOnce(&mut BookRecord) -> Result<(), String>) -> Result<(), String> {
    let hash = curation::resolve(db, hash)?;
    let mut record = curation::record(db, &hash)?.ok_or("the book was removed")?;
    edit(&mut record)?;
    curation::set_record(db, &hash, &record)
}

fn review(dir: &Path, db: &Connection) -> Result<(), String> {
    for (hash, record, sources) in curation::books(db)? {
        let authors = curation::author(&record);
        let cover = dir.join("covers").join(format!("{hash}-card.jpg")).exists();
        println!("{}  {} — {}", &hash[..12], record.title, authors);
        println!("    {}  |  cover: {}", sources.join(", "), if cover { "yes" } else { "none" });
        for code in subject_projection::project_subject_codes(&record.book.subjects) {
            let paths = subject_projection::unified_subject_paths(&code.system_id, &code.code);
            println!("    {}:{}  →  {}", code.system_id, code.code, if paths.is_empty() { "(not in taxonomy)".to_owned() } else { paths.join("  |  ") });
        }
    }
    let mut links = db.prepare("SELECT title,author,entry_url,license FROM external_book ORDER BY title").map_err(|e| e.to_string())?;
    let rows = links.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))).map_err(|e| e.to_string())?;
    for row in rows {
        let (title, author, url, license) = row.map_err(|e| e.to_string())?;
        println!("{title} — {author}\n    external: {url}  |  license: {license}");
    }
    Ok(())
}

fn has_cover(dir: &Path, key: &str) -> bool {
    ["card", "detail"].iter().all(|size| dir.join(format!("covers/{key}-{size}.jpg")).is_file())
}

/// Reuse retained art, or retain the source's image before asking for remote
/// artwork. A broken source image still allows metadata-cover fallback.
fn source_cover(dir: &Path, key: &str, http: &Client, image: Option<&Url>) -> bool {
    if has_cover(dir, key) {
        return true;
    }
    let Some(image) = image else { return false };
    match get(http, image).and_then(|bytes| cover_from_image(dir, key, &bytes)) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("optional source cover unavailable for {key}: {error}");
            false
        }
    }
}

/// The book's own cover, as the library would extract it. False when it has none.
fn cover_from_book(dir: &Path, hash: &str, bytes: &[u8]) -> Result<bool, String> {
    let versions = thumbnail::generate_thumbnail_versions_with_pdf_renderer("epub", Cursor::new(bytes.to_vec()), thumbnail::render_pdf_cover).map_err(|e| e.to_string())?;
    let Some(versions) = versions else { return Ok(false) };
    store_cover(dir, hash, versions)?;
    Ok(true)
}

fn cover_from_image(dir: &Path, hash: &str, bytes: &[u8]) -> Result<(), String> {
    let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
    let decoded = thumbnail::decode_cover_image(bytes, format).map_err(|e| e.to_string())?;
    store_cover(dir, hash, thumbnail::ThumbnailVersions::from_image(&decoded).map_err(|e| e.to_string())?)
}

/// Both sizes the client draws, by the library's thumbnail code.
fn store_cover(dir: &Path, hash: &str, versions: thumbnail::ThumbnailVersions) -> Result<(), String> {
    std::fs::write(dir.join("covers").join(format!("{hash}-card.jpg")), &versions.browse).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("covers").join(format!("{hash}-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH))), &versions.browse).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("covers").join(format!("{hash}-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH))), &versions.card_high_density).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("covers").join(format!("{hash}-detail.jpg")), &versions.high_density).map_err(|e| e.to_string())
}
