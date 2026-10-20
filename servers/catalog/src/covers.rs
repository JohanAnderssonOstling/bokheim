//! Retained cover images for hosted books and source-linked entries.
use crate::{cover_from_book, cover_from_image, get, manifest::verify_standardebooks_identity};
use reqwest::{blocking::Client, Url};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    source: String,
    entry_id: String,
    image_url: String,
    /// Select a specific hosted or external entry when a source ID names both.
    #[serde(default)]
    target_key: Option<String>,
    /// An already downloaded image; relative paths resolve beside the manifest.
    #[serde(default)]
    file_path: Option<PathBuf>,
}

/// Stage source artwork for an existing catalog without changing its database,
/// EPUBs or retained images. A receipt is written only after every book succeeds.
pub fn extract_standardebooks(dir: &Path, output: &Path) -> Result<(), String> {
    let db = Connection::open_with_flags(dir.join("curation.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    let mut statement = db.prepare("SELECT DISTINCT book_hash,entry_id FROM source_entry WHERE source='standardebooks.org' AND book_hash IS NOT NULL ORDER BY book_hash,entry_id").map_err(|error| error.to_string())?;
    let books = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    if books.is_empty() {
        return Err("catalog has no hosted Standard Ebooks acquisitions".into());
    }
    fs::create_dir(output).map_err(|error| format!("{}: {error}", output.display()))?;
    fs::create_dir(output.join("covers")).map_err(|error| error.to_string())?;
    let mut receipt = Vec::new();
    for (index, (hash, identity)) in books.iter().enumerate() {
        blake3::Hash::from_hex(hash).map_err(|error| error.to_string())?;
        let bytes = fs::read(dir.join("books").join(format!("{hash}.epub"))).map_err(|error| format!("{hash}: {error}"))?;
        if content_address::hash_reader(&mut &bytes[..]).map_err(|error| error.to_string())?.to_hex() != *hash {
            return Err(format!("catalog EPUB does not match its content hash: {hash}"));
        }
        verify_standardebooks_identity(&bytes, identity)?;
        if !cover_from_book(output, hash, &bytes)? {
            return Err(format!("{identity}: EPUB has no embedded cover"));
        }
        receipt.push(json!({"hash":hash,"entry_id":identity}));
        if (index + 1) % 75 == 0 {
            eprintln!("extracted {} / {} Standard Ebooks covers", index + 1, books.len());
        }
    }
    fs::write(output.join("standardebooks-covers.json"), serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    println!("{} Standard Ebooks covers extracted; source catalog unchanged", books.len());
    Ok(())
}

/// Build versioned card derivatives from retained detail art. The source and
/// its evidence remain intact, so publishing never compounds an earlier crop.
pub fn generate_cards(dir: &Path, key: &str) -> Result<(), String> {
    let source = dir.join("covers").join(format!("{key}-detail.jpg"));
    if !source.exists() {
        return Ok(());
    }
    let widths = [thumbnail::BROWSE_THUMBNAIL_WIDTH, thumbnail::THUMBNAIL_WIDTH];
    let paths = widths.map(|width| dir.join("covers").join(format!("{key}-{}.jpg", thumbnail::card_thumbnail_suffix(width))));
    if paths.iter().all(|path| path.exists()) {
        return Ok(());
    }
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    let format = image::guess_format(&bytes).map_err(|error| error.to_string())?;
    let image = thumbnail::decode_cover_image(&bytes, format).map_err(|error| error.to_string())?;
    for (width, path) in widths.into_iter().zip(paths) {
        if path.exists() {
            continue;
        }
        let bytes = thumbnail::encode_card_thumbnail_at_width(&image, width).map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn retain(dir: &Path, db: &Connection, http: &Client, manifest: &Path) -> Result<(), String> {
    db.busy_timeout(Duration::from_secs(30)).map_err(|e| e.to_string())?;
    let entries: Vec<Entry> = serde_json::from_slice(&fs::read(manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    // Resolve every identity before fetching or writing any images.
    let mut jobs = Vec::new();
    for entry in entries {
        let url = Url::parse(&entry.image_url).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err("cover URL must use HTTP or HTTPS without credentials".into());
        }
        let target = resolve_target(db, &entry)?;
        let (key, page, external) = target.ok_or_else(|| format!("unknown cover entry: {} {}", entry.source, entry.entry_id))?;
        blake3::Hash::from_hex(&key).map_err(|e| e.to_string())?;
        if !seen.insert(key.clone()) {
            return Err("duplicate cover manifest target".into());
        }
        jobs.push((entry, url, key, page, external));
    }
    let mut evidence = Vec::new();
    for (index, (entry, url, key, _, _)) in jobs.iter().enumerate() {
        let bytes = match &entry.file_path {
            Some(path) => fs::read(manifest.parent().unwrap_or(Path::new(".")).join(path)).map_err(|e| e.to_string())?,
            None => get(http, url)?,
        };
        cover_from_image(dir, key, &bytes)?;
        let cover = json!({"source_url": entry.image_url, "image_blake3": blake3::hash(&bytes).to_hex().to_string(), "image_bytes": bytes.len()});
        evidence.push(cover);
        if (index + 1) % 75 == 0 {
            eprintln!("retained {} / {} covers", index + 1, jobs.len());
        }
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    for ((entry, _, key, page, external), cover) in jobs.iter().zip(evidence) {
        record_evidence(&tx, &entry.source, &entry.entry_id, page, key, *external, cover)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    println!("{} cover images retained; no book files downloaded", jobs.len());
    Ok(())
}

fn resolve_target(db: &Connection, entry: &Entry) -> Result<Option<(String, String, bool)>, String> {
    db.query_row(
        "SELECT entry_key,entry_url,1 FROM external_book WHERE source=?1 AND entry_id=?2 AND (?4 IS NULL OR entry_key=?4) UNION ALL SELECT hash,coalesce(entry_url,?3),0 FROM book_source WHERE source=?1 AND entry_id=?2 AND (?4 IS NULL OR hash=?4) LIMIT 1",
        params![entry.source, entry.entry_id, entry.image_url, entry.target_key],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn record_evidence(db: &Connection, source: &str, entry_id: &str, page: &str, key: &str, external: bool, cover: Value) -> Result<(), String> {
    db.execute(
        "INSERT INTO source_entry(source,entry_id,entry_url,book_hash,external_entry_key,metadata) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source,entry_id) DO UPDATE SET metadata=json_set(source_entry.metadata,'$.cover',json_extract(excluded.metadata,'$.cover'))",
        params![source, entry_id, page, (!external).then_some(key), external.then_some(key), json!({"cover": cover}).to_string()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipWriter};

    pub(crate) fn standardebooks_epub(identity: &str) -> Vec<u8> {
        let mut image = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(300, 450, image::Rgb([25, 80, 145]))).write_to(&mut image, image::ImageFormat::Png).unwrap();
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        let package = format!(
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0" unique-identifier="book-id"><metadata><dc:identifier id="book-id">{identity}</dc:identifier><dc:title>A source work</dc:title><dc:creator>A writer</dc:creator><dc:language>en</dc:language><meta name="cover" content="cover"/></metadata><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#
        );
        for (name, bytes) in [
            ("mimetype", b"application/epub+zip".as_slice()),
            ("META-INF/container.xml", br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>"#.as_slice()),
            ("book.opf", package.as_bytes()),
            ("cover.png", image.get_ref().as_slice()),
            ("chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><p>A source work.</p></body></html>"#.as_slice()),
        ] {
            archive.start_file(name, SimpleFileOptions::default()).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn standardebooks_repair_stages_only_verified_source_books_and_preserves_originals() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::curation::open(dir.path()).unwrap();
        let identity = "https://standardebooks.org/ebooks/a-writer/a-source-work";
        let bytes = standardebooks_epub(identity);
        let hash = content_address::hash_reader(&mut &bytes[..]).unwrap().to_hex();
        let record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        crate::curation::set_record(&db, &hash, &record).unwrap();
        crate::curation::set_record(&db, "unrelated", &record).unwrap();
        fs::write(dir.path().join(format!("books/{hash}.epub")), &bytes).unwrap();
        fs::write(dir.path().join(format!("covers/{hash}-card.jpg")), b"previous chosen cover").unwrap();
        record_evidence(&db, "standardebooks.org", identity, identity, &hash, false, json!({})).unwrap();
        record_evidence(&db, "publicdomainlibrary.org", "other", "https://example.org/other", "unrelated", false, json!({})).unwrap();
        let output = dir.path().join("staged");
        extract_standardebooks(dir.path(), &output).unwrap();
        let receipt: Value = serde_json::from_slice(&fs::read(output.join("standardebooks-covers.json")).unwrap()).unwrap();
        assert_eq!(receipt, json!([{"hash":hash,"entry_id":identity}]));
        let expected = thumbnail::generate_thumbnail_versions_with_pdf_renderer("epub", Cursor::new(bytes.clone()), thumbnail::render_pdf_cover).unwrap().unwrap();
        assert_eq!(fs::read(output.join(format!("covers/{hash}-card.jpg"))).unwrap(), expected.browse);
        assert_eq!(fs::read(output.join(format!("covers/{hash}-detail.jpg"))).unwrap(), expected.high_density);
        assert_eq!(fs::read(dir.path().join(format!("covers/{hash}-card.jpg"))).unwrap(), b"previous chosen cover");
        assert_eq!(fs::read(dir.path().join(format!("books/{hash}.epub"))).unwrap(), bytes);
        assert_eq!(fs::read_dir(output.join("covers")).unwrap().count(), 4);
        assert!(extract_standardebooks(dir.path(), &output).is_err());
        db.execute("UPDATE source_entry SET entry_id='https://standardebooks.org/ebooks/another-work' WHERE source='standardebooks.org'", []).unwrap();
        let rejected = dir.path().join("rejected");
        assert!(extract_standardebooks(dir.path(), &rejected).is_err());
        assert!(!rejected.join("standardebooks-covers.json").exists());
    }

    #[test]
    fn recovering_one_derivative_preserves_the_existing_card() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("covers")).unwrap();
        let versions = thumbnail::ThumbnailVersions::from_image(&image::RgbImage::new(8, 8)).unwrap();
        crate::store_cover(dir.path(), "retained", versions).unwrap();
        let retained = dir.path().join(format!("covers/retained-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH)));
        let missing = dir.path().join(format!("covers/retained-{}.jpg", thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH)));
        fs::write(&retained, b"retained card bytes").unwrap();
        fs::remove_file(&missing).unwrap();
        generate_cards(dir.path(), "retained").unwrap();
        assert_eq!(fs::read(retained).unwrap(), b"retained card bytes");
        assert!(missing.is_file());
    }
    use book_model::{BookDetails, BookFormat, BookRecord};

    #[test]
    fn explicit_cover_target_distinguishes_hosted_and_external_entries() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE external_book(entry_key TEXT,source TEXT,entry_id TEXT,entry_url TEXT); CREATE TABLE book_source(hash TEXT,source TEXT,entry_id TEXT,entry_url TEXT); INSERT INTO external_book VALUES('external','test','one','https://example.org/external'); INSERT INTO book_source VALUES('hosted','test','one','https://example.org/hosted'); INSERT INTO book_source VALUES('unrelated','test','two','https://example.org/two');").unwrap();
        let mut entry = Entry { source: "test".into(), entry_id: "one".into(), image_url: "https://example.org/cover.jpg".into(), target_key: None, file_path: None };
        assert_eq!(resolve_target(&db, &entry).unwrap().unwrap(), ("external".into(), "https://example.org/external".into(), true));
        entry.target_key = Some("hosted".into());
        assert_eq!(resolve_target(&db, &entry).unwrap().unwrap(), ("hosted".into(), "https://example.org/hosted".into(), false));
        entry.target_key = Some("external".into());
        assert!(resolve_target(&db, &entry).unwrap().unwrap().2);
        entry.target_key = Some("unrelated".into());
        assert!(resolve_target(&db, &entry).unwrap().is_none());
    }

    #[test]
    fn cover_evidence_preserves_source_metadata_and_link_identity() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::curation::open(dir.path()).unwrap();
        // Evidence can identify an external entry independently of a content hash.
        let key = crate::links::store_entry(
            &db,
            &crate::links::Entry {
                source: "test".into(),
                entry_id: "one".into(),
                entry_url: "https://example.org/book".into(),
                license: "source".into(),
                record: BookRecord { metadata: BookDetails { title: "Linked book".into(), ..Default::default() }, description: String::new() },
                file_url: None,
                format: BookFormat::Epub,
                toc: Vec::new(),
                source_evidence: None,
                enriched: false,
            },
        )
        .unwrap();
        db.execute("INSERT INTO source_entry(source,entry_id,entry_url,external_entry_key,metadata) VALUES('test','one','https://example.org/book',?1,?2)", params![key, r#"{"popularity":12,"description":"Source text"}"#]).unwrap();
        record_evidence(&db, "test", "one", "https://example.org/book", &key, true, json!({"source_url":"https://example.org/cover.png"})).unwrap();
        let (hash, entry, raw): (Option<String>, String, String) = db.query_row("SELECT book_hash,external_entry_key,metadata FROM source_entry", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        let metadata: Value = serde_json::from_str(&raw).unwrap();
        assert!(hash.is_none());
        assert_eq!(entry, key);
        assert_eq!(metadata["popularity"], 12);
        assert_eq!(metadata["description"], "Source text");
        assert_eq!(metadata["cover"]["source_url"], "https://example.org/cover.png");
        assert_eq!(fs::read_dir(dir.path().join("books")).unwrap().count(), 0);
    }
}
