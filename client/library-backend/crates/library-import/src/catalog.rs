//! Adding a book downloaded over the network: a public-domain catalog book,
//! checked against the catalog's hash and with the catalog's record as its
//! enrichment answer, an external catalog audiobook from its source, or a
//! book from an OPDS feed, enriched like any import.

use client_runtime::{BackendError, Failure};
use library_database::catalog::CatalogBook;
use library_model::{CardKey, LibraryUpdate};
use library_placement::stage_import;
use library_runtime::events::LibraryEvent;
use sync_common::{BookId, DirId};

use crate::LibraryImport;

impl LibraryImport {
    pub async fn import_catalog_book(&self, parent_id: DirId, hash: BookId) -> Result<BookId, BackendError> {
        let book = library_database::catalog::entry(CardKey::from(hash)).map_err(BackendError::operation)?.filter(|book| book.hash.is_some()).ok_or_else(|| BackendError::classified(Failure::NotFound, "the book is not in the catalog"))?;
        let entry = CardKey::from(hash);
        let bytes = library_catalog::download_book_with_progress(hash, &|progress| self.events.publish_update(LibraryUpdate::CatalogImport { entry, progress })).await?;
        self.import_catalog_download(parent_id, &book, bytes).await
    }

    /// Downloads an external audiobook from its source and imports it with
    /// the catalog's record, as a hosted catalog book is.
    pub async fn import_external_catalog_audiobook(&self, parent_id: DirId, entry: CardKey) -> Result<BookId, BackendError> {
        let book = library_database::catalog::entry(entry).map_err(BackendError::operation)?.filter(|book| book.hash.is_none()).ok_or_else(|| BackendError::classified(Failure::NotFound, "the audiobook is not in the catalog"))?;
        let bytes = library_catalog::download_audiobook_with_progress(entry, &|progress| self.events.publish_update(LibraryUpdate::CatalogImport { entry, progress })).await?;
        let imported = self.import_catalog_download(parent_id, &book, bytes).await?;
        self.refresh_external_catalog_audiobook_cover(imported, entry).await?;
        Ok(imported)
    }

    /// Uses selected catalog artwork to replace all cached thumbnail sizes,
    /// including those of an audiobook already imported into the library.
    /// Audio files and their source artwork are never rewritten.
    pub async fn refresh_external_catalog_audiobook_cover(&self, hash: BookId, entry: CardKey) -> Result<bool, BackendError> {
        if self.database.thumbnail_book_format(hash).map_err(BackendError::operation)? != Some(book_model::BookFormat::Mp3Folder) {
            return Ok(false);
        }
        let Some(bytes) = library_catalog::fresh_external_cover(entry).await.map_err(BackendError::message)? else { return Ok(false) };
        let versions = cpu_host::cover(&*self.cpu, bytes).await?;
        // Fetch and CPU work may overlap removal of the book. Keep old valid
        // thumbnails until the replacement has been decoded successfully.
        if self.database.thumbnail_book_format(hash).map_err(BackendError::operation)? != Some(book_model::BookFormat::Mp3Folder) {
            return Ok(false);
        }
        self.assets.write_thumbnail_set(&hash, &versions.high_density, &versions.browse, &versions.card_high_density).await.map_err(BackendError::operation)?;
        self.database.complete_thumbnail_work(&hash, true).map_err(BackendError::operation)?;
        self.events.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash });
        Ok(true)
    }

    async fn import_catalog_download(&self, parent_id: DirId, book: &CatalogBook, bytes: Vec<u8>) -> Result<BookId, BackendError> {
        let name = if book.author.is_empty() { book.title.clone() } else { format!("{} - {}", book.author, book.title) };
        let extension = book.format().canonical_extension();
        let file_name = format!("{}.{extension}", name.replace(['/', '\\', ':'], " ").trim());
        let imported = self.import_downloaded_book(parent_id, &file_name, bytes).await?;
        self.database.record_catalog_answer(&imported, book.record.clone()).map_err(BackendError::operation)?;
        Ok(imported)
    }

    /// Imports a downloaded file as if it were a local one.
    pub async fn import_downloaded_book(&self, parent_id: DirId, file_name: &str, bytes: Vec<u8>) -> Result<BookId, BackendError> {
        let mut import = self.validate_import(&parent_id, file_name)?;
        let (name, staged) = stage_import(&self.assets, &self.database, parent_id, import.file_name.clone(), Box::new(std::io::Cursor::new(bytes)), Default::default()).await.map_err(BackendError::operation)?;
        import.file_name = name;
        self.finish_import(import, staged).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use book_model::{BookDetails, BookFormat, BookRecord};
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use library_database::catalog::import_record;
    use library_database::{Database, FilesystemScan, ImportCommit, InspectedFilesystemScan, ScanApplyResult, ScanFileObservation, ScannedBook};
    use library_files::AssetStore;
    use library_model::{BlobKind, PublishedStagedBook};
    use library_placement::LibraryPlacement;
    use library_runtime::{events::LibraryEventRouter, scheduler::SyncScheduler};
    use rusqlite::Connection;
    use std::{
        io::{Cursor, Read, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };
    use sync_common::ROOT_DIR_ID;

    fn picture(color: [u8; 3]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 60, Rgb(color))).write_to(&mut output, ImageFormat::Png).unwrap();
        output.into_inner()
    }

    #[tokio::test]
    async fn catalog_answers_follow_file_hashes_and_selected_audio_covers_preserve_files() {
        let root = tempfile::tempdir().unwrap();
        let database = Arc::new(Database::open(":memory:").unwrap());
        database.initialize_library().unwrap();
        let hash = BookId::new(&"a".repeat(64));
        let key = CardKey([8; 32]);
        let record = BookRecord { metadata: BookDetails { title: "Audio book".into(), ..Default::default() }, description: String::new() };
        database
            .commit_import(ImportCommit {
                parent_id: ROOT_DIR_ID,
                file_name: "Story".into(),
                format: BookFormat::Mp3Folder,
                content_hash: hash,
                size_bytes: 10,
                inspection_version: 1,
                inspection: Some(book_inspection::InspectedBook { metadata: record.clone(), pdf: None, audiobook: None, toc: Vec::new() }),
                published: PublishedStagedBook { name: "Story".into(), relative_path: "/Story".into(), published: false, fingerprint: None },
                request_thumbnail: false,
                restore_paths: Vec::new(),
            })
            .unwrap();
        let cpu = Arc::new(client_platform_native::cpu_host::NativeHost);
        let epub_path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../book-inspection/tests/fixtures/back-matter-evidence.epub"));
        let inspected = filesystem_scanner::ingestion::ingest_file(epub_path.clone(), "fixture.epub".into(), &*cpu).await.unwrap();
        let epub_hash = inspected.content_hash;
        let epub_bytes = std::fs::read(epub_path).unwrap();
        let catalog = Connection::open_in_memory().unwrap();
        catalog.execute_batch(include_str!("../../library-database/tests/sql/catalog_pagination.sql")).unwrap();
        catalog
            .execute_batch(
                "INSERT INTO snapshot VALUES(1,8,1);
                 INSERT INTO source VALUES(1,'example.org');
                 INSERT INTO description VALUES(1,''),(2,'Reviewed catalog description');
                 INSERT INTO subject_source VALUES(1,'manual:subject');
                 INSERT INTO subject_authority VALUES(1,'unified');
                 INSERT INTO subject VALUES(1,NULL,'Short Stories',NULL,1,1,'Literature / Genres / Literary Forms / Forms & Literary Traditions / Short Stories');",
            )
            .unwrap();
        catalog.execute("INSERT INTO book(row_id,key,external,title,description_id,source_id,entry_url,format) VALUES(1,?1,1,'Audio book',1,1,'https://example.org/book',16)", [&key.0[..]]).unwrap();
        catalog.execute("INSERT INTO book(row_id,key,external,title,description_id,source_id,format) VALUES(2,?1,0,'Curated fixture',2,1,1)", [epub_hash.as_bytes()]).unwrap();
        catalog.execute_batch("INSERT INTO book_source VALUES(1,1),(2,1); INSERT INTO book_subject VALUES(2,1);").unwrap();
        library_database::catalog::activate(catalog.serialize("main").unwrap().to_vec()).unwrap();

        let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let old = cpu_host::cover(&*cpu, picture([240, 20, 20])).await.unwrap();
        assets.write_thumbnail_set(&hash, &old.high_density, &old.browse, &old.card_high_density).await.unwrap();
        database.complete_thumbnail_work(&hash, true).unwrap();
        let story = root.path().join("Story");
        std::fs::create_dir(&story).unwrap();
        std::fs::write(story.join("01.mp3"), b"original audio and tags").unwrap();
        let (sync, _) = SyncScheduler::new();
        let (asset_work, _) = SyncScheduler::new();
        let (files, _) = SyncScheduler::new();
        let (enrichment, _) = SyncScheduler::new();
        let (events, _) = LibraryEventRouter::new(sync, asset_work, files, enrichment);
        let placement = Arc::new(LibraryPlacement { database: database.clone(), assets: assets.clone(), events: events.clone() });
        let importer = LibraryImport { library_id: DirId::new_v4(), cloud: Default::default(), provider_upload_lock: Default::default(), database: database.clone(), assets: assets.clone(), cpu: cpu.clone(), events, placement, root: root.path().to_str().unwrap().into() };
        // A renamed copy uses the catalog only when its actual bytes match.
        let added = importer.import_downloaded_book(ROOT_DIR_ID, "renamed.epub", epub_bytes.clone()).await.unwrap();
        assert_eq!(added, epub_hash);
        let answer = database.enrichment_cover_record(&added).unwrap().unwrap();
        assert_eq!(answer.metadata.title, inspected.inspection.metadata.metadata.title);
        assert_eq!(answer.book.subjects[0].source(), "manual:subject");
        assert_eq!(answer.book.subjects[0].authority(), Some("unified"));
        assert!(database.enrichment_candidates(&[added], i64::MAX).unwrap().is_empty());
        assert!(import_record(added, BookFormat::Pdf).unwrap().is_none());
        assert!(import_record(BookId::from_bytes(key.0), BookFormat::Mp3Folder).unwrap().is_none());
        // Discovery of an EPUB already on disk uses the same curated answer.
        let discovered = Database::open(":memory:").unwrap();
        discovered.initialize_library().unwrap();
        let mut snapshot = discovered.scan_snapshot().unwrap();
        let file = ScanFileObservation { directory: ROOT_DIR_ID, name: "discovered.epub".into(), path: "discovered.epub".into(), fingerprint: inspected.fingerprint };
        let discovery = FilesystemScan { readable: true, files: vec![file], ..Default::default() };
        let books = InspectedFilesystemScan {
            readable: true,
            books: vec![ScannedBook {
                directory: ROOT_DIR_ID,
                name: "discovered.epub".into(),
                path: "discovered.epub".into(),
                fingerprint: inspected.fingerprint,
                content_hash: added,
                size_bytes: inspected.size_bytes,
                extension: "epub".into(),
                inspection: inspected.inspection,
                inspection_version: book_inspection::inspection_version(BookFormat::Epub),
            }],
        };
        assert_eq!(discovered.apply_filesystem_scan_batch(&mut snapshot, &discovery, &books, true, None).unwrap(), ScanApplyResult::Applied);
        assert_eq!(discovered.enrichment_cover_record(&added).unwrap().unwrap().book.subjects, answer.book.subjects);
        assert!(discovered.enrichment_candidates(&[added], i64::MAX).unwrap().is_empty());
        // A valid EPUB with different bytes and the same embedded metadata must
        // still use ordinary enrichment rather than a title/author match.
        let mut different = epub_bytes.clone();
        let end = different.len();
        assert_eq!(&different[end - 22..end - 18], b"PK\x05\x06");
        let comment = b"different edition";
        different[end - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        different.extend_from_slice(comment);
        let other = importer.import_downloaded_book(ROOT_DIR_ID, "different.epub", different).await.unwrap();
        assert_ne!(other, added);
        assert_eq!(database.enrichment_candidates(&[other], i64::MAX).unwrap().len(), 1);
        assert!(database.enrichment_cover_record(&other).unwrap().is_none());
        assert_eq!(std::fs::read(root.path().join("renamed.epub")).unwrap(), epub_bytes);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        std::env::set_var("BOKHEIM_CATALOG_ORIGIN", format!("http://{}", listener.local_addr().unwrap()));
        let chosen = picture([20, 20, 240]);
        let server = thread::spawn(move || {
            for chosen in [chosen, b"invalid image".to_vec()] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let size = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..size]).starts_with(&format!("GET /covers/{key}-detail.jpg ")));
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", chosen.len()).unwrap();
                stream.write_all(&chosen).unwrap();
            }
        });
        assert!(importer.refresh_external_catalog_audiobook_cover(hash, key).await.unwrap());
        let master = assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap();
        assert_ne!(master, old.high_density);
        assert!(importer.refresh_external_catalog_audiobook_cover(hash, key).await.is_err());
        server.join().unwrap();
        assert_eq!(assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), master);
        let decoded = image::load_from_memory(&master).unwrap().to_rgb8();
        assert!(decoded.get_pixel(0, 0)[2] > 200);
        assert!(assets.thumbnail_set_exists(&hash).unwrap());
        assert_eq!(database.thumbnail_work_state(&hash).unwrap().as_deref(), Some("ready"));
        assert_eq!(std::fs::read(story.join("01.mp3")).unwrap(), b"original audio and tags");
        assert!(!story.join("cover.jpg").exists());
        // Verify the actual stored display variants, not just the decoder output.
        for (width, previous) in [(thumbnail::BROWSE_THUMBNAIL_WIDTH, old.browse), (thumbnail::THUMBNAIL_WIDTH, old.card_high_density)] {
            let name = library_files::asset_store::thumbnail_variant_name(&hash, width);
            let stored = assets.read_named(name).await.unwrap().unwrap();
            assert_ne!(stored, previous);
            let pixels = image::load_from_memory(&stored).unwrap().to_rgb8();
            assert!(pixels.get_pixel(0, 0)[2] > 200);
        }
    }
}
