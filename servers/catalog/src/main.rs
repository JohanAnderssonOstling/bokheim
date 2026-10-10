//! Curates public-domain books into a catalog snapshot. Every book is added by
//! hand, from a named OPDS entry or a file dropped in the inbox; nothing is
//! crawled. Adding a book inspects and
//! enriches it, so every book in the database is in the catalog. See
//! docs/public-domain-catalog-plan.md.
mod curation;
mod opds;
mod publish;

use book_model::{BookFormat, BookRecord, BookSubject};
use metadata_contract::books::{BookEnrichmentRequest, BookEnrichmentResponse, BookEnrichmentStatus, BookFacts};
use reqwest::blocking::Client;
use reqwest::Url;
use rusqlite::Connection;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: catalog DIR search FEED_URL
       catalog DIR add FEED_URL [ENTRY_ID]
       catalog DIR import
       catalog DIR remove HASH
       catalog DIR review
       catalog DIR subject|unsubject HASH lcc|ddc|bisac|unified CODE
       catalog DIR cover HASH IMAGE_URL
       catalog DIR publish OUT_DIR RELEASE_ID

DIR holds curation.sqlite3 and the books and covers. HASH may be a prefix.
import adds the EPUBs dropped in DIR/inbox/SOURCE/, SOURCE naming where they
came from (for example DIR/inbox/standardebooks.org/).
add and import ask BOKHEIM_METADATA_URL (default https://meta.bokheim.se/).
publish writes the tree the catalog origin serves (releases/N/catalog.sqlite3,
books/, covers/) and prints the manifest's catalog entry; the URL uses
BOKHEIM_CATALOG_ORIGIN (default https://catalog.bokheim.se).";

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let [dir, command, rest @ ..] = args else { return Err(USAGE.into()) };
    let dir = PathBuf::from(dir);
    let db = curation::open(&dir)?;
    let http = Client::builder().user_agent("bokheim-catalog").build().map_err(|e| e.to_string())?;
    match (command.as_str(), rest) {
        ("search", [feed]) => search(&http, feed),
        ("add", [feed]) => add(&dir, &db, &http, feed, None),
        ("add", [feed, entry]) => add(&dir, &db, &http, feed, Some(entry)),
        ("import", []) => import(&dir, &db, &http),
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
    let epub = entry.epub.as_ref().ok_or("the entry offers no EPUB")?;
    let bytes = get(http, epub)?;
    let file_name = epub.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or("book.epub").to_owned();
    let source = base.host_str().unwrap_or_default();
    // The source's own classification is evidence for enrichment.
    let evidence = format!("catalog:{source}");
    let lcsh = entry.lcsh.iter().filter_map(|name| BookSubject::new(None, name, &evidence, Some("lcsh".into()), None).ok());
    let lcc = entry.lcc.iter().filter_map(|code| BookSubject::new(None, code, &evidence, Some("lcc".into()), Some(code.clone())).ok());
    let hash = store_book(dir, db, http, &bytes, &file_name, lcsh.chain(lcc).collect(), entry.image.as_ref())?;
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
        let added = std::fs::read(&file).map_err(|e| e.to_string()).and_then(|bytes| store_book(dir, db, http, &bytes, &name, Vec::new(), None)).and_then(|hash| curation::add_source(db, &hash, &source, &name, None, None));
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
fn store_book(dir: &Path, db: &Connection, http: &Client, bytes: &[u8], file_name: &str, evidence: Vec<BookSubject>, image: Option<&Url>) -> Result<String, String> {
    let content_hash = content_address::hash_reader(&mut &bytes[..]).map_err(|e| e.to_string())?;
    let hash = content_hash.to_hex();
    if curation::record(db, &hash)?.is_some() {
        println!("{hash}  already in the catalog");
        return Ok(hash);
    }
    let inspected = book_inspection::inspect_book_reader_with_hash(Path::new(file_name), BookFormat::Epub, Cursor::new(bytes.to_vec()), Some(content_hash)).map_err(|e| e.to_string())?;
    let mut record = inspected.metadata;
    for subject in evidence {
        if !record.book.subjects.iter().any(|existing| existing.name() == subject.name() && existing.authority() == subject.authority()) {
            record.book.subjects.push(subject);
        }
    }
    let (record, cover_id) = enrich(http, record, file_name)?;
    if !cover_from_book(dir, &hash, bytes)? {
        let answer_cover = cover_id.map(|id| metadata_url()?.join(&format!("v3/covers/{id}")).map_err(|e| e.to_string())).transpose()?;
        if let Some(image) = image.cloned().or(answer_cover) {
            cover_from_image(dir, &hash, &get(http, &image)?)?;
        }
    }
    std::fs::write(dir.join("books").join(format!("{hash}.epub")), bytes).map_err(|e| e.to_string())?;
    curation::set_record(db, &hash, &record)?;
    println!("{hash}  {}", record.title);
    Ok(hash)
}

fn metadata_url() -> Result<Url, String> {
    Url::parse(&std::env::var("BOKHEIM_METADATA_URL").unwrap_or_else(|_| "https://meta.bokheim.se/".into())).map_err(|e| e.to_string())
}

/// The book's record merged with the metadata server's answer, as the
/// library merges it, and the answer's cover reference.
fn enrich(http: &Client, record: BookRecord, file_name: &str) -> Result<(BookRecord, Option<String>), String> {
    // The file's own description always wins, so the server needs none.
    let mut facts = record.clone();
    facts.description.clear();
    let readings = book_inspection::title_readings(&record, &[file_name.to_owned()]);
    let body = serde_json::to_vec(&BookEnrichmentRequest { books: vec![BookFacts { key: String::new(), record: facts, audiobook: false, readings, recording: None }] }).map_err(|e| e.to_string())?;
    let url = metadata_url()?.join("v3/books").map_err(|e| e.to_string())?;
    let response = http.post(url.clone()).header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().and_then(|r| r.error_for_status()).map_err(|e| format!("{url}: {e}"))?;
    let response: BookEnrichmentResponse = serde_json::from_slice(&response.bytes().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let answer = response.results.into_iter().next().ok_or("the metadata server omitted the book")?;
    match (answer.status, answer.record) {
        (BookEnrichmentStatus::Failed, _) => Err(format!("enrichment failed, add again later: {}", answer.detail)),
        (BookEnrichmentStatus::Found, Some(mut found)) => {
            if !record.description.is_empty() {
                found.description = record.description;
            }
            Ok((found, answer.cover.map(|cover| cover.id)))
        }
        _ => Ok((record, None)),
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
    Ok(())
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
    std::fs::write(dir.join("covers").join(format!("{hash}-detail.jpg")), &versions.high_density).map_err(|e| e.to_string())
}
