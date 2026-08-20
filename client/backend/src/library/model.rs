use crate::ContentHash;
use library_model::BookFormat;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

#[cfg(test)]
use super::covers::generate_thumbnail_for_sync;

pub use app_contract::library::*;

pub trait BookReader: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> BookReader for T {}
pub type BoxedBookReader = Box<dyn BookReader>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderFormat {
    Epub,
    Pdf,
    Cbz,
    WebPub,
    Audiobook,
    Materialized,
}

impl ReaderFormat {
    pub(crate) const fn from_book_format(format: BookFormat) -> Self {
        match format {
            BookFormat::Epub => Self::Epub,
            BookFormat::Pdf => Self::Pdf,
            BookFormat::Cbz => Self::Cbz,
            BookFormat::WebPub => Self::WebPub,
            BookFormat::M4b => Self::Audiobook,
            BookFormat::Mobi | BookFormat::Fb2 | BookFormat::Html => Self::Materialized,
        }
    }
}

pub struct ResolvedBook {
    pub path: PathBuf,
    /// A logical path carrying the authoritative synchronized format. The
    /// underlying local or remote cache path may be content-addressed and have
    /// no extension, so publication adapters must dispatch with this path.
    pub publication_path: PathBuf,
    pub format: ReaderFormat,
    pub reader: BoxedBookReader,
}

pub(crate) struct LibraryAccountState {
    pub user_id: Option<String>,
    pub bound_user_id: Option<String>,
    pub signed_in: bool,
}

/// Canonical content identity: full-file BLAKE3 for every asset.
pub(crate) fn generate_content_hash_blake3(path: &Path, _buf: &mut [u8]) -> std::io::Result<ContentHash> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize().to_hex().to_string();
    Ok(ContentHash::new(&hash))
}

#[cfg(test)]
mod tests {
    use super::{generate_content_hash_blake3, generate_thumbnail_for_sync, AuthorSort, AuthorSummary, Dir, FolderSort};
    use crate::app::{AppBackend, AppDataLocation, BackendContext};
    use crate::library::database as db;
    use crate::library::LibraryBackend;
    use crate::{ContentHash, DownloadState};
    use image::{DynamicImage, ImageOutputFormat};
    use library_model::{agent_id_from_migration_seed, AuthorId};
    use lopdf::{dictionary, Document, Object, Stream};
    use std::io::{Cursor, Write};
    use std::path::Path;
    use test_support::{fixture_content_hash, fixture_dir_id, fixture_library_id};
    use zip::write::SimpleFileOptions;

    fn write_test_pdf(path: &Path) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let contents_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 500.into()],
            "Resources" => dictionary! {},
            "Contents" => contents_id,
        });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("ID", Object::Array(vec![Object::string_literal(vec![1_u8; 16]), Object::string_literal(vec![2_u8; 16])]));
        document.save(path).unwrap();
    }

    fn open_test_library(app_data: &Path, library_dir: &Path, library_id: &str) -> LibraryBackend {
        let app = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.to_path_buf())).unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.to_path_buf())).unwrap();
        LibraryBackend::open(&app, &context, library_dir.to_str().unwrap(), fixture_library_id(library_id)).unwrap().0
    }

    fn imported_card(library: &LibraryBackend, content_hash: ContentHash) -> db::BookCardRow {
        let conn = library.db.get().unwrap();
        db::get_book_cards_in_dir_scoped(&conn, "00000000-0000-0000-0000-000000000000", false, "", app_contract::library::LibraryFileTypeFilter::All).unwrap().into_iter().find(|card| card.content_hash == content_hash).unwrap()
    }

    #[test]
    fn generated_content_hash_is_the_book_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("book.epub");
        std::fs::write(&path, b"book bytes").unwrap();
        let mut buffer = [0_u8; 16];

        let content_hash = generate_content_hash_blake3(&path, &mut buffer).unwrap();

        assert_eq!(content_hash, ContentHash::new(&blake3::hash(b"book bytes").to_hex().as_str()));
    }

    #[test]
    fn generated_content_hash_reports_missing_files() {
        let mut buffer = [0_u8; 16];
        assert!(generate_content_hash_blake3(Path::new("definitely-missing.epub"), &mut buffer).is_err());
    }

    #[test]
    fn pdf_import_indexes_thumbnail_and_resume_page() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let library = open_test_library(&app_data, &library_dir, "pdf-test");

        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "paper.pdf").unwrap();
        assert_eq!(imported_card(&library, content_hash).title, "paper");
        assert!(library.resolve_book_for_reader(content_hash).unwrap().path.ends_with("paper.pdf"));
        let conn = library.db.get().unwrap();
        db::update_pdf_reading_page(&conn, content_hash, 0).unwrap();
        assert_eq!(db::get_pdf_reading_page(&conn, content_hash).unwrap(), Some(0));
        assert!(app_data.join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{content_hash}.jpg")).exists());
    }

    #[test]
    fn opening_uses_an_existing_local_instance_instead_of_materializing_a_relationship_path() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let library = open_test_library(&app_data, &library_dir, "existing-instance-test");
        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "paper.pdf").unwrap();
        let existing_path = library_dir.join("paper.pdf");
        let missing_dir_id = fixture_dir_id("missing-instance");
        let missing_path = library_dir.join("Missing").join("paper.pdf");

        {
            let conn = library.db.get().unwrap();
            db::upsert_blob_local_ref(&conn, crate::BlobKind::Book, &content_hash, existing_path.to_str().unwrap(), true).unwrap();
            conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, ?2, 'Missing')", rusqlite::params![missing_dir_id.to_string(), sync_common::ROOT_DIR_ID.to_string()]).unwrap();
            conn.execute("UPDATE book_dir SET deleted_at=1 WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES (?1, (SELECT row_id FROM book WHERE content_hash=?2), 'paper.pdf', '', 0)",
                rusqlite::params![missing_dir_id.to_string(), content_hash.as_str()],
            )
            .unwrap();
        }

        let resolved = library.resolve_book_for_reader(content_hash).unwrap();
        assert_eq!(resolved.path, existing_path.canonicalize().unwrap());
        assert!(!missing_path.exists());
    }

    #[test]
    fn opening_dispatches_from_book_format_when_local_path_has_no_extension() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let library = open_test_library(&app_data, &library_dir, "extensionless-instance-test");
        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "paper.pdf").unwrap();
        let extensionless_path = library_dir.join(content_hash.as_str());
        std::fs::rename(library_dir.join("paper.pdf"), &extensionless_path).unwrap();
        db::upsert_blob_local_ref(&library.db.get().unwrap(), crate::BlobKind::Book, &content_hash, extensionless_path.to_str().unwrap(), true).unwrap();

        let resolved = library.resolve_book_for_reader(content_hash).unwrap();

        assert_eq!(resolved.path, extensionless_path);
        assert_eq!(resolved.format, super::ReaderFormat::Pdf);
        assert_eq!(resolved.publication_path.extension().and_then(|extension| extension.to_str()), Some("pdf"));
    }

    #[test]
    fn local_copy_eviction_keeps_the_book_in_the_catalogue() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let library = open_test_library(&app_data, &library_dir, "eviction-test");

        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "paper.pdf").unwrap();
        let local_path = library.resolve_book_for_reader(content_hash).unwrap().path;
        assert!(local_path.exists());
        assert!(db::is_book_downloaded(&library.db.get().unwrap(), content_hash).unwrap());

        assert!(matches!(library.remove_local_book_copy(content_hash).unwrap(), DownloadState::NotDownloaded));
        assert!(!local_path.exists());
        assert!(db::get_book_cards_in_dir_scoped(&library.db.get().unwrap(), "00000000-0000-0000-0000-000000000000", false, "", app_contract::library::LibraryFileTypeFilter::All)
            .unwrap()
            .iter()
            .any(|candidate| candidate.content_hash == content_hash));
    }

    #[test]
    fn folder_copy_eviction_preserves_placements_outside_the_subtree() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let library = open_test_library(&app_data, &library_dir, "folder-eviction-test");
        let first = library.create_directory(&sync_common::ROOT_DIR_ID, "First").unwrap();
        let second = library.create_directory(&sync_common::ROOT_DIR_ID, "Second").unwrap();
        let content_hash = library.import_book(&first.id, &source, "paper.pdf").unwrap();
        library.copy_book_to_directory(&content_hash, &first.id, &second.id).unwrap();

        let status = library.remove_local_folder_copies(&first.id).unwrap();

        assert_eq!((status.downloaded_books, status.total_books), (0, 1));
        assert!(!library_dir.join("First/paper.pdf").exists());
        assert!(library_dir.join("Second/paper.pdf").is_file());
        let second_status = library.folder_download_status(&second.id).unwrap();
        assert_eq!((second_status.downloaded_books, second_status.total_books), (1, 1));
    }

    #[test]
    fn html_import_packages_reachable_assets_without_rewriting_sources() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("essay.html");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        let html = b"<!doctype html><html><head><title>A better title</title><meta name=\"author\" content=\"Ada Lovelace\"><link rel=\"stylesheet\" href=\"assets/book.css\"></head><body><img src=\"assets/cover.png\">Text</body></html>";
        std::fs::create_dir_all(temp.path().join("assets")).unwrap();
        std::fs::write(&source, html).unwrap();
        std::fs::write(temp.path().join("assets/book.css"), b"body { color: navy }").unwrap();
        std::fs::write(temp.path().join("assets/cover.png"), b"image bytes").unwrap();
        std::fs::write(temp.path().join("unrelated.txt"), b"private sibling").unwrap();
        let library = open_test_library(&app_data, &library_dir, "html-test");

        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "essay.html").unwrap();
        let stored = library.resolve_book_for_reader(content_hash).unwrap().path;
        assert!(stored.ends_with("essay.webpub"));
        assert_eq!(stored.extension().and_then(|extension| extension.to_str()), Some("webpub"));
        let publication = html_publication::PublicationSource::from_path(stored).unwrap();
        assert_eq!(publication.provider.read_bytes("essay.html").unwrap(), html);
        assert_eq!(publication.provider.read_bytes("assets/book.css").unwrap(), b"body { color: navy }");
        assert!(publication.provider.read_bytes("unrelated.txt").is_err());
        let card = imported_card(&library, content_hash);
        assert_eq!(card.title, "A better title");
        assert_eq!(card.author, "Ada Lovelace");
    }

    #[test]
    fn cbz_import_reads_comic_info_and_generates_its_cover() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("comic.cbz");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        let mut image = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 3).write_to(&mut image, ImageOutputFormat::Png).unwrap();
        let file = std::fs::File::create(&source).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("001.png", options).unwrap();
        archive.write_all(image.get_ref()).unwrap();
        archive.start_file("ComicInfo.xml", options).unwrap();
        archive.write_all(br#"<ComicInfo><Title>Native Comic</Title><Writer>Grace Hopper</Writer><Pages><Page Image="0" Type="FrontCover"/></Pages></ComicInfo>"#).unwrap();
        archive.finish().unwrap();
        let library = open_test_library(&app_data, &library_dir, "cbz-test");

        let content_hash = library.import_book(&sync_common::ROOT_DIR_ID, &source, "comic.cbz").unwrap();

        let card = imported_card(&library, content_hash);
        assert_eq!(card.title, "Native Comic");
        assert_eq!(card.author, "Grace Hopper");
        assert!(app_data.join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{content_hash}.jpg")).exists());
    }

    #[test]
    fn author_name_sort_is_case_insensitive_and_deterministic() {
        let mut authors = vec![
            AuthorSummary::try_new(agent_id_from_migration_seed(b"zelda"), "zelda".to_string(), None, 3, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Alice"), "Alice".to_string(), None, 1, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"alice"), "alice".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
        ];

        AuthorSort::Name.sort(&mut authors);

        assert_eq!(authors.iter().map(|author| author.name().as_str()).collect::<Vec<_>>(), vec!["Alice", "alice", "zelda"]);
    }

    #[test]
    fn author_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut authors = vec![
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Zelda"), "Zelda".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Alice"), "Alice".to_string(), None, 7, Vec::new(), Vec::new()).unwrap(),
            AuthorSummary::try_new(agent_id_from_migration_seed(b"Bob"), "Bob".to_string(), None, 2, Vec::new(), Vec::new()).unwrap(),
        ];

        AuthorSort::BookCount.sort(&mut authors);

        assert_eq!(authors.iter().map(|author| (author.name().as_str(), author.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }

    #[test]
    fn folder_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut directories = vec![Dir::new(fixture_dir_id("z"), "Zelda".to_owned(), 2, 0), Dir::new(fixture_dir_id("a"), "Alice".to_owned(), 7, 0), Dir::new(fixture_dir_id("b"), "Bob".to_owned(), 2, 0)];

        FolderSort::BookCount.sort(&mut directories);

        assert_eq!(directories.iter().map(|directory| (directory.name().as_str(), directory.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }

    #[test]
    fn thumbnail_generation_populates_ui_and_content_addressed_sync_caches_deterministically() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        let epub_path = library_dir.join("book.epub");

        let mut cover = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 3).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
        let file = std::fs::File::create(&epub_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored)).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#)
            .unwrap();
        archive.start_file("OEBPS/cover.png", options).unwrap();
        archive.write_all(cover.get_ref()).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", options).unwrap();
        archive.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>book</body></html>").unwrap();
        archive.finish().unwrap();

        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let library_root = library_dir.canonicalize().unwrap();
        let content_hash = ContentHash::new(&blake3::hash(&std::fs::read(&epub_path).unwrap()).to_hex().as_str());
        generate_thumbnail_for_sync(&context, &library_root, &epub_path, &content_hash).unwrap();

        let ui_bytes = std::fs::read(app_data.join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{content_hash}.jpg"))).unwrap();
        let sync_path = crate::sync::transfer::thumbnail_path_from_content_hash(&library_root, &content_hash);
        let sync_bytes = std::fs::read(&sync_path).unwrap();
        assert_eq!(&ui_bytes[..2], &[0xff, 0xd8]);
        assert_eq!(&sync_bytes[..3], &[0xff, 0xd8, 0xff]);
        let dimensions = image::load_from_memory_with_format(&sync_bytes, image::ImageFormat::Jpeg).unwrap().to_rgb8().dimensions();
        assert_eq!(dimensions, (300, 450));

        generate_thumbnail_for_sync(&context, &library_root, &epub_path, &content_hash).unwrap();
        assert_eq!(std::fs::read(sync_path).unwrap(), sync_bytes);
    }
}
