use super::*;
use crate::crawl::{annotate_popular_listing, crawl_endpoint_groups, internal_page_links, prioritize_startup_crawls};
use crate::extraction::{extract_catalog_metadata, extract_epub_catalog_metadata, is_synthetic_cover_key, mobi_catalog_metadata, ExtractedCatalogData};
use axum::body::{to_bytes, Body};
use axum::http::Request;
use axum::routing::get;
use image::{GenericImageView, ImageOutputFormat};
use lopdf::dictionary;
use opds::{registry_from_endpoints, OpdsEndpointId};
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::ServiceExt;
use zip::write::SimpleFileOptions;

#[test]
fn adaptive_crawl_batch_grows_and_shrinks_with_commit_latency() {
    let mut batch = AdaptiveCrawlWriteBatch::default();
    assert_eq!(batch.page_limit, CRAWL_WRITE_BATCH_INITIAL_PAGES);
    batch.record_commit(CRAWL_WRITE_BATCH_FAST_COMMIT - Duration::from_millis(1));
    assert_eq!(batch.page_limit, CRAWL_WRITE_BATCH_MAX_PAGES);
    batch.record_commit(CRAWL_WRITE_BATCH_SLOW_COMMIT + Duration::from_millis(1));
    assert_eq!(batch.page_limit, CRAWL_WRITE_BATCH_INITIAL_PAGES);
    batch.record_commit(CRAWL_WRITE_BATCH_SLOW_COMMIT + Duration::from_millis(1));
    batch.record_commit(CRAWL_WRITE_BATCH_SLOW_COMMIT + Duration::from_millis(1));
    batch.record_commit(CRAWL_WRITE_BATCH_SLOW_COMMIT + Duration::from_millis(1));
    assert_eq!(batch.page_limit, CRAWL_WRITE_BATCH_MIN_PAGES);
    batch.record_commit(CRAWL_WRITE_BATCH_SLOW_COMMIT + Duration::from_millis(1));
    assert_eq!(batch.page_limit, CRAWL_WRITE_BATCH_MIN_PAGES);
    batch.started_at = Some(Instant::now() - CRAWL_WRITE_BATCH_MAX_AGE - Duration::from_millis(1));
    assert!(!batch.reached_capacity(), "replacement crawls ignore age and flush only at bounded capacity");
    assert!(batch.should_flush_incremental(), "incremental crawls retain their visibility deadline");
}

#[test]
fn origin_throttle_increases_cautiously_and_backs_off_aggressively() {
    let now = Instant::now();
    let mut state = AdaptiveOriginThrottleState { in_flight: 0, concurrency_limit: CRAWL_PAGE_CONCURRENCY, success_streak: 0, minimum_spacing: CRAWL_ORIGIN_INITIAL_SPACING, next_request_at: now, blocked_until: None, latency_ewma_ms: None };
    for _ in 0..CRAWL_ORIGIN_SUCCESS_WINDOW {
        state.record(OriginThrottleFeedback::Success(Duration::from_millis(100)), now);
    }
    assert_eq!(state.concurrency_limit, CRAWL_PAGE_CONCURRENCY + 1);
    assert!(state.minimum_spacing < CRAWL_ORIGIN_INITIAL_SPACING);

    state.record(OriginThrottleFeedback::Success(Duration::from_secs(2)), now);
    assert_eq!(state.concurrency_limit, CRAWL_PAGE_CONCURRENCY);
    assert!(state.minimum_spacing >= CRAWL_ORIGIN_INITIAL_SPACING);

    state.record(OriginThrottleFeedback::RateLimited(Some(Duration::from_secs(90))), now);
    assert_eq!(state.concurrency_limit, 1);
    assert!(state.minimum_spacing >= Duration::from_secs(1));
    assert!(state.blocked_until.is_some_and(|until| until >= now + Duration::from_secs(90)));
}

#[test]
fn transient_origin_failures_halve_concurrency_without_affecting_other_origins() {
    let now = Instant::now();
    let mut first = AdaptiveOriginThrottleState { in_flight: 0, concurrency_limit: CRAWL_PAGE_CONCURRENCY_MAX, success_streak: 0, minimum_spacing: CRAWL_ORIGIN_MIN_SPACING, next_request_at: now, blocked_until: None, latency_ewma_ms: None };
    let second_limit = CRAWL_PAGE_CONCURRENCY_MAX;
    first.record(OriginThrottleFeedback::TransientFailure, now);
    assert_eq!(first.concurrency_limit, CRAWL_PAGE_CONCURRENCY_MAX / 2);
    assert!(first.blocked_until.is_some_and(|until| until >= now + CRAWL_ORIGIN_TRANSIENT_BACKOFF));
    assert_eq!(second_limit, CRAWL_PAGE_CONCURRENCY_MAX);
}

fn endpoint(url: String) -> OpdsEndpoint {
    OpdsEndpoint { id: OpdsEndpointId::parse("test").unwrap(), title: "Test".to_owned(), description: "Test catalog".to_owned(), languages: vec!["Multilingual".to_owned()], url }
}

fn service(endpoint: OpdsEndpoint, directory: &Path) -> CatalogService {
    let registry = registry_from_endpoints(1, std::slice::from_ref(&endpoint));
    let store = CatalogStore::open(directory.join("catalog-cache.db")).unwrap();
    store.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).unwrap();
    for process in ["catalogue_crawling", "cover_downloads", "enrichment"] {
        store.set_process_enabled(process, true).unwrap();
    }
    let covers = CatalogCoverCache::new(directory.join("covers")).unwrap();
    CatalogService {
        state: Arc::new(CatalogServiceState {
            source: RegistrySource::Bundled,
            snapshot: RwLock::new(RegistrySnapshot { observed_bytes: serde_json::to_vec(&registry).unwrap(), registry }),
            endpoints: RwLock::new(vec![endpoint]),
            opds: OpdsClient::new().unwrap(),
            store,
            covers,
            cover_cache_directory: directory.join("covers"),
            store_write_lock: Mutex::new(()),
            crawl_locks: Mutex::new(HashMap::new()),
            crawl_progress: Mutex::new(HashMap::new()),
            origin_throttles: Mutex::new(HashMap::new()),
            source_update_locks: Mutex::new(HashMap::new()),
            epub_range_origins: Mutex::new(HashMap::new()),
            background_download_slots: Semaphore::new(BACKGROUND_DOWNLOAD_LIMIT),
            interactive_download_slots: Semaphore::new(MAX_INTERACTIVE_DOWNLOADS),
            queued_download_slots: Semaphore::new(MAX_QUEUED_DOWNLOADS),
            extraction_slots: Semaphore::new(MAX_CONCURRENT_EXTRACTIONS),
            extraction_notify: Notify::new(),
            processes: CatalogProcessRuntime::new(CatalogProcessSettings { catalogue_crawling: true, cover_downloads: true, enrichment: true }),
            acquisition_directory: directory.join("acquisitions"),
            metadata_endpoint: None,
            metadata_client: reqwest::Client::new(),
            metadata_backfill_metrics: MetadataBackfillRuntimeMetrics::default(),
            description_backfill_metrics: MetadataBackfillRuntimeMetrics::default(),
            extraction_metrics: CatalogExtractionRuntimeMetrics::default(),
            description_backfill_available: AtomicBool::new(false),
            writer_metrics: StdMutex::new(BTreeMap::new()),
            max_crawl_pages: DEFAULT_MAX_CRAWL_PAGES,
            started_at: Instant::now(),
        }),
    }
}

async fn wait_for_successful_extractions(service: &CatalogService, expected: u64) {
    service.resume_pending_epub_extractions().await;
    let state = service.state.store.inventory().unwrap().epub_extractions;
    assert!(
        state.succeeded >= expected,
        "EPUB extraction did not finish: pending={}, running={}, downloading={}, extracting={}, enriching={}, succeeded={}, failed={}",
        state.pending,
        state.running,
        state.downloading,
        state.extracting,
        state.enriching,
        state.succeeded,
        state.failed
    );
}

#[tokio::test]
async fn first_crawl_publishes_root_before_descendant_traversal_finishes() {
    let child_started = Arc::new(AtomicBool::new(false));
    let release_child = Arc::new(tokio::sync::Notify::new());
    let upstream =
        Router::new().route("/root", get(|| async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Root"},"navigation":[{"href":"/child","type":"application/opds+json","title":"Child"}]}"#) })).route(
            "/child",
            get({
                let child_started = child_started.clone();
                let release_child = release_child.clone();
                move || {
                    let child_started = child_started.clone();
                    let release_child = release_child.clone();
                    async move {
                        child_started.store(true, Ordering::SeqCst);
                        release_child.notified().await;
                        ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Child"},"navigation":[{"title":"Website","href":"/about","type":"text/html"}]}"#)
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let crawl_service = service.clone();
    let crawl_source = source_id.clone();
    let crawl_task = tokio::spawn(async move { crawl_service.crawl_source(&crawl_source).await });

    tokio::time::timeout(Duration::from_secs(1), async {
        while !child_started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let root = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(root.page.title, "Root");
    assert_eq!(service.state.store.inventory().unwrap().sources[0].crawl_status, "running");

    release_child.notify_waiters();
    crawl_task.await.unwrap().unwrap();
    let inventory = service.state.store.inventory().unwrap();
    assert_eq!(inventory.active_page_count, 2);
    assert_eq!(inventory.sources[0].crawl_status, "succeeded");
    upstream_task.abort();
}

#[tokio::test]
async fn source_crawl_overlaps_only_the_bounded_number_of_page_requests() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(CRAWL_PAGE_CONCURRENCY));
    let child = |active: Arc<AtomicUsize>, peak: Arc<AtomicUsize>, barrier: Arc<tokio::sync::Barrier>| {
        get(move || {
            let active = active.clone();
            let peak = peak.clone();
            let barrier = barrier.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                barrier.wait().await;
                active.fetch_sub(1, Ordering::SeqCst);
                ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Leaf"},"navigation":[{"title":"Website","href":"/about","type":"text/html"}]}"#)
            }
        })
    };
    let upstream = Router::new()
        .route(
            "/root",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/opds+json")],
                    r#"{"metadata":{"title":"Root"},"navigation":[{"href":"/one","type":"application/opds+json","title":"One"},{"href":"/two","type":"application/opds+json","title":"Two"},{"href":"/three","type":"application/opds+json","title":"Three"},{"href":"/four","type":"application/opds+json","title":"Four"}]}"#,
                )
            }),
        )
        .route("/one", child(active.clone(), peak.clone(), barrier.clone()))
        .route("/two", child(active.clone(), peak.clone(), barrier.clone()))
        .route("/three", child(active.clone(), peak.clone(), barrier.clone()))
        .route("/four", child(active.clone(), peak.clone(), barrier));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    tokio::time::timeout(Duration::from_secs(2), service.crawl_source(&source_id)).await.unwrap().unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), CRAWL_PAGE_CONCURRENCY);
    task.abort();
}

#[tokio::test]
async fn source_crawl_honors_retry_after_then_recovers() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let upstream = Router::new().route(
        "/root",
        get({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        Response::builder().status(StatusCode::TOO_MANY_REQUESTS).header(header::RETRY_AFTER, "1").body(Body::empty()).unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "application/opds+json")
                            .body(Body::from(r#"{"metadata":{"title":"Recovered"},"navigation":[{"title":"Website","href":"/about","type":"text/html"}]}"#))
                            .unwrap()
                    }
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let started = Instant::now();

    tokio::time::timeout(Duration::from_secs(3), service.crawl_source(&source_id)).await.unwrap().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(service.state.store.inventory().unwrap().sources[0].crawl_status, "succeeded");
    task.abort();
}

fn epub_with_embedded_cover() -> Vec<u8> {
    epub_with_embedded_cover_dimensions(360, 540)
}

fn epub_with_embedded_cover_dimensions(width: u32, height: u32) -> Vec<u8> {
    epub_with_embedded_cover_and_optional_subject(width, height, true)
}

fn epub_with_embedded_cover_and_optional_subject(width: u32, height: u32, include_subject: bool) -> Vec<u8> {
    let mut cover = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(width, height).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    archive.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored)).unwrap();
    archive.write_all(b"application/epub+zip").unwrap();
    archive.start_file("META-INF/container.xml", options).unwrap();
    archive
        .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
        .unwrap();
    archive.start_file("OEBPS/content.opf", options).unwrap();
    let subject = if include_subject { "<dc:subject>History / United States / 20th Century</dc:subject>" } else { "" };
    let package = format!(
        r##"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Embedded Title</dc:title><dc:title id="subtitle">Embedded Subtitle</dc:title><meta refines="#subtitle" property="title-type">subtitle</meta><dc:identifier id="isbn">9780821338278</dc:identifier><meta refines="#isbn" property="identifier-type">ISBN</meta><dc:creator file-as="Author, Ada">Ada Author</dc:creator><dc:contributor id="translator">Tess Translator</dc:contributor><meta refines="#translator" property="role" scheme="marc:relators">trl</meta><dc:publisher>Example Press</dc:publisher><dc:date>2024-05-06</dc:date><dc:language>sv</dc:language><dc:description>Embedded description</dc:description><dc:rights>CC BY 4.0</dc:rights><dc:source>Publisher archive</dc:source><dc:type>Text</dc:type><dc:format>application/epub+zip</dc:format><dc:relation>urn:example:related</dc:relation><dc:coverage>Sweden</dc:coverage><meta id="series" property="belongs-to-collection">Archive Studies</meta><meta refines="#series" property="collection-type">series</meta><meta refines="#series" property="group-position">2</meta><meta property="schema:audience">Young adult</meta><meta property="schema:typicalAgeRange">12-17</meta>{subject}</metadata><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"##
    );
    archive.write_all(package.as_bytes()).unwrap();
    archive.start_file("OEBPS/cover.png", options).unwrap();
    archive.write_all(cover.get_ref()).unwrap();
    archive.start_file("OEBPS/chapter.xhtml", options).unwrap();
    archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>book</body></html>"#).unwrap();
    archive.start_file("OEBPS/nav.xhtml", options).unwrap();
    archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><ol><li><a href="chapter.xhtml">The Hidden Chapter</a></li></ol></nav></body></html>"#).unwrap();
    archive.finish().unwrap().into_inner()
}

fn pdf_with_description() -> Vec<u8> {
    let mut document = lopdf::Document::with_version("1.7");
    let info = document.add_object(lopdf::dictionary! {
        "Title" => lopdf::Object::string_literal("Embedded PDF Title"),
        "Author" => lopdf::Object::string_literal("Ada PDF Author"),
        "Subject" => lopdf::Object::string_literal("A bounded description stored in the PDF information dictionary."),
    });
    let pages = document.new_object_id();
    let content = document.add_object(lopdf::Stream::new(lopdf::dictionary! {}, Vec::new()));
    let page = document.add_object(lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 400.into(), 600.into()],
    });
    document.objects.insert(
        pages,
        lopdf::Object::Dictionary(lopdf::dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let root = document.add_object(lopdf::dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", root);
    document.trailer.set("Info", info);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn extracts_subject_and_canonical_isbn_from_epub_once() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("book.epub");
    std::fs::write(&path, epub_with_embedded_cover()).unwrap();
    let extracted = extract_epub_catalog_metadata(&path, false).unwrap();
    assert_eq!(extracted.isbns, ["9780821338278"]);
    assert_eq!(extracted.subjects[0].name(), "History / United States / 20th Century");
    let saved = extracted.saved_metadata.unwrap();
    assert_eq!(saved.book.publication.publishers[0].name().as_str(), "Example Press");
    assert_eq!(saved.book.publication.dates[0].value(), "2024-05-06");
    assert_eq!(saved.toc[0].title, "The Hidden Chapter");
    assert!(extracted.thumbnail.is_none());
}

#[test]
fn extracts_local_back_matter_evidence_without_advertised_book_isbn() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("book.epub");
    std::fs::write(&path, include_bytes!("../../../shared/publication-inspection/tests/fixtures/back-matter-evidence.epub")).unwrap();
    let extracted = extract_epub_catalog_metadata(&path, false).unwrap();
    assert_eq!(extracted.isbns, ["9780393243277"]);
    assert!(extracted.subjects.iter().any(|s| s.code() == Some("Q125 .A12 2023")));
    let saved = extracted.saved_metadata.unwrap();
    assert!(saved.book.publication.properties.iter().any(|p| p.source() == "inspection:local:v2"));
}

#[test]
fn extraction_worker_serializes_a_bounded_result_for_the_parent() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("book.epub");
    let output = temporary.path().join("result.bin");
    std::fs::write(&input, epub_with_embedded_cover()).unwrap();

    run_catalog_extraction_worker(&input, "epub", "metadata", &output).unwrap();

    let bytes = std::fs::read(output).unwrap();
    assert!(bytes.len() <= MAX_EXTRACTION_RESULT_BYTES);
    let extracted: ExtractedCatalogData = wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).unwrap();
    assert_eq!(extracted.isbns, ["9780821338278"]);
    assert!(extracted.thumbnail.is_none());
}

#[test]
fn extracts_pdf_description_without_rasterizing_a_cover() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("book.pdf");
    std::fs::write(&path, pdf_with_description()).unwrap();

    let extracted = extract_catalog_metadata(&path, OpdsBookFormat::Pdf, true).unwrap();

    assert_eq!(extracted.metadata.title.as_deref(), Some("Embedded PDF Title"));
    assert_eq!(extracted.metadata.authors[0].name, "Ada PDF Author");
    assert_eq!(extracted.metadata.description.as_deref(), Some("A bounded description stored in the PDF information dictionary."));
    assert!(extracted.thumbnail.is_none());
}

#[test]
fn extraction_worker_rejects_oversized_input_before_parsing() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("oversized.pdf");
    let output = temporary.path().join("result.bin");
    let file = std::fs::File::create(&input).unwrap();
    file.set_len(MAX_PDF_METADATA_PUBLICATION_BYTES + 1).unwrap();

    let error = run_catalog_extraction_worker(&input, "pdf", "metadata", &output).unwrap_err();

    assert!(error.contains("extraction size limit"));
    assert!(!output.exists());
}

#[test]
fn extracts_mobi_description_isbn_and_subjects_from_exth_metadata() {
    use mobi::headers::ExthRecord;

    let mut mobi = mobi::MobiMetadata::default();
    mobi.name = b"Embedded MOBI Title".to_vec();
    mobi.exth.records.insert(ExthRecord::Author, vec![b"Ada MOBI Author".to_vec()]);
    mobi.exth.records.insert(ExthRecord::Description, vec![b"A useful MOBI description.".to_vec()]);
    mobi.exth.records.insert(ExthRecord::Isbn, vec![b"978-0-8213-3827-8".to_vec()]);
    mobi.exth.records.insert(ExthRecord::Subject, vec![b"History".to_vec(), b"Sweden".to_vec()]);

    let (metadata, isbns, subjects) = mobi_catalog_metadata(&mobi);

    assert_eq!(metadata.title.as_deref(), Some("Embedded MOBI Title"));
    assert_eq!(metadata.authors[0].name, "Ada MOBI Author");
    assert_eq!(metadata.description.as_deref(), Some("A useful MOBI description."));
    assert_eq!(isbns, ["9780821338278"]);
    assert_eq!(subjects.iter().map(PublicationSubject::name).collect::<Vec<_>>(), ["History", "Sweden"]);
}

#[test]
fn schedules_epub_mobi_and_pdf_for_the_same_extraction_pipeline() {
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    for (catalog_format, opds_format) in [(CatalogBookFormat::Epub, OpdsBookFormat::Epub), (CatalogBookFormat::Mobi, OpdsBookFormat::Mobi), (CatalogBookFormat::Pdf, OpdsBookFormat::Pdf)] {
        let identity = CatalogPublicationIdentity { edition_id: format!("{catalog_format:?}"), work_id: None };
        let mut publication = CatalogPublication {
            identity: identity.clone(),
            title: "Book".to_owned(),
            authors: Vec::new(),
            languages: Vec::new(),
            description: None,
            image_url: None,
            metadata: Default::default(),
            actions: vec![CatalogPublicationAction::Import {
                request: CatalogImportRequest {
                    source_id: source_id.clone(),
                    catalog_locator: "https://example.com/opds".to_owned(),
                    publication: identity,
                    download_url: format!("https://example.com/book.{}", opds_format.extension()),
                    book_format: catalog_format,
                },
            }],
        };
        assert_eq!(publication_metadata_acquisition(&publication).unwrap().book_format, opds_format);
        assert_eq!(publication_cover_fallback(&source_id, &mut publication).unwrap().1.book_format, opds_format);
        assert!(publication.image_url.as_deref().is_some_and(is_synthetic_cover_key));
    }
}

#[test]
fn startup_reconciliation_removes_sources_no_longer_in_the_registry() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let stale = CatalogSourceId::parse("opds-stale").unwrap();
    cache.ensure_source(&stale, "https://stale.example/opds").unwrap();
    let configured = endpoint("https://example.com/opds".to_owned());

    assert_eq!(reconcile_stored_sources(&cache, std::slice::from_ref(&configured), None).unwrap(), vec![stale]);
    let sources = cache.inventory().unwrap().sources;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].source_id, opds_catalog_source_id(&configured.id));
}

#[test]
fn litteraturbanken_is_first_in_the_startup_crawl_queue() {
    let mut endpoints = vec![
        endpoint("https://example.com/first".to_owned()),
        OpdsEndpoint {
            id: OpdsEndpointId::parse(PRIORITY_CRAWL_SOURCE).unwrap(),
            title: "Litteraturbanken".to_owned(),
            description: "Swedish literature".to_owned(),
            languages: vec!["Swedish".to_owned()],
            url: "https://litteraturbanken.se/epub".to_owned(),
        },
    ];
    prioritize_startup_crawls(&mut endpoints);
    assert_eq!(endpoints[0].id.as_str(), PRIORITY_CRAWL_SOURCE);
}

#[test]
fn crawl_scheduler_groups_sources_by_origin_without_changing_origin_rate() {
    let endpoint = |id: &str, url: &str| OpdsEndpoint { id: OpdsEndpointId::parse(id).unwrap(), title: id.to_owned(), description: String::new(), languages: Vec::new(), url: url.to_owned() };
    let groups = crawl_endpoint_groups(vec![
        endpoint("first", "https://one.example/catalog"),
        endpoint("second", "https://two.example/catalog"),
        endpoint("same-origin", "https://one.example/other"),
        endpoint("different-port", "https://one.example:8443/catalog"),
    ]);

    assert_eq!(groups.len(), 3, "each distinct origin gets one concurrent crawl task");
    assert_eq!(groups[0].iter().map(|endpoint| endpoint.id.as_str()).collect::<Vec<_>>(), ["first", "same-origin"]);
    assert_eq!(groups[1].iter().map(|endpoint| endpoint.id.as_str()).collect::<Vec<_>>(), ["second"]);
    assert_eq!(groups[2].iter().map(|endpoint| endpoint.id.as_str()).collect::<Vec<_>>(), ["different-port"]);
}

#[tokio::test]
async fn missing_embedded_subjects_use_the_client_ambiguous_match_policy() {
    let metadata = Router::new().route(
        "/v2/classifications",
        post(|body: Bytes| async move {
            let request = metadata_contract::decode_v2_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
            let response = metadata_contract::RichMetadataEnrichmentResponse {
                snapshot: metadata_contract::SnapshotDescription { dump_date: "2026-08".to_owned(), imported_at_ms: 1 },
                results: vec![metadata_contract::RichIsbnMetadataResult {
                    wikidata_books: Vec::new(),
                    requested_isbn: request.isbns[0].clone(),
                    canonical_isbn13: Some("9780821338278".to_owned()),
                    status: LookupStatus::Ambiguous,
                    matches: vec![
                        metadata_contract::RichWorkMetadataMatch {
                            open_library_work_id: Some("OL1W".to_owned()),
                            exact_edition_ids: vec!["OL1M".to_owned()],
                            classifications: vec![metadata_contract::Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.9".to_owned(), source: ClassificationSource::ExactEdition }],
                            authors: Vec::new(),
                            places: Vec::new(),
                            people: Vec::new(),
                            popularity: Some(metadata_contract::WorkPopularity { want_to_read_count: 2, currently_reading_count: 0, already_read_count: 1, rating_count: 1, rating_sum: 3 }),
                            description: Some("Shared Open Library description".to_owned()),
                            subjects: Vec::new(),
                        },
                        metadata_contract::RichWorkMetadataMatch {
                            open_library_work_id: Some("OL2W".to_owned()),
                            exact_edition_ids: vec!["OL2M".to_owned()],
                            classifications: vec![metadata_contract::Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.9".to_owned(), source: ClassificationSource::Work }],
                            authors: Vec::new(),
                            places: Vec::new(),
                            people: Vec::new(),
                            popularity: Some(metadata_contract::WorkPopularity { want_to_read_count: 12, currently_reading_count: 2, already_read_count: 5, rating_count: 3, rating_sum: 13 }),
                            description: Some("Shared Open Library description".to_owned()),
                            subjects: Vec::new(),
                        },
                    ],
                }],
                places: Vec::new(),
                people: Vec::new(),
            };
            ([(header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)], metadata_contract::encode_v2_response(&response).unwrap())
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, metadata).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let mut service = service(endpoint("http://127.0.0.1:9/root".to_owned()), temporary.path());
    Arc::get_mut(&mut service.state).unwrap().metadata_endpoint = Some(Url::parse(&format!("http://{address}/v2/classifications")).unwrap());

    let enriched = service.metadata_matches(&["9780821338278".to_owned()]).await.unwrap();
    let subjects = &enriched["9780821338278"].subjects;
    assert_eq!(subjects[0].source(), "openlibrary:2026-08:exact-edition");
    let projected = subject_projection::project_subjects(subjects);
    assert_eq!(projected[0].system_id(), "lcc");
    assert_eq!(projected[0].subject_path(), "LCC / Science / Mathematics / Calculating machines / Electronic computers. Computer science / QA76.9");
    assert_eq!(enriched["9780821338278"].popularity.as_ref().unwrap().reading_log_count(), 19);
    assert_eq!(enriched["9780821338278"].description.as_deref(), Some("Shared Open Library description"));
    task.abort();
}

#[tokio::test]
async fn crawl_enriches_publications_from_temporary_epubs_and_reuses_the_downloaded_cover() {
    let epub_requests = Arc::new(AtomicUsize::new(0));
    let request_counter = epub_requests.clone();
    let epub = Arc::new(epub_with_embedded_cover());
    let response_epub = epub.clone();
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/opds+json"), (header::ETAG, "\"metadata-v1\"")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":{"en":"Feed Title","sv":"Katalogtitel"},"sortAs":{"en":"Feed Title","sv":"Katalogtitel"},"author":[{"name":"Feed Author"}],"language":"en","subject":[{"name":{"en":"Fiction / Short Stories","sv":"Skönlitteratur / Noveller"},"scheme":"BISAC","code":"FIC029000"}]},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                        .into_response()
                }),
            )
            .route(
                "/one.epub",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_epub.clone();
                    async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let catalog_service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    catalog_service.crawl_source(&source_id).await.unwrap();
    wait_for_successful_extractions(&catalog_service, 1).await;
    let response = catalog_service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    let publication = &response.payloads[0].publication;
    assert_eq!(publication.title, "Embedded Title");
    assert!(publication.metadata.title_localizations.iter().any(|localized| localized.language == "sv" && localized.value == "Katalogtitel"));
    assert!(publication.metadata.sort_name_localizations.iter().any(|localized| localized.language == "sv" && localized.value == "Katalogtitel"));
    assert_eq!(publication.authors.iter().map(|author| (author.name.as_str(), author.sort_name.as_deref())).collect::<Vec<_>>(), vec![("Ada Author", Some("Author, Ada"))]);
    assert_eq!(publication.languages, ["sv"]);
    assert_eq!(publication.description, Some(CatalogDescription::PlainText { text: "Embedded description".to_owned() }));
    assert!(publication.metadata.subjects.iter().any(|subject| subject.source == "opds2:subject" && subject.code.as_deref() == Some("FIC029000")));
    assert!(publication.metadata.subjects.iter().any(|subject| subject.name_localizations.iter().any(|localized| localized.language == "sv" && localized.value == "Skönlitteratur / Noveller")));
    assert!(publication.metadata.subjects.iter().any(|subject| subject.source == "dc:subject" && subject.name == "History / United States / 20th Century"));
    assert!(publication.metadata.publishers.iter().any(|publisher| publisher.name == "Example Press"));
    assert!(publication.metadata.identifiers.iter().any(|identifier| identifier.scheme == catalog_model::Scheme::Isbn && identifier.value == "9780821338278"));
    assert!(publication.metadata.dates.iter().any(|date| date.value == "2024-05-06" && date.source == "epub:package:date"));
    assert_eq!(publication.metadata.subtitle.as_deref(), Some("Embedded Subtitle"));
    assert!(publication.metadata.collections.iter().any(|collection| collection.name == "Archive Studies" && collection.position.as_deref() == Some("2")));
    assert!(publication.metadata.contributors.iter().any(|contributor| contributor.name == "Tess Translator" && contributor.roles == ["trl"]));
    assert_eq!(publication.metadata.rights, ["CC BY 4.0"]);
    assert_eq!(publication.metadata.sources, ["Publisher archive"]);
    assert_eq!(publication.metadata.types, ["Text"]);
    assert_eq!(publication.metadata.formats, ["application/epub+zip"]);
    assert_eq!(publication.metadata.relations, ["urn:example:related"]);
    assert_eq!(publication.metadata.coverage, ["Sweden"]);
    assert!(publication.metadata.audiences.iter().any(|audience| audience.value == "Young adult"));
    assert!(publication.metadata.age_ranges.iter().any(|age| age.value == "12-17"));
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
    assert_eq!(std::fs::read_dir(temporary.path().join("acquisitions")).unwrap().count(), 0);
    let extraction = &catalog_service.state.store.inventory().unwrap().sources[0].epub_extractions;
    assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 0, 1, 0));
    assert_eq!(extraction.attempts, 1);
    assert_eq!(extraction.downloaded_bytes, epub.len() as u64);
    let toc_search = catalog_service.search_response(&source_id, CatalogSearchRequest { query: "hidden chapter".to_owned(), supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(toc_search.search.toc_matches[0].identity.edition_id, "one");

    let image_url = publication.image_url.as_deref().unwrap();
    assert_eq!(image::guess_format(&catalog_service.catalog_cover(&source_id, image_url).await.unwrap()).unwrap(), image::ImageFormat::Jpeg);
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "metadata and cover extraction should share the full fallback download");

    catalog_service.crawl_source(&source_id).await.unwrap();
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "a modified feed should reuse persisted enrichment for the same EPUB URL");
    let recrawled = catalog_service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(recrawled.payloads[0].publication.title, "Embedded Title");
    assert_eq!(recrawled.payloads[0].publication.metadata.subjects.len(), 2);
    assert_eq!(recrawled.payloads[0].publication.metadata.subtitle.as_deref(), Some("Embedded Subtitle"));
    assert!(recrawled.payloads[0].publication.metadata.identifiers.iter().any(|identifier| identifier.scheme == catalog_model::Scheme::Isbn));

    drop(catalog_service);
    let restarted = service(endpoint(format!("http://{address}/root")), temporary.path());
    restarted.crawl_source(&source_id).await.unwrap();
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "persistent extraction state should prevent a redownload after restart");
    assert_eq!(restarted.state.store.inventory().unwrap().epub_extractions.succeeded, 1);
    task.abort();
}

#[tokio::test]
async fn independent_worker_resumes_a_persisted_pending_extraction() {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_counter = requests.clone();
    let epub = Arc::new(epub_with_embedded_cover());
    let response_epub = epub.clone();
    let upstream = Router::new().route(
        "/one.epub",
        get(move || {
            request_counter.fetch_add(1, Ordering::SeqCst);
            let bytes = response_epub.clone();
            async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let root = format!("http://{address}/root");
    let acquisition_url = format!("http://{address}/one.epub");
    let service = service(endpoint(root.clone()), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let identity = CatalogPublicationIdentity { edition_id: "one".to_owned(), work_id: None };
    let publication = CatalogPublication {
        identity: identity.clone(),
        title: "Feed Title".to_owned(),
        authors: Vec::new(),
        languages: vec![LanguageTag::parse("en").unwrap()],
        description: None,
        image_url: None,
        metadata: Default::default(),
        actions: vec![CatalogPublicationAction::Import {
            request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: root.clone(), publication: identity, download_url: acquisition_url.clone(), book_format: CatalogBookFormat::Epub },
        }],
    };
    let page = CatalogPage {
        source_id: source_id.clone(),
        freshness: catalog_model::CatalogFreshness::Current,
        title: "Books".to_owned(),
        locator: root.clone(),
        navigation: Vec::new(),
        publications: vec![publication],
        groups: Vec::new(),
        facets: Vec::new(),
        pagination: catalog_model::CatalogPagination::default(),
    };
    let prepared = CatalogStore::prepare_page_writes(vec![CatalogPageWrite { page, etag: None, last_modified: None, provider_paginated: false }]).unwrap();
    service.state.store.put_prepared_pages(&source_id, &root, &prepared).unwrap();
    service.resume_pending_epub_extractions().await;

    let extraction = &service.state.store.inventory().unwrap().epub_extractions;
    assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 0, 1, 0));
    assert_eq!(requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
    task.abort();
}

#[tokio::test]
async fn crawl_activates_before_extraction_and_refresh_returns_an_incremental_delta() {
    let release = Arc::new(Semaphore::new(0));
    let download_started = Arc::new(AtomicBool::new(false));
    // This test exercises activation ordering and metadata deltas, not image
    // resizing. An intentionally unsuitable embedded image keeps that
    // unrelated CPU-heavy path out of its three-second scheduling assertion.
    let epub = Arc::new(epub_with_embedded_cover_and_optional_subject(48, 72, false));
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"Feed Title","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/one.epub", get({
                let release = release.clone();
                let download_started = download_started.clone();
                move || {
                    let release = release.clone();
                    let download_started = download_started.clone();
                    let epub = epub.clone();
                    async move {
                        let first_request = !download_started.swap(true, Ordering::SeqCst);
                        if first_request {
                            let _release = release.acquire().await.unwrap();
                        }
                        ([(header::CONTENT_TYPE, "application/epub+zip")], epub.as_ref().clone())
                    }
                }
            }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    tokio::time::timeout(Duration::from_secs(1), service.crawl_source(&source_id)).await.expect("crawl waited for EPUB extraction").unwrap();
    let base = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(base.payloads[0].publication.title, "Feed Title");
    assert_eq!(service.state.store.inventory().unwrap().sources[0].crawl_status, "succeeded");
    let extraction_service = service.clone();
    let extraction_task = tokio::spawn(async move { extraction_service.resume_pending_epub_extractions().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !download_started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    release.add_permits(1);
    extraction_task.await.unwrap();
    let delta = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: Some(base.epoch.clone()), known_publications: base.page.publications.clone() }).await.unwrap();
    assert_eq!(delta.payloads.len(), 1);
    assert_eq!(delta.payloads[0].publication.title, "Embedded Title");
    assert!(delta.payloads[0].sequence > base.payloads[0].sequence);
    task.abort();
}

#[tokio::test]
async fn coverless_epub_is_downloaded_once_then_discarded_after_thumbnail_extraction() {
    let epub_requests = Arc::new(AtomicUsize::new(0));
    let request_counter = epub_requests.clone();
    let epub = Arc::new(epub_with_embedded_cover());
    let response_epub = epub.clone();
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]},{"metadata":{"identifier":"pdf","title":"PDF","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.pdf","type":"application/pdf"}]}]}"#)
                }),
            )
            .route(
                "/one.epub",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_epub.clone();
                    async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    let page = service.browser_page(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    let image_url = page.publications[0].image_url.as_deref().unwrap();
    assert!(is_synthetic_cover_key(image_url));
    assert!(page.publications[1].image_url.as_deref().is_some_and(is_synthetic_cover_key), "PDF publications should use the shared extraction fallback");
    assert_eq!(epub_requests.load(Ordering::SeqCst), 0);

    let first = service.catalog_cover(&source_id, image_url).await.unwrap();
    assert_eq!(image::guess_format(&first).unwrap(), image::ImageFormat::Jpeg);
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
    assert_eq!(std::fs::read_dir(temporary.path().join("acquisitions")).unwrap().count(), 0);

    let second = service.catalog_cover(&source_id, image_url).await.unwrap();
    assert_eq!(second, first);
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn crawl_is_atomic_and_search_never_contacts_upstream() {
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    let upstream = Router::new().route("/root", get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async {
                ([
                    (header::CONTENT_TYPE, "application/opds+json"),
                    (header::ETAG, "\"v1\""),
                ], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"Harry Potter","author":[{"name":"J. K. Rowling"}],"language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#)
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let mut endpoint = endpoint(format!("http://{address}/root"));
    endpoint.languages = vec!["English".to_owned()];
    let service = service(endpoint, temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    service.crawl_source(&source_id).await.unwrap();
    let before_search = requests.load(Ordering::SeqCst);
    let response = service.search_response(&source_id, CatalogSearchRequest { query: "harry".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(response.search.title_matches.len(), 1);
    assert!(response.search.author_matches.is_empty());
    assert_eq!(response.payloads.len(), 1);
    let second =
        service.search_response(&source_id, CatalogSearchRequest { query: "harry".to_owned(), supported_languages: vec!["en".to_owned()], epoch: Some(response.epoch), known_publications: response.search.title_matches }).await.unwrap();
    assert!(second.payloads.is_empty());
    assert_eq!(requests.load(Ordering::SeqCst), before_search);
    task.abort();
}

#[tokio::test]
async fn replacement_crawl_reuses_cached_leaf_pages_without_fetching_them() {
    let root_requests = Arc::new(AtomicUsize::new(0));
    let leaf_requests = Arc::new(AtomicUsize::new(0));
    let upstream = Router::new()
            .route(
                "/root",
                get({
                    let root_requests = root_requests.clone();
                    move || {
                        let root_requests = root_requests.clone();
                        async move {
                            root_requests.fetch_add(1, Ordering::SeqCst);
                            (
                                [(header::CONTENT_TYPE, "application/opds+json")],
                                r#"{"metadata":{"title":"Root"},"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#,
                            )
                        }
                    }
                }),
            )
            .route(
                "/books",
                get({
                    let leaf_requests = leaf_requests.clone();
                    move || {
                        let leaf_requests = leaf_requests.clone();
                        async move {
                            leaf_requests.fetch_add(1, Ordering::SeqCst);
                            (
                                [(header::CONTENT_TYPE, "application/opds+json")],
                                r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                            )
                        }
                    }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    service.crawl_source(&source_id).await.unwrap();
    service.crawl_source(&source_id).await.unwrap();

    assert_eq!(root_requests.load(Ordering::SeqCst), 2);
    assert_eq!(leaf_requests.load(Ordering::SeqCst), 1);
    assert_eq!(service.state.store.inventory().unwrap().active_page_count, 2);
    assert_eq!(service.search_response(&source_id, CatalogSearchRequest { query: "one".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() },).await.unwrap().search.title_matches.len(), 1);
    task.abort();
}

#[tokio::test]
async fn page_response_filters_languages_before_sending_unknown_payloads() {
    let upstream = Router::new().route("/root", get(|| async {
            ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]},{"metadata":{"identifier":"deux","title":"Deux","language":"fr"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/deux.epub","type":"application/epub+zip"}]}]}"#)
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    service.crawl_source(&source_id).await.unwrap();
    let first = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    assert_eq!(first.page.publications.len(), 1);
    assert_eq!(first.payloads.len(), 1);
    assert_eq!(first.payloads[0].publication.identity.edition_id, "one");
    let known = first.page.publications.clone();
    let second = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: Some(first.epoch), known_publications: known }).await.unwrap();
    assert!(second.payloads.is_empty());
    task.abort();
}

#[tokio::test]
async fn successful_crawl_removes_pages_no_longer_reachable_from_root() {
    let include_old_page = Arc::new(AtomicBool::new(true));
    let root_state = include_old_page.clone();
    let upstream = Router::new()
            .route(
                "/root",
                get(move || {
                    let body = if root_state.load(Ordering::SeqCst) {
                        r#"{"metadata":{"title":"Books"},"navigation":[{"title":"Old","href":"/old","type":"application/opds+json"}],"publications":[{"metadata":{"identifier":"current","title":"Current Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/current.epub","type":"application/epub+zip"}]}]}"#
                    } else {
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"current","title":"Current Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/current.epub","type":"application/epub+zip"}]}]}"#
                    };
                    async move { ([(header::CONTENT_TYPE, "application/opds+json")], body) }
                }),
            )
            .route(
                "/old",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Old"},"publications":[{"metadata":{"identifier":"old","title":"Removed Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/old.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    service.crawl_source(&source_id).await.unwrap();
    assert_eq!(
        service.search_response(&source_id, CatalogSearchRequest { query: "removed".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() },).await.unwrap().search.title_matches.len(),
        1
    );

    include_old_page.store(false, Ordering::SeqCst);
    service.crawl_source(&source_id).await.unwrap();
    assert!(service.search_response(&source_id, CatalogSearchRequest { query: "removed".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() },).await.unwrap().search.title_matches.is_empty());
    task.abort();
}

#[tokio::test]
async fn cover_worker_downloads_and_caches_each_saved_url_once() {
    let mut source = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(320, 480).write_to(&mut source, ImageOutputFormat::Png).unwrap();
    let source = Arc::new(source.into_inner());
    let cover_requests = Arc::new(AtomicUsize::new(0));
    let response_source = source.clone();
    let response_requests = cover_requests.clone();
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/cover.png",
                get(move || {
                    response_requests.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_source.clone();
                    async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let image_url = format!("http://{address}/cover.png");

    service.crawl_source(&source_id).await.unwrap();
    service.resume_pending_cover_downloads().await;
    let jpeg = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(bytes) = service.catalog_cover(&source_id, &image_url).await {
                break bytes;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(image::guess_format(&jpeg).unwrap(), image::ImageFormat::Jpeg);
    assert_eq!(cover_requests.load(Ordering::SeqCst), 1);

    let response = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
    let cover_reference = response.payloads[0].publication.image_url.as_deref().unwrap();
    let digest = catalog_core::catalog_cover_asset_digest(cover_reference).expect("the catalogue response should own its cached cover");
    assert_eq!(digest, blake3::hash(&jpeg).to_hex().as_str());

    let router = app(service.clone());
    let cover_response = router.clone().oneshot(Request::builder().uri(cover_reference).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cover_response.status(), StatusCode::OK);
    assert_eq!(cover_response.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(cover_response.headers()[header::CACHE_CONTROL], COVER_CACHE_CONTROL);
    let etag = cover_response.headers()[header::ETAG].clone();
    assert_eq!(to_bytes(cover_response.into_body(), 4 * 1024 * 1024).await.unwrap().as_ref(), jpeg.as_slice());
    let not_modified = router.oneshot(Request::builder().uri(cover_reference).header(header::IF_NONE_MATCH, etag).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);

    service.crawl_source(&source_id).await.unwrap();
    tokio::task::yield_now().await;
    assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn tiny_catalog_cover_falls_back_to_cover_from_metadata_epub_download() {
    let mut tiny_cover = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(32, 48).write_to(&mut tiny_cover, ImageOutputFormat::Png).unwrap();
    let tiny_cover = Arc::new(tiny_cover.into_inner());
    let epub = Arc::new(epub_with_embedded_cover());
    let cover_requests = Arc::new(AtomicUsize::new(0));
    let epub_requests = Arc::new(AtomicUsize::new(0));
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/cover.png",
                get({
                    let requests = cover_requests.clone();
                    let bytes = tiny_cover.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let bytes = bytes.clone();
                        async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                    }
                }),
            )
            .route(
                "/one.epub",
                get({
                    let requests = epub_requests.clone();
                    let bytes = epub.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let bytes = bytes.clone();
                        async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                    }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let image_url = format!("http://{address}/cover.png");

    service.crawl_source(&source_id).await.unwrap();
    service.resume_pending_cover_downloads().await;
    wait_for_successful_extractions(&service, 1).await;
    let jpeg = service.catalog_cover(&source_id, &image_url).await.unwrap();
    let dimensions = image::load_from_memory(&jpeg).unwrap().dimensions();

    assert_eq!(dimensions, (600, 900));
    assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
    assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "metadata and cover fallback must share one full download after the range probe");
    task.abort();
}

#[tokio::test]
async fn tiny_catalog_and_epub_covers_make_publication_coverless() {
    let mut tiny_cover = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(32, 48).write_to(&mut tiny_cover, ImageOutputFormat::Png).unwrap();
    let tiny_cover = Arc::new(tiny_cover.into_inner());
    let epub = Arc::new(epub_with_embedded_cover_dimensions(48, 72));
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/cover.png", get(move || {
                let bytes = tiny_cover.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
            }))
            .route("/one.epub", get(move || {
                let bytes = epub.clone();
                async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
            }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    service.crawl_source(&source_id).await.unwrap();
    service.resume_pending_cover_downloads().await;
    wait_for_successful_extractions(&service, 1).await;
    let response = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();

    assert_eq!(response.payloads[0].publication.image_url, None);
    task.abort();
}

#[tokio::test]
async fn public_registry_is_uncompressed_binary_and_needs_no_authentication() {
    let temporary=tempfile::tempdir().unwrap();
    let service = CatalogService::bundled_with_storage(Some(temporary.path().join("catalog.db")),temporary.path().join("covers")).unwrap();
    let router = app(service);
    let response = router.clone().oneshot(Request::builder().uri("/api/opds/catalogs").header(header::ACCEPT, transport_wire::MEDIA_TYPE).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
    let bytes = to_bytes(response.into_body(), transport_wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
    let registry: CatalogRegistry = transport_wire::decode(&bytes, transport_wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
    assert!(!registry.catalogs.is_empty());
    assert!(registry.catalogs.iter().all(|source| source.id!="office-of-the-historian"));

    let response = router.oneshot(Request::builder().uri("/api/catalog/sources").header(header::ACCEPT, transport_wire::MEDIA_TYPE).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), transport_wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
    let registry: CatalogSourceRegistry = transport_wire::decode(&bytes, transport_wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
    assert!(!registry.sources.is_empty());
    assert!(registry.sources.iter().all(|registration| !registration.root_locator.is_empty()));
}

#[tokio::test]
async fn browser_api_supports_cors_json_browsing_and_validated_acquisitions() {
    let mut cover = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(320, 480).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
    let response_cover = Arc::new(cover.into_inner());
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/search",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Search"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/cover.png", get(move || {
                let bytes = response_cover.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
            }))
            .route("/one.epub", get(|| async { ([(header::CONTENT_TYPE, "application/epub+zip")], b"epub bytes".as_slice()) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let router = app(service.clone());

    let preflight = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/web/sources/opds-test/search")
                .header(header::ORIGIN, "https://app.bokheim.se")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(preflight.status().is_success());
    assert_eq!(preflight.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");

    let page_request = CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() };
    let page = router
        .clone()
        .oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/page").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&page_request).unwrap())).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let page: CatalogPage = serde_json::from_slice(&to_bytes(page.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(page.publications[0].title, "One");
    let cover = service.catalog_cover(&source_id, page.publications[0].image_url.as_deref().unwrap()).await.unwrap();
    assert_eq!(image::guess_format(&cover).unwrap(), image::ImageFormat::Jpeg);

    let search_request = CatalogSearchRequest { query: "one".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() };
    let search = router
        .clone()
        .oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/search").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&search_request).unwrap())).unwrap())
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let search: CatalogPage = serde_json::from_slice(&to_bytes(search.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert_eq!(search.publications[0].title, "One");

    let request = page.publications[0]
        .actions
        .first()
        .map(|action| match action {
            catalog_model::CatalogPublicationAction::Import { request } => request.clone(),
        })
        .unwrap();
    let acquisition =
        router.oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/acquire").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&request).unwrap())).unwrap()).await.unwrap();
    assert_eq!(acquisition.status(), StatusCode::OK);
    assert_eq!(acquisition.headers().get(header::CONTENT_TYPE).unwrap(), "application/epub+zip");
    assert_eq!(&to_bytes(acquisition.into_body(), 1024).await.unwrap()[..], b"epub bytes");
    task.abort();
}

/// Background metadata downloads must not consume either interactive
/// acquisition or cover capacity.
#[tokio::test]
async fn acquisition_waits_for_the_background_extraction_worker() {
    let mut cover = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(320, 480).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
    let cover = Arc::new(cover.into_inner());
    let response_cover = cover.clone();
    let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/one.epub", get(|| async { ([(header::CONTENT_TYPE, "application/epub+zip")], b"epub bytes".as_slice()) }))
            .route(
                "/cover.png",
                get(move || {
                    let bytes = response_cover.clone();
                    async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                }),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    let source_id = CatalogSourceId::parse("opds-test").unwrap();

    let page = service.browser_page(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
    let request = page.publications[0]
        .actions
        .first()
        .map(|action| match action {
            CatalogPublicationAction::Import { request } => request.clone(),
        })
        .unwrap();

    let enrichment = service.state.background_download_slots.acquire_many(BACKGROUND_DOWNLOAD_LIMIT as u32).await.unwrap();
    let downloaded = tokio::time::timeout(Duration::from_secs(5), service.acquisition(&source_id, &request))
        .await
        .expect("browser acquisition must not block indefinitely behind background enrichment")
        .expect("browser acquisition must not be refused while background enrichment holds the permit");
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"epub bytes");
    let image_url = page.publications[0].image_url.as_deref().unwrap();
    let cover = tokio::time::timeout(Duration::from_secs(5), service.catalog_cover(&source_id, image_url))
        .await
        .expect("cover lookup must not block behind background metadata downloads")
        .expect("cover lookup must retain independent interactive capacity");
    assert_eq!(image::guess_format(&cover).unwrap(), image::ImageFormat::Jpeg);
    drop(enrichment);
    task.abort();
}

#[tokio::test]
async fn startup_refresh_activates_every_registered_source_without_hourly_spacing() {
    let epub = Arc::new(epub_with_embedded_cover());
    let response_epub = epub.clone();
    let upstream = Router::new()
        .route(
            "/root",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/opds+json")],
                    r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                )
            }),
        )
        .route(
            "/one.epub",
            get(move || {
                let bytes = response_epub.clone();
                async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let service = service(endpoint(format!("http://{address}/root")), temporary.path());
    service.refresh_all_sources().await;
    wait_for_successful_extractions(&service, 1).await;
    assert_eq!(service.state.store.inventory().unwrap().sources[0].active_page_count, 1);
    let router = app(service.clone());
    let response = router.clone().oneshot(Request::builder().uri("/api/web/stats").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let stats: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(stats["status"], "available");
    assert_eq!(stats["registry_revision"], 1);
    assert_eq!(stats["metadata_enrichment_configured"], false);
    assert!(stats["uptime_seconds"].is_u64());
    assert_eq!(stats["active_page_count"], 1);
    assert_eq!(stats["active_publication_count"], 1);
    assert_eq!(stats["active_epub_count"], 1);
    assert_eq!(stats["epub_extractions"]["succeeded"], 1);
    assert_eq!(stats["epub_extractions"]["attempts"], 1);
    assert_eq!(stats["sources"][0]["crawl_status"], "succeeded");
    assert_eq!(stats["sources"][0]["crawl_frontier_count"], 0);
    assert_eq!(stats["extraction_failures"], serde_json::json!([]));
    let response = router.oneshot(Request::builder().uri("/internal/operations").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let operations_router = operations_app(service.clone());
    let response = operations_router.clone().oneshot(Request::builder().uri("/internal/operations").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let operations: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(operations["extraction_failures"], serde_json::json!([]));
    assert_eq!(operations["processes"].as_array().unwrap().len(), 3);
    let pause = operations_router.oneshot(Request::post("/internal/processes/enrichment").header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"action":"pause"}"#)).unwrap()).await.unwrap();
    assert_eq!(pause.status(), StatusCode::OK);
    let paused: serde_json::Value = serde_json::from_slice(&to_bytes(pause.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(paused["process"], "enrichment");
    assert_eq!(paused["enabled"], false);
    let settings = service.state.store.process_settings().unwrap();
    assert!(settings.catalogue_crawling);
    assert!(settings.cover_downloads);
    assert!(!settings.enrichment);
    task.abort();
}

#[tokio::test]
async fn service_start_pauses_every_administrator_controlled_worker() {
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog-cache.db");
    let cache = CatalogStore::open(&cache_path).unwrap();
    for process in ["catalogue_crawling", "cover_downloads", "enrichment"] {
        cache.set_process_enabled(process, true).unwrap();
    }
    drop(cache);

    let service = CatalogService::bundled_with_storage(Some(cache_path), temporary.path().join("covers")).unwrap();
    assert_eq!(service.state.store.process_settings().unwrap(), CatalogProcessSettings::default());
    assert!(CatalogProcess::ALL.into_iter().all(|process| !service.process_enabled(process)));
}

#[test]
fn catalogue_failures_are_normalized_into_stable_retry_categories() {
    assert_eq!(classify_catalog_failure("publication download exceeded 45 seconds"), ("network_timeout", true));
    assert_eq!(classify_catalog_failure("HTTP 404 Not Found"), ("http_unavailable", false));
    assert_eq!(classify_catalog_failure("invalid EPUB ZIP central directory"), ("invalid_publication", false));
    assert_eq!(classify_catalog_failure("database worker stopped"), ("internal", true));
}

#[test]
fn catalogue_server_embeds_the_complete_authoritative_taxonomy() {
    let paths = subject_projection::unified_subject_paths("bisac", "HIS036000");
    assert!(paths.contains(&"Humanities / History / History by Region / North American History / U.S. History".to_owned()), "the curated server taxonomy must retain the resolved History route, got {paths:?}");
}

#[test]
fn crawl_frontier_is_disk_backed_deduplicated_and_bounded() {
    let mut frontier = DiskCrawlFrontier::new("https://example.com/root", 3).unwrap();
    frontier.enqueue([CrawlLocator::regular("https://example.com/one"), CrawlLocator::regular("https://example.com/one"), CrawlLocator::regular("https://example.com/two")]).unwrap();
    assert_eq!(frontier.claim_batch().unwrap(), [CrawlLocator::regular("https://example.com/root"), CrawlLocator::regular("https://example.com/one"), CrawlLocator::regular("https://example.com/two")]);
    frontier.enqueue([CrawlLocator::regular("https://example.com/root")]).unwrap();
    assert!(frontier.claim_batch().unwrap().is_empty(), "claimed URLs must remain deduplicated on disk");
    frontier.enqueue([CrawlLocator { locator: "https://example.com/root".to_owned(), popular_listing: true }]).unwrap();
    assert_eq!(frontier.claim_batch().unwrap(), [CrawlLocator { locator: "https://example.com/root".to_owned(), popular_listing: true }], "new relation evidence revisits a page once");

    let mut bounded = DiskCrawlFrontier::new("https://example.com/root", 2).unwrap();
    let error = bounded.enqueue([CrawlLocator::regular("https://example.com/one"), CrawlLocator::regular("https://example.com/two")]).unwrap_err();
    assert!(error.to_string().contains("exceeded 2 discovered pages"));
}

#[test]
fn popular_feed_relation_reaches_publication_rank_evidence() {
    let source_id = CatalogSourceId::parse("opds-test").unwrap();
    let publication = CatalogPublication {
        identity: CatalogPublicationIdentity { edition_id: "book".to_owned(), work_id: None },
        title: "Book".to_owned(),
        authors: Vec::new(),
        languages: Vec::new(),
        description: None,
        image_url: None,
        metadata: Default::default(),
        actions: Vec::new(),
    };
    let mut page = CatalogPage {
        source_id,
        freshness: catalog_model::CatalogFreshness::Current,
        title: "Root".to_owned(),
        locator: "https://example.com/root".to_owned(),
        navigation: vec![catalog_model::CatalogLink {
            title: "Popular".to_owned(),
            target: CatalogLinkTarget::Page { locator: "https://example.com/popular".to_owned() },
            relations: vec!["http://opds-spec.org/sort/popular".to_owned()],
            number_of_items: None,
            active: false,
        }],
        publications: vec![publication],
        groups: Vec::new(),
        facets: Vec::new(),
        pagination: Default::default(),
    };
    let links = internal_page_links(&Url::parse("https://example.com/root").unwrap(), &page, false).unwrap();
    assert_eq!(links, [CrawlLocator { locator: "https://example.com/popular".to_owned(), popular_listing: true }]);

    page.locator = "https://example.com/popular".to_owned();
    annotate_popular_listing(&mut page, true);
    assert_eq!(page.publications[0].metadata.popularity_ranks, [PopularityRank { position: 1, source: "opds:sort/popular:https://example.com/popular".to_owned() }]);
}

#[tokio::test]
async fn inspected_print_isbn_fetches_lcc_without_changing_ebook_identity() {
    let metadata = Router::new()
        .route(
            "/v2/edition-identities",
            post(|body: Bytes| async move {
                let request = metadata_contract::decode_v2_edition_identity_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
                let query = &request.queries[0];
                assert_eq!(query.title, "Science History");
                assert_eq!(query.authors, ["Ada Author"]);
                let response = EditionIdentityResponse {
                    snapshot: metadata_contract::SnapshotDescription { dump_date: "2026-08".into(), imported_at_ms: 1 },
                    results: vec![metadata_contract::EditionIdentityResult {
                        catalog_subjects: None,
                        query_id: query.query_id.clone(),
                        status: EditionIdentityStatus::Matched,
                        candidates: Vec::new(),
                        candidates_truncated: false,
                        matched: Some(metadata_contract::EditionIdentityMatch {
                            subtitle: None,
                            canonical_isbn13: "9780821338278".into(),
                            open_library_edition_id: "OL1M".into(),
                            open_library_work_id: Some("OL1W".into()),
                            title: query.title.clone(),
                            work_title: None,
                            title_match: metadata_contract::EditionTitleMatch::Edition,
                            authors: query.authors.clone(),
                            publishers: Vec::new(),
                            publication_year: None,
                            matched_publisher: false,
                            matched_publication_year: false,
                            classifications: Vec::new(),
                        }),
                    }],
                };
                ([(header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)], metadata_contract::encode_v2_edition_identity_response(&response).unwrap())
            }),
        )
        .route(
            "/v2/classifications",
            post(|body: Bytes| async move {
                let request = metadata_contract::decode_v2_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
                assert_eq!(request.isbns, ["9780821338278"]);
                let record = publication_enrichment::related_isbn::classification_record(vec![metadata_contract::Classification {
                    scheme: ClassificationScheme::LibraryOfCongress,
                    notation: "Q125".into(),
                    source: ClassificationSource::ExactEdition,
                    evidence: Vec::new(),
                }]);
                let response = metadata_contract::RichMetadataEnrichmentResponse {
                    snapshot: metadata_contract::SnapshotDescription { dump_date: "2026-08".into(), imported_at_ms: 1 },
                    results: vec![metadata_contract::RichIsbnMetadataResult {
                        wikidata_books: Vec::new(),
                        requested_isbn: request.isbns[0].clone(),
                        canonical_isbn13: Some(request.isbns[0].clone()),
                        status: LookupStatus::Matched,
                        matches: vec![record],
                    }],
                    places: Vec::new(),
                    people: Vec::new(),
                };
                ([(header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)], metadata_contract::encode_v2_response(&response).unwrap())
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, metadata).await.unwrap() });
    let temporary = tempfile::tempdir().unwrap();
    let mut service = service(endpoint("http://127.0.0.1:9/root".into()), temporary.path());
    Arc::get_mut(&mut service.state).unwrap().metadata_endpoint = Some(Url::parse(&format!("http://{address}")).unwrap());
    let mut book = publication_model::BookMetadataState {
        title: "Science History".into(),
        description: String::new(),
        publication: Default::default(),
        contributors: vec![publication_model::Contributor::new("Ada Author", None, publication_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
    };
    let section = publication_inspection::evidence::inspect_section(&book, "copyright.xhtml", "Copyright Ada Author\nScience History\nISBN 9780821338278 (hardcover)\nISBN 9780393243277 (epub)");
    publication_inspection::evidence::apply_evidence(&mut book, &[section], "epub");
    let before = book.publication.identifiers.clone();
    let subjects = service.inspected_print_subjects(&book).await.unwrap();
    assert_eq!(subjects.len(), 1);
    assert_eq!(subjects[0].code(), Some("Q125"));
    assert_eq!(subjects[0].source(), "inspection:verified-print-isbn:9780821338278");
    assert_eq!(book.publication.identifiers, before);
    assert_eq!(book.publication.identifiers[0].canonical_value().as_deref(), Some("9780393243277"));
    task.abort();
}

#[tokio::test]
async fn year_only_catalogue_matches_supply_subjects_without_an_isbn() {
    let router = Router::new().route(
        "/v2/edition-identities",
        post(|body: Bytes| async move {
            let request = metadata_contract::decode_v2_edition_identity_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
            let results = request
                .queries
                .into_iter()
                .map(|q| metadata_contract::EditionIdentityResult {
                    catalog_subjects: None,
                    query_id: q.query_id,
                    status: EditionIdentityStatus::Ambiguous,
                    matched: None,
                    candidates_truncated: false,
                    candidates: [1999, 2013]
                        .into_iter()
                        .map(|year| metadata_contract::EditionIdentityMatch {
                            subtitle: None,
                            canonical_isbn13: if year == 1999 { "9780821338278" } else { "9780393243277" }.into(),
                            open_library_edition_id: format!("OL{year}M"),
                            open_library_work_id: Some(format!("OL{year}W")),
                            title: q.title.clone(),
                            work_title: None,
                            title_match: metadata_contract::EditionTitleMatch::Edition,
                            authors: q.authors.clone(),
                            publishers: vec!["Example Press".into()],
                            publication_year: Some(year),
                            matched_publisher: false,
                            matched_publication_year: false,
                            classifications: vec![metadata_contract::Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: format!("E806 .S89 {year}"), source: ClassificationSource::ExactEdition, evidence: vec![] }],
                        })
                        .collect(),
                })
                .collect();
            metadata_contract::encode_v2_edition_identity_response(&EditionIdentityResponse { snapshot: metadata_contract::SnapshotDescription { dump_date: "test".into(), imported_at_ms: 0 }, results }).unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(endpoint("http://127.0.0.1:9/root".into()), temp.path());
    Arc::get_mut(&mut service.state).unwrap().metadata_endpoint = Some(Url::parse(&format!("http://{address}")).unwrap());
    let mut metadata = CatalogEpubMetadata {
        book: publication_model::BookMetadataState {
            title: "Example History".into(),
            description: String::new(),
            publication: Default::default(),
            contributors: vec![publication_model::Contributor::new("Ada Author", None, publication_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
        },
        toc: Vec::new(),
    };
    assert!(service.infer_catalog_epub_isbn(&mut metadata).await.unwrap().is_none());
    assert!(metadata.book.publication.identifiers.is_empty());
    assert_eq!(metadata.book.publication.subjects.len(), 1);
    assert_eq!(metadata.book.publication.subjects[0].code(), Some("E806"));
    task.abort();
}

#[tokio::test]
async fn shared_subject_catalogue_matches_supply_art_history_without_an_isbn() {
    let router = Router::new().route(
        "/v2/edition-identities",
        post(|body: Bytes| async move {
            let request = metadata_contract::decode_v2_edition_identity_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
            let results = request
                .queries
                .into_iter()
                .map(|q| metadata_contract::EditionIdentityResult {
                    catalog_subjects: None,
                    query_id: q.query_id,
                    status: EditionIdentityStatus::Ambiguous,
                    matched: None,
                    candidates_truncated: false,
                    candidates: [1999, 2013]
                        .into_iter()
                        .map(|year| metadata_contract::EditionIdentityMatch {
                            subtitle: None,
                            canonical_isbn13: if year == 1999 { "9780821338278" } else { "9780393243277" }.into(),
                            open_library_edition_id: format!("OL{year}M"),
                            open_library_work_id: Some(format!("OL{year}W")),
                            title: "Art History".into(),
                            work_title: None,
                            title_match: metadata_contract::EditionTitleMatch::Edition,
                            authors: q.authors.clone(),
                            publishers: vec![if year == 1999 { "Oxford University Press, USA" } else { "Oxford University Press" }.into()],
                            publication_year: Some(year),
                            matched_publisher: false,
                            matched_publication_year: false,
                            classifications: vec![metadata_contract::Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "N5300".into(), source: ClassificationSource::ExactEdition, evidence: vec![] }],
                        })
                        .collect(),
                })
                .collect();
            metadata_contract::encode_v2_edition_identity_response(&EditionIdentityResponse { snapshot: metadata_contract::SnapshotDescription { dump_date: "test".into(), imported_at_ms: 0 }, results }).unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    let mut service = service(endpoint("http://127.0.0.1:9/root".into()), temp.path());
    Arc::get_mut(&mut service.state).unwrap().metadata_endpoint = Some(Url::parse(&format!("http://{address}")).unwrap());
    let mut metadata = CatalogEpubMetadata {
        book: publication_model::BookMetadataState {
            title: "Art History: A Very Short Introduction".into(),
            description: String::new(),
            publication: Default::default(),
            contributors: vec![publication_model::Contributor::new("Ada Author", None, publication_model::AUTHOR_MARC_RELATOR_CODE).unwrap()],
        },
        toc: Vec::new(),
    };
    assert!(service.infer_catalog_epub_isbn(&mut metadata).await.unwrap().is_none());
    assert!(metadata.book.publication.identifiers.is_empty());
    assert_eq!(metadata.book.publication.subjects.len(), 1);
    assert_eq!(metadata.book.publication.subjects[0].code(), Some("N5300"));
    task.abort();
}
