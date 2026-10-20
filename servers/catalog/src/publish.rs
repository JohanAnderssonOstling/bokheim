//! Builds the snapshot clients attach, and the files they download.
use crate::curation;
use book_model::{AuthorId, BookMetadata, BookRecord, Scheme, Scope};
use metadata_contract::catalog::source_evidence;
use metadata_contract::matching::author_match_keys;
use reqwest::Url;
use rusqlite::{params, Connection, OpenFlags};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use subject_projection::{project_subject_codes, project_subject_concepts, unified_taxonomy_revision_id, UNIFIED_MATCHER_VERSION};

mod compression;
mod integer_keys;

pub const FORMAT_VERSION: u32 = 8;
const PAGE_SIZE: u32 = 8192;

/// Adds transfer encodings without changing an immutable snapshot's bytes,
/// release ID, or signed digest.
pub fn compress_snapshot(path: &Path) -> Result<(), String> {
    let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    source.query_row("SELECT release_id,format_version FROM snapshot WHERE singleton=1", [], |_| Ok(())).map_err(|error| error.to_string())?;
    drop(source);
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    compression::write_sidecars(path, &bytes)
}

/// Publication is the only writer; this temporary lookup avoids retaining a
/// second copy of every description in a SQLite uniqueness index.
#[derive(Default)]
struct Descriptions {
    by_text: HashMap<String, i64>,
}

impl Descriptions {
    fn store(&mut self, db: &Connection, text: &str) -> Result<i64, String> {
        if let Some(id) = self.by_text.get(text) {
            return Ok(*id);
        }
        let id = db.prepare_cached("INSERT INTO description(text) VALUES(?1) RETURNING row_id").map_err(|error| error.to_string())?.query_row([text], |row| row.get(0)).map_err(|error| error.to_string())?;
        self.by_text.insert(text.to_owned(), id);
        Ok(id)
    }
}

fn key_bytes(key: &str) -> Result<Vec<u8>, String> {
    Ok(blake3::Hash::from_hex(key).map_err(|error| error.to_string())?.as_bytes().to_vec())
}

fn book_id(db: &Connection, key: &str) -> Result<i64, String> {
    db.query_row("SELECT row_id FROM book WHERE key=?1", [key_bytes(key)?], |row| row.get(0)).map_err(|error| error.to_string())
}

fn source_id(db: &Connection, sources: &mut BTreeMap<String, i64>, name: &str) -> Result<i64, String> {
    if let Some(id) = sources.get(name) {
        return Ok(*id);
    }
    let id = sources.values().max().copied().unwrap_or(0) + 1;
    db.execute("INSERT INTO source(id,name) VALUES(?1,?2)", params![id, name]).map_err(|error| error.to_string())?;
    sources.insert(name.to_owned(), id);
    Ok(id)
}

fn store_evidence(db: &Connection, sources: &mut BTreeMap<String, i64>, source: &str, entry_id: &str, url: &str, hosted: Option<&str>, external: Option<&str>, metadata: &str) -> Result<(), String> {
    let metadata = serde_json::from_str(metadata).map_err(|error| error.to_string())?;
    let evidence = source_evidence(source, &metadata)?;
    let id = source_id(db, sources, source)?;
    let value = evidence.popularity.as_ref();
    let ranking_source = value.and_then(|value| value.source.as_deref()).map(|name| source_id(db, sources, name)).transpose()?;
    let combined = value.and_then(|value| value.combined_popularity.as_ref());
    let hosted_id = hosted.map(|key| book_id(db, key)).transpose()?;
    let external_id = external.map(|key| book_id(db, key)).transpose()?;
    let popularity = value.and_then(|value| value.popularity).map(i64::try_from).transpose().map_err(|error| error.to_string())?;
    let epub_popularity = value.and_then(|value| value.epub_popularity).map(i64::try_from).transpose().map_err(|error| error.to_string())?;
    db.prepare_cached("INSERT INTO source_entry(source_id,entry_id,entry_url,hosted_row_id,external_row_id,ranking_source_id,score,popularity,epub_popularity,reading_percentile,epub_percentile) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)")
        .map_err(|error| error.to_string())?
        .execute(params![
            id,
            entry_id,
            url,
            hosted_id,
            external_id,
            ranking_source,
            value.and_then(|value| value.score()),
            popularity,
            epub_popularity,
            combined.and_then(|value| value.reading_percentile),
            combined.and_then(|value| value.epub_percentile)
        ])
        .map_err(|error| error.to_string())?;
    for name in evidence.sources {
        let original = source_id(db, sources, &name)?;
        db.execute("INSERT OR IGNORE INTO source_entry_source(source_id,entry_id,original_source_id) VALUES(?1,?2,?3)", params![id, entry_id, original]).map_err(|error| error.to_string())?;
    }
    if evidence.external_audio {
        if let Some(key) = external {
            db.execute("UPDATE book SET format=16 WHERE row_id=?1 AND external=1 AND format=1", [book_id(db, key)?]).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn store_metadata(db: &Connection, key: &str, metadata: &BookMetadata) -> Result<(), String> {
    let key = book_id(db, key)?;
    for language in &metadata.languages {
        db.prepare_cached("INSERT OR IGNORE INTO book_language(book_row_id,language) VALUES(?1,?2)").map_err(|error| error.to_string())?.execute(params![key, language.as_str()]).map_err(|error| error.to_string())?;
    }
    for identifier in &metadata.identifiers {
        let (scheme, label) = match &identifier.scheme {
            Scheme::Unspecified => (0, ""),
            Scheme::Isbn => (1, ""),
            Scheme::Asin => (2, ""),
            Scheme::Doi => (3, ""),
            Scheme::Issn => (4, ""),
            Scheme::Oclc => (5, ""),
            Scheme::Lccn => (6, ""),
            Scheme::Uuid => (7, ""),
            Scheme::Calibre => (8, ""),
            Scheme::Google => (9, ""),
            Scheme::Goodreads => (10, ""),
            Scheme::Kobo => (11, ""),
            Scheme::Other(label) => (12, label.as_str()),
        };
        let scope = match identifier.scope {
            Scope::Book => 0,
            Scope::Edition => 1,
            Scope::Work => 2,
        };
        db.prepare_cached("INSERT OR IGNORE INTO book_identifier(book_row_id,scheme,scheme_label,value,scope) VALUES(?1,?2,?3,?4,?5)")
            .map_err(|error| error.to_string())?
            .execute(params![key, scheme, label, identifier.value, scope])
            .map_err(|error| error.to_string())?;
    }
    for subject in &metadata.subjects {
        let source: i64 = db
            .prepare_cached("INSERT INTO subject_source(name) VALUES(?1) ON CONFLICT DO UPDATE SET name=excluded.name RETURNING id")
            .map_err(|error| error.to_string())?
            .query_row([&subject.source], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        let authority = subject
            .authority
            .as_ref()
            .map(|name| {
                db.prepare_cached("INSERT INTO subject_authority(name) VALUES(?1) ON CONFLICT DO UPDATE SET name=excluded.name RETURNING id")
                    .map_err(|error| error.to_string())?
                    .query_row([name], |row| row.get::<_, i64>(0))
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        let id: i64 = db
            .prepare_cached("INSERT INTO subject(id,name,sort_name,source_id,authority_id,code) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT DO UPDATE SET name=excluded.name RETURNING subject_id")
            .map_err(|error| error.to_string())?
            .query_row(params![subject.id, subject.name, subject.sort_name, source, authority, subject.code], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        db.prepare_cached("INSERT OR IGNORE INTO book_subject(book_row_id,subject_id) VALUES(?1,?2)").map_err(|error| error.to_string())?.execute(params![key, id]).map_err(|error| error.to_string())?;
    }
    let codes = project_subject_codes(&metadata.subjects);
    for (_, concept) in project_subject_concepts(&codes) {
        db.prepare_cached("INSERT OR IGNORE INTO book_concept(book_row_id,concept_id) VALUES(?1,?2)").map_err(|error| error.to_string())?.execute(params![key, concept]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Default)]
struct Authors {
    by_identity: HashMap<AuthorId, AuthorId>,
    by_name: HashMap<String, AuthorId>,
}

impl Authors {
    /// Only author credits enter the catalog's author index.
    fn store(&mut self, db: &Connection, key: &str, record: &BookRecord) -> Result<(), String> {
        let key = book_id(db, key)?;
        for credit in record.authors() {
            let keys = author_match_keys(credit.name());
            let id = self.by_identity.get(&credit.agent_id()).or_else(|| keys.iter().find_map(|key| self.by_name.get(key))).copied().unwrap_or(credit.agent_id());
            self.by_identity.insert(credit.agent_id(), id);
            for key in keys {
                self.by_name.entry(key).or_insert(id);
            }
            let author: i64 = db
                .prepare_cached("INSERT INTO author(author_id,name) VALUES(?1,?2) ON CONFLICT(author_id) DO UPDATE SET name=min(name,excluded.name) RETURNING row_id")
                .map_err(|error| error.to_string())?
                .query_row(params![&id.as_bytes()[..], credit.name()], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            db.execute("INSERT OR IGNORE INTO book_author(book_row_id,author_row_id) VALUES(?1,?2)", params![key, author]).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

const SCHEMA: &str = "
CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),format_version INTEGER NOT NULL,release_id INTEGER NOT NULL) STRICT;
CREATE TABLE source(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE description(row_id INTEGER PRIMARY KEY,text TEXT NOT NULL) STRICT;
CREATE TABLE book(
 row_id INTEGER PRIMARY KEY,key BLOB NOT NULL UNIQUE CHECK(length(key)=32),external INTEGER NOT NULL CHECK(external IN(0,1)),
 title TEXT NOT NULL,subtitle TEXT,description_id INTEGER NOT NULL REFERENCES description(row_id),year INTEGER,file_bytes INTEGER,
 source_id INTEGER REFERENCES source(id),entry_url TEXT,download_url TEXT,format INTEGER NOT NULL CHECK(format IN(1,2,4,8,16))
) STRICT;
CREATE VIEW external_book AS SELECT book.row_id,key AS entry_key,source_id,entry_url,title,subtitle,description.text AS description,year,download_url,format
 FROM book JOIN description ON description.row_id=book.description_id WHERE external=1;
CREATE TABLE author(row_id INTEGER PRIMARY KEY,author_id BLOB NOT NULL UNIQUE CHECK(length(author_id)=16),name TEXT NOT NULL) STRICT;
CREATE TABLE book_author(book_row_id INTEGER NOT NULL REFERENCES book(row_id),author_row_id INTEGER NOT NULL REFERENCES author(row_id),PRIMARY KEY(book_row_id,author_row_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_language(book_row_id INTEGER NOT NULL REFERENCES book(row_id),language TEXT NOT NULL,PRIMARY KEY(book_row_id,language)) STRICT, WITHOUT ROWID;
CREATE TABLE book_identifier(book_row_id INTEGER NOT NULL REFERENCES book(row_id),scheme INTEGER NOT NULL CHECK(scheme BETWEEN 0 AND 12),scheme_label TEXT NOT NULL,value TEXT NOT NULL,scope INTEGER NOT NULL CHECK(scope IN(0,1,2)),PRIMARY KEY(book_row_id,scheme,scheme_label,value,scope)) STRICT, WITHOUT ROWID;
CREATE TABLE subject_source(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE UNIQUE INDEX subject_source_name ON subject_source(name);
CREATE TABLE subject_authority(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE UNIQUE INDEX subject_authority_name ON subject_authority(name);
CREATE TABLE subject(subject_id INTEGER PRIMARY KEY,id TEXT,name TEXT NOT NULL,sort_name TEXT,source_id INTEGER NOT NULL REFERENCES subject_source(id),authority_id INTEGER REFERENCES subject_authority(id),code TEXT) STRICT;
CREATE UNIQUE INDEX subject_identity ON subject(coalesce(id,''),name,coalesce(sort_name,''),source_id,coalesce(authority_id,0),coalesce(code,''));
CREATE TABLE book_subject(book_row_id INTEGER NOT NULL REFERENCES book(row_id),subject_id INTEGER NOT NULL REFERENCES subject(subject_id),PRIMARY KEY(book_row_id,subject_id)) STRICT, WITHOUT ROWID;
CREATE TABLE subject_projection(singleton INTEGER PRIMARY KEY CHECK(singleton=1),taxonomy_revision TEXT NOT NULL) STRICT;
CREATE TABLE book_concept(book_row_id INTEGER NOT NULL REFERENCES book(row_id),concept_id INTEGER NOT NULL,PRIMARY KEY(book_row_id,concept_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_source(book_row_id INTEGER NOT NULL REFERENCES book(row_id),source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(book_row_id,source_id)) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry(
 source_id INTEGER NOT NULL REFERENCES source(id),entry_id TEXT NOT NULL,entry_url TEXT NOT NULL,
 hosted_row_id INTEGER REFERENCES book(row_id),external_row_id INTEGER REFERENCES book(row_id),ranking_source_id INTEGER REFERENCES source(id),
 score REAL,popularity INTEGER CHECK(popularity>=0),epub_popularity INTEGER CHECK(epub_popularity>=0),reading_percentile REAL,epub_percentile REAL,
 PRIMARY KEY(source_id,entry_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry_source(source_id INTEGER NOT NULL,entry_id TEXT NOT NULL,original_source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(source_id,entry_id,original_source_id),FOREIGN KEY(source_id,entry_id) REFERENCES source_entry(source_id,entry_id)) STRICT, WITHOUT ROWID;
";

fn finish_snapshot(db: &Connection) -> Result<(), String> {
    // Subject deduplication is complete. Readers join subjects by integer ID;
    // retaining the publication index would duplicate the subject text.
    db.execute_batch("DROP INDEX subject_identity; DROP INDEX subject_source_name; DROP INDEX subject_authority_name; VACUUM").map_err(|error| error.to_string())
}

/// Converts an installed legacy snapshot without crawling, changing curation,
/// or copying book and cover assets. The caller installs the result atomically.
pub fn compact_snapshot(input: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("compact snapshot output already exists".into());
    }
    let source = Connection::open_with_flags(input, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    let (release, format): (i64, u32) = source.query_row("SELECT release_id,format_version FROM snapshot WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|error| error.to_string())?;
    if format != 7 {
        return Err("compact-snapshot expects a format-7 catalog".into());
    }
    integer_keys::copy_normalized(input, output, release)
}

pub fn publish(dir: &Path, db: &Connection, out: &Path, release_id: u64) -> Result<(), String> {
    if release_id == 0 {
        return Err("RELEASE_ID must be a positive integer".into());
    }
    let path = out.join(format!("releases/{release_id}/catalog.sqlite3"));
    if path.exists() {
        return Err(format!("{} already exists; releases are immutable", path.display()));
    }
    std::fs::create_dir_all(out.join(format!("releases/{release_id}"))).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("books")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("covers")).map_err(|e| e.to_string())?;
    let books = curation::books(db)?;
    let snapshot = Connection::open_in_memory().map_err(|e| e.to_string())?;
    snapshot.pragma_update(None, "page_size", PAGE_SIZE).map_err(|e| e.to_string())?;
    snapshot.set_prepared_statement_cache_capacity(128);
    snapshot.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    snapshot.execute("INSERT INTO snapshot VALUES(1,?1,?2)", params![FORMAT_VERSION, release_id as i64]).map_err(|e| e.to_string())?;
    snapshot.execute("INSERT INTO subject_projection VALUES(1,?1)", [format!("{}:{UNIFIED_MATCHER_VERSION}", unified_taxonomy_revision_id())]).map_err(|e| e.to_string())?;
    let mut sources = BTreeMap::new();
    let mut authors = Authors::default();
    let mut descriptions = Descriptions::default();
    let mut total_file_bytes = 0;
    for (hash, record, book_sources) in &books {
        crate::covers::generate_cards(dir, hash)?;
        for file in [
            format!("books/{hash}.epub"),
            format!("covers/{hash}-card.jpg"),
            format!("covers/{hash}-detail.jpg"),
            format!("covers/{hash}-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH)),
            format!("covers/{hash}-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH)),
        ] {
            let (from, to) = (dir.join(&file), out.join(&file));
            if from.exists() && !to.exists() {
                if file.starts_with("books/") {
                    // Content-addressed EPUBs are immutable; share their blocks
                    // with local publication when both trees use one filesystem.
                    std::fs::hard_link(&from, &to).or_else(|_| std::fs::copy(&from, &to).map(|_| ())).map_err(|e| format!("{}: {e}", from.display()))?;
                } else {
                    std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
                }
            }
        }
        let file_bytes = std::fs::metadata(dir.join(format!("books/{hash}.epub"))).map_err(|e| e.to_string())?.len();
        let description = descriptions.store(&snapshot, &record.description)?;
        snapshot
            .execute(
                "INSERT INTO book(key,external,title,subtitle,description_id,year,file_bytes,format) VALUES(?1,0,?2,?3,?4,?5,?6,1)",
                params![key_bytes(hash)?, record.title, record.subtitle, description, record.book.year, file_bytes as i64],
            )
            .map_err(|e| e.to_string())?;
        authors.store(&snapshot, hash, record)?;
        store_metadata(&snapshot, hash, &record.book)?;
        for name in book_sources {
            let next = sources.len() as i64 + 1;
            let id = *sources.entry(name.clone()).or_insert(next);
            snapshot.execute("INSERT OR IGNORE INTO source(id,name) VALUES(?1,?2)", params![id, name]).map_err(|e| e.to_string())?;
            snapshot.execute("INSERT INTO book_source(book_row_id,source_id) VALUES(?1,?2)", params![book_id(&snapshot, hash)?, id]).map_err(|e| e.to_string())?;
        }
        total_file_bytes += file_bytes;
    }
    let mut links = db.prepare("SELECT entry_key,source,entry_url,title,subtitle,record,CASE WHEN format=16 AND EXISTS(SELECT 1 FROM source_entry WHERE external_entry_key=external_book.entry_key AND json_extract(metadata,'$.audiobook_delivery')='external_download') THEN file_url END,format FROM external_book ORDER BY entry_key").map_err(|e| e.to_string())?;
    let rows = links
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?, row.get::<_, Option<String>>(6)?, row.get::<_, i64>(7)?))
        })
        .map_err(|e| e.to_string())?;
    let mut external_count = 0;
    for row in rows {
        let (key, source, url, title, subtitle, record, download_url, format) = row.map_err(|e| e.to_string())?;
        // Older crawlers and stored source evidence can still contain the
        // malformed Archive.org links. Correct the offered acquisition URL
        // independently of when its source page was ingested.
        let download_url = download_url
            .map(|raw| {
                let parsed = Url::parse(&raw).map_err(|e| e.to_string())?;
                crate::loyalbooks::canonical_archive_download(parsed).map(|url| url.to_string())
            })
            .transpose()?;
        let next = sources.len() as i64 + 1;
        let id = *sources.entry(source.clone()).or_insert(next);
        snapshot.execute("INSERT OR IGNORE INTO source(id,name) VALUES(?1,?2)", params![id, source]).map_err(|e| e.to_string())?;
        let record: BookRecord = serde_json::from_str(&record).map_err(|error| error.to_string())?;
        let description = descriptions.store(&snapshot, &record.description)?;
        snapshot
            .execute(
                "INSERT INTO book(key,external,source_id,entry_url,title,subtitle,description_id,year,download_url,format) VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![key_bytes(&key)?, id, url, title, subtitle, description, record.book.year, download_url, format],
            )
            .map_err(|e| e.to_string())?;
        authors.store(&snapshot, &key, &record)?;
        store_metadata(&snapshot, &key, &record.book)?;
        // External identities address cover assets only, never hosted book files.
        crate::covers::generate_cards(dir, &key)?;
        for size in ["card".to_owned(), "detail".to_owned(), thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH), thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH)] {
            let file = format!("covers/{key}-{size}.jpg");
            let (from, to) = (dir.join(&file), out.join(&file));
            if from.exists() && !to.exists() {
                std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            }
        }
        external_count += 1;
    }
    let mut evidence =
        db.prepare("SELECT source,entry_id,entry_url,book_hash,external_entry_key,metadata FROM source_entry WHERE book_hash IS NOT NULL OR external_entry_key IS NOT NULL ORDER BY source,entry_id").map_err(|e| e.to_string())?;
    let rows = evidence
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?)))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (source, entry_id, entry_url, book_hash, external_key, metadata) = row.map_err(|e| e.to_string())?;
        store_evidence(&snapshot, &mut sources, &source, &entry_id, &entry_url, book_hash.as_deref(), external_key.as_deref(), &metadata)?;
    }
    finish_snapshot(&snapshot)?;
    let bytes = snapshot.serialize("main").map_err(|e| e.to_string())?.to_vec();
    drop(snapshot);
    // Assemble the database in memory and expose the release only after a
    // complete write. A failed write must not leave an immutable half-release.
    let temporary = path.with_extension("sqlite3.partial");
    std::fs::write(&temporary, &bytes).map_err(|error| error.to_string())?;
    if let Err(error) = compression::write_sidecars(&path, &bytes) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        compression::remove_sidecars(&path);
        return Err(error.to_string());
    }
    let size = bytes.len() as u64;
    eprintln!("{external_count} external catalog entries");
    eprintln!("{}: {} books, {} bytes ({} per book); {} bytes of books", path.display(), books.len(), size, size / books.len().max(1) as u64, total_file_bytes);
    // The manifest's catalog entry, for the release that publishes OUT_DIR at
    // the catalog origin.
    let origin = std::env::var("BOKHEIM_CATALOG_ORIGIN").unwrap_or_else(|_| "https://catalogs.bokheim.se".into());
    let sha256 = ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref().iter().map(|byte| format!("{byte:02x}")).collect();
    let artifact = update_protocol::Artifact { url: format!("{origin}/releases/{release_id}/catalog.sqlite3"), bytes: size, sha256 };
    let release = update_protocol::CatalogRelease { release_id, format_version: FORMAT_VERSION, artifact };
    println!("{}", serde_json::to_string_pretty(&release).map_err(|e| e.to_string())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_categories_keep_descendants_and_separate_branches_with_source_evidence() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute("INSERT INTO description VALUES(1,'')", []).unwrap();
        let key = "0".repeat(64);
        db.execute("INSERT INTO book(key,external,title,description_id,format) VALUES(?1,0,'Example',1,1)", [key_bytes(&key).unwrap()]).unwrap();
        let metadata = BookMetadata {
            subjects: vec![
                BookSubject::new(None, "PZ3", "metadata:work", Some("lcc".into()), Some("PZ3".into())).unwrap(),
                BookSubject::new(None, "PR4550", "metadata:work", Some("lcc".into()), Some("PR4550".into())).unwrap(),
                BookSubject::new(None, "Mystery", "manual:subject", Some("unified".into()), Some("Literature / Genres / Mystery & Thrillers / Mystery & Detective".into())).unwrap(),
            ],
            ..Default::default()
        };
        store_metadata(&db, &key, &metadata).unwrap();
        let ids = db.prepare("SELECT concept_id FROM book_concept ORDER BY concept_id").unwrap().query_map([], |r| r.get::<_, i64>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids, [3915, 4034]);
        assert_eq!(db.query_row("SELECT count(*) FROM book_subject", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
        assert_eq!(db.query_row("SELECT code FROM subject WHERE code='PZ3'", [], |r| r.get::<_, String>(0)).unwrap(), "PZ3");
    }

    #[test]
    fn stored_categories_apply_curated_exclusions_without_removing_raw_codes() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute("INSERT INTO description VALUES(1,'')", []).unwrap();
        let key = "0".repeat(64);
        db.execute("INSERT INTO book(key,external,title,description_id,format) VALUES(?1,0,'Example',1,1)", [key_bytes(&key).unwrap()]).unwrap();
        let metadata = BookMetadata {
            subjects: vec![
                BookSubject::new(None, "PR6003", "metadata:work", Some("lcc".into()), Some("PR6003".into())).unwrap(),
                BookSubject::new(None, "Satire", "manual:subject", Some("unified".into()), Some("Literature / Genres / Humorous Writing / Humorous Fiction / Satire".into())).unwrap(),
                BookSubject::new(None, "English", subject_projection::MANUAL_SUBJECT_EXCLUSION_SOURCE, Some("unified".into()), Some("Literature / By Language / English".into())).unwrap(),
                BookSubject::new(None, "Language", subject_projection::MANUAL_SUBJECT_EXCLUSION_SOURCE, Some("unified".into()), Some("Language".into())).unwrap(),
            ],
            ..Default::default()
        };
        store_metadata(&db, &key, &metadata).unwrap();
        let ids = db.prepare("SELECT concept_id FROM book_concept ORDER BY concept_id").unwrap().query_map([], |row| row.get::<_, i64>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids, [4107]);
        assert_eq!(db.query_row("SELECT count(*) FROM book_subject", [], |row| row.get::<_, i64>(0)).unwrap(), 4);
        assert_eq!(db.query_row("SELECT count(*) FROM subject WHERE code='PR6003'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    }
    use crate::links::{store_entry, Entry};
    use book_model::{BookDetails, BookFormat, BookRecord, BookSubject, BookTocEntry, Contributor, MarcRelatorCode, AUTHOR_MARC_RELATOR_CODE};

    const EPUB: &[u8] = include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/ocr-image-priority.epub");

    fn fixture(dir: &Path, db: &Connection) -> (String, BookRecord) {
        let hash = content_address::hash_reader(&mut EPUB).unwrap().to_hex();
        std::fs::write(dir.join(format!("books/{hash}.epub")), EPUB).unwrap();
        let record = BookRecord { metadata: BookDetails { title: "Curated title".into(), ..Default::default() }, description: "Curated description".into() };
        curation::set_record(db, &hash, &record).unwrap();
        curation::add_source(db, &hash, "test", "entry", None, None).unwrap();
        (hash, record)
    }

    #[test]
    fn publication_indexes_only_authors_for_hosted_and_external_epubs() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let (hash, mut record) = fixture(dir.path(), &db);
        record.contributors = vec![
            Contributor::new("An Editor", MarcRelatorCode(*b"edt")).unwrap(),
            Contributor::new("An Author", AUTHOR_MARC_RELATOR_CODE).unwrap(),
            Contributor::new("A Translator", MarcRelatorCode(*b"trl")).unwrap(),
            Contributor::new("An Illustrator", MarcRelatorCode(*b"ill")).unwrap(),
            Contributor::new("A Contributor", MarcRelatorCode(*b"ctb")).unwrap(),
        ];
        curation::set_record(&db, &hash, &record).unwrap();
        for (entry_id, include_author) in [("with-author", true), ("without-author", false)] {
            let mut external = record.clone();
            if !include_author {
                external.contributors.retain(|credit| credit.role_code() != AUTHOR_MARC_RELATOR_CODE);
            }
            store_entry(
                &db,
                &Entry {
                    source: "test".into(), entry_id: entry_id.into(), entry_url: format!("https://source.example/{entry_id}"),
                    license: "source-only".into(), record: external, file_url: None, format: BookFormat::Epub,
                    toc: Vec::new(), source_evidence: None, enriched: false,
                },
            ).unwrap();
        }
        let out = dir.path().join("published");
        publish(dir.path(), &db, &out, 1).unwrap();
        let snapshot = Connection::open(out.join("releases/1/catalog.sqlite3")).unwrap();
        let names = snapshot.prepare("SELECT name FROM author").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(names, ["An Author"]);
        let counts = snapshot.prepare("SELECT external,count(author_row_id) FROM book LEFT JOIN book_author ON book.row_id=book_author.book_row_id GROUP BY book.row_id ORDER BY external,count(author_row_id)")
            .unwrap().query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(counts, [(0, 1), (1, 0), (1, 1)]);
        assert_eq!(curation::record(&db, &hash).unwrap().unwrap().contributors, record.contributors);
    }

    #[test]
    fn publication_omits_navigation_and_preserves_curated_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let (hash, record) = fixture(dir.path(), &db);
        let toc = vec![BookTocEntry { title: "Part one".into(), target: "text/part.xhtml".into(), children: vec![BookTocEntry { title: "Chapter one".into(), target: "text/part.xhtml#chapter-1".into(), children: Vec::new() }] }];
        curation::set_toc(&db, &hash, &toc).unwrap();
        let manifest = dir.path().join("links.json");
        let entry = serde_json::json!([{"source": "litteraturbanken.se", "entry_id": "lb-test", "entry_url": "https://litteraturbanken.se/book/info", "file_url": "https://litteraturbanken.se/book.epub", "license": "lb-assv", "format": "Epub", "record": record, "toc": toc}]);
        std::fs::write(&manifest, serde_json::to_vec(&entry).unwrap()).unwrap();
        crate::links::import(&db, &reqwest::blocking::Client::new(), &manifest, false).unwrap();
        curation::set_record(&db, &hash, &record).unwrap();
        let out = dir.path().join("published");
        publish(dir.path(), &db, &out, 1).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(dir.path().join(format!("books/{hash}.epub"))).unwrap().ino(), std::fs::metadata(out.join(format!("books/{hash}.epub"))).unwrap().ino());
        }
        let snapshot = Connection::open(out.join("releases/1/catalog.sqlite3")).unwrap();
        let (title, year, description): (String, Option<i32>, String) =
            snapshot.query_row("SELECT title,year,description.text FROM book JOIN description ON description.row_id=book.description_id WHERE key=?1 AND external=0", [&hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(title, record.title);
        assert_eq!(year, record.book.year);
        assert_eq!(description, record.description);
        assert_eq!(curation::toc(&db, &hash).unwrap(), toc);
        for table in ["book", "external_book"] {
            let count: i64 = snapshot.query_row("SELECT count(*) FROM pragma_table_info(?1) WHERE name='toc'", [table], |row| row.get(0)).unwrap();
            assert_eq!(count, 0);
        }
        let format: u32 = snapshot.query_row("SELECT format_version FROM snapshot", [], |row| row.get(0)).unwrap();
        assert_eq!(format, FORMAT_VERSION);
        let (url, title, description): (String, String, String) = snapshot.query_row("SELECT entry_url,title,description FROM external_book", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(url, "https://litteraturbanken.se/book/info");
        let (format, download): (i64, Option<String>) = snapshot.query_row("SELECT format,download_url FROM external_book", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(format, BookFormat::Epub as i64);
        assert!(download.is_none());
        assert_eq!(title, record.title);
        assert_eq!(description, record.description);
        assert_eq!(std::fs::read_dir(out.join("books")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(out.join("covers")).unwrap().count(), 0);
        assert!(publish(dir.path(), &db, &out, 1).unwrap_err().contains("immutable"));
    }

    #[test]
    fn failed_snapshot_write_can_be_retried_without_a_half_release() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        fixture(dir.path(), &db);
        let out = dir.path().join("published");
        let blocked = out.join("releases/1/catalog.sqlite3.partial");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(publish(dir.path(), &db, &out, 1).is_err());
        assert!(!out.join("releases/1/catalog.sqlite3").exists());
        std::fs::remove_dir(blocked).unwrap();
        publish(dir.path(), &db, &out, 1).unwrap();
        assert!(out.join("releases/1/catalog.sqlite3").exists());
    }

    #[test]
    fn publication_preserves_source_evidence_and_external_audio_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let record = BookRecord { metadata: BookDetails { title: "Audio book".into(), ..Default::default() }, description: "Source description".into() };
        let page = "https://www.loyalbooks.com/book/book";
        let download = "https://archive.org/download/book/audio.zip";
        let key = store_entry(
            &db,
            &Entry {
                source: "loyalbooks.com".into(),
                entry_id: "book".into(),
                entry_url: page.into(),
                license: "source-declares-free".into(),
                record: record.clone(),
                file_url: Some("http://www.archive.org/download//book/audio.zip".into()),
                format: BookFormat::Mp3Folder,
                toc: Vec::new(),
                source_evidence: None,
                enriched: false,
            },
        )
        .unwrap();
        let metadata = r#"{"rating":5,"rating_count":2,"popularity":[{"rank":7}],"audiobook_delivery":"external_download"}"#;
        db.execute("INSERT INTO source_entry(source,entry_id,entry_url,external_entry_key,metadata) VALUES('loyalbooks.com','book',?1,?2,?3)", params![page, key, metadata]).unwrap();
        for size in ["card", "detail"] {
            std::fs::write(dir.path().join(format!("covers/{key}-{size}.jpg")), format!("retained {size} image")).unwrap();
        }
        let out = dir.path().join("published");
        publish(dir.path(), &db, &out, 1).unwrap();
        let snapshot = Connection::open(out.join("releases/1/catalog.sqlite3")).unwrap();
        let (source, stored_page, stored_download, stored_score, stored_title): (String, String, String, Option<f64>, String) = snapshot.query_row("SELECT source.name,source_entry.entry_url,external_book.download_url,source_entry.score,external_book.title FROM source_entry JOIN source ON source.id=source_entry.source_id JOIN external_book ON external_book.row_id=source_entry.external_row_id", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert_eq!(source, "loyalbooks.com");
        assert_eq!(stored_page, page);
        assert_eq!(stored_download, download);
        let format: i64 = snapshot.query_row("SELECT format FROM external_book", [], |row| row.get(0)).unwrap();
        assert_eq!(format, BookFormat::Mp3Folder as i64);
        assert_eq!(stored_score, Some(2.0 / 3.0));
        for size in ["card", "detail"] {
            assert_eq!(std::fs::read(out.join(format!("covers/{key}-{size}.jpg"))).unwrap(), format!("retained {size} image").as_bytes());
        }
        assert_eq!(stored_title, record.title);
        assert_eq!(std::fs::read_dir(out.join("books")).unwrap().count(), 0);
    }

    #[test]
    fn audio_format_is_preserved_without_permission_to_offer_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let record = BookRecord { metadata: BookDetails { title: "Source-only audio".into(), ..Default::default() }, description: String::new() };
        store_entry(
            &db,
            &Entry {
                source: "source.example".into(),
                entry_id: "audio".into(),
                entry_url: "https://source.example/audio".into(),
                license: "source-only".into(),
                record,
                file_url: None,
                format: BookFormat::Mp3Folder,
                toc: Vec::new(),
                source_evidence: None,
                enriched: false,
            },
        )
        .unwrap();
        let out = dir.path().join("published");
        publish(dir.path(), &db, &out, 1).unwrap();
        let snapshot = Connection::open(out.join("releases/1/catalog.sqlite3")).unwrap();
        let (format, download): (i64, Option<String>) = snapshot.query_row("SELECT format,download_url FROM external_book", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(format, BookFormat::Mp3Folder as i64);
        assert!(download.is_none());
        assert_eq!(std::fs::read_dir(out.join("books")).unwrap().count(), 0);
    }

    #[test]
    fn legacy_external_formats_are_backfilled_from_source_evidence_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        for (id, format) in [("epub", BookFormat::Epub), ("audio", BookFormat::Mp3Folder)] {
            let record = BookRecord { metadata: BookDetails { title: id.into(), ..Default::default() }, description: String::new() };
            let key = store_entry(
                &db,
                &Entry { source: "test".into(), entry_id: id.into(), entry_url: "https://source.example/book".into(), license: "source-only".into(), record, file_url: None, format, toc: Vec::new(), source_evidence: None, enriched: false },
            )
            .unwrap();
            if format == BookFormat::Mp3Folder {
                db.execute("INSERT INTO source_entry(source,entry_id,entry_url,external_entry_key,metadata) VALUES('test',?1,'https://source.example/book',?2,?3)", params![id, key, r#"{"audiobook_delivery":"external_download"}"#]).unwrap();
            }
        }
        db.execute_batch("ALTER TABLE external_book DROP COLUMN format;").unwrap();
        drop(db);
        let db = curation::open(dir.path()).unwrap();
        let formats = || db.prepare("SELECT entry_id,format FROM external_book ORDER BY entry_id").unwrap().query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(formats(), [("audio".into(), 16), ("epub".into(), 1)]);
        db.execute("UPDATE external_book SET format=2 WHERE entry_id='epub'", []).unwrap();
        db.execute("UPDATE external_book SET format=1 WHERE entry_id='audio'", []).unwrap();
        let reopened = curation::open(dir.path()).unwrap();
        assert_eq!(reopened.query_row("SELECT format FROM external_book WHERE entry_id='epub'", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(reopened.query_row("SELECT format FROM external_book WHERE entry_id='audio'", [], |row| row.get::<_, i64>(0)).unwrap(), 16);
    }

    #[test]
    fn legacy_curation_is_backfilled_once_without_replacing_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = Connection::open(dir.path().join("curation.sqlite3")).unwrap();
        legacy.execute_batch("CREATE TABLE book(hash TEXT PRIMARY KEY,title TEXT NOT NULL,subtitle TEXT,author TEXT NOT NULL,record TEXT NOT NULL) STRICT;").unwrap();
        drop(legacy);
        let db = curation::open(dir.path()).unwrap();
        let (hash, record) = fixture(dir.path(), &db);
        let path = dir.path().join(format!("books/{hash}.epub"));
        let expected = book_inspection::inspect_epub_metadata_with_toc(&path).unwrap().toc;
        assert!(!expected.is_empty());
        curation::ensure_toc(dir.path(), &db).unwrap();
        let out = dir.path().join("published");
        publish(dir.path(), &db, &out, 1).unwrap();
        assert_eq!(curation::toc(&db, &hash).unwrap(), expected);
        assert_eq!(curation::record(&db, &hash).unwrap(), Some(record));
        std::fs::remove_file(path).unwrap();
        curation::ensure_toc(dir.path(), &db).unwrap();
        // [] is complete too, and must not trigger repeated extraction.
        curation::set_toc(&db, &hash, &[]).unwrap();
        curation::ensure_toc(dir.path(), &db).unwrap();
        assert!(curation::toc(&db, &hash).unwrap().is_empty());
    }

    #[test]
    fn backfill_rejects_a_file_that_differs_from_its_catalog_hash() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let (hash, _) = fixture(dir.path(), &db);
        std::fs::write(dir.path().join(format!("books/{hash}.epub")), b"different bytes").unwrap();
        assert!(curation::ensure_toc(dir.path(), &db).unwrap_err().contains("differs from the catalog's hash"));
        assert!(curation::toc(&db, &hash).unwrap_err().contains("not been extracted"));
    }
}
