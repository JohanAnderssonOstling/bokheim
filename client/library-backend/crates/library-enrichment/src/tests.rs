#![cfg(not(target_arch = "wasm32"))]

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use book_model::{AUTHOR_MARC_RELATOR_CODE, BookDetails, BookFormat, BookRecord, Contributor, Identifier, Scheme, Scope};
use image::{DynamicImage, ImageFormat, RgbImage};
use library_database::{DatabaseClient, ImportCommit, executor::BackendExecutor};
use library_files::asset_store::AssetStore;
use library_model::{BlobKind, PublishedStagedBook};
use library_services::events::LibraryEventRouter;
use std::{
    collections::{BTreeMap, HashSet},
    io::Cursor,
    sync::{Arc, Mutex},
};
use sync_common::{BookId, ROOT_DIR_ID};

use super::{isbn_key, isbns};
use crate::{CoverClient, LibraryEnrichment, MetadataClient};

struct Provider {
    origin: String,
    requests: Mutex<Vec<String>>,
    isbn_status: StatusCode,
    isbn_image: Vec<u8>,
    search_docs: Mutex<Vec<serde_json::Value>>,
    fallback_image: Vec<u8>,
}

fn artwork(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, image::Rgb(color))).write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn record() -> BookRecord {
    let mut record = BookRecord { metadata: BookDetails { title: "Utilitarianism".into(), ..Default::default() }, description: String::new() };
    record.contributors.push(Contributor::new("John Stuart Mill", AUTHOR_MARC_RELATOR_CODE).unwrap());
    record.book.identifiers.push(Identifier::new("9780306406157", Scheme::Isbn, Scope::Book).unwrap());
    record
}

async fn serve(State(provider): State<Arc<Provider>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    provider.requests.lock().unwrap().push(request.uri().to_string());
    match path.as_str() {
        "/metadata/v3/books" => {
            let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
            let request: metadata_contract::books::BookEnrichmentRequest = serde_json::from_slice(&bytes).unwrap();
            assert!(!request.books[0].needs_cover, "the metadata server must not fetch artwork");
            axum::Json(serde_json::json!({"policy":"books_v5","results":[{"key":request.books[0].key,"status":"no_match","record":null,"cover":null}]})).into_response()
        }
        "/ol/b/isbn/9780306406157-L.jpg" => {
            if !provider.isbn_status.is_success() {
                return provider.isbn_status.into_response();
            }
            ([("content-type", "image/png")], provider.isbn_image.clone()).into_response()
        }
        "/ol/search.json" => axum::Json(serde_json::json!({"docs":*provider.search_docs.lock().unwrap()})).into_response(),
        "/ol/b/id/42-L.jpg" => ([("content-type", "image/png")], provider.fallback_image.clone()).into_response(),
        _ => Response::builder().status(404).body(Body::empty()).unwrap(),
    }
}

async fn provider(status: StatusCode, width: u32) -> (CoverClient, Arc<Provider>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let provider = Arc::new(Provider {
        origin: origin.clone(),
        requests: Default::default(),
        isbn_status: status,
        isbn_image: artwork(width, width * 3 / 2, [240, 100, 0]),
        search_docs: Mutex::new(vec![serde_json::json!({"title":"Utilitarianism","author_name":["John Stuart Mill"],"cover_i":42})]),
        fallback_image: artwork(300, 450, [0, 100, 240]),
    });
    let app = Router::new().fallback(any(serve)).with_state(provider.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut client = CoverClient::default();
    client.openlibrary_origin = format!("{origin}/ol/").parse().unwrap();
    client.openlibrary_covers = client.openlibrary_origin.clone();
    client.interval_ms = 0;
    (client, provider, task)
}

#[tokio::test]
async fn openlibrary_isbn_artwork_is_downloaded_directly() {
    let (client, provider, task) = provider(StatusCode::OK, 600).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.isbn_image);
    assert_eq!(*provider.requests.lock().unwrap(), ["/ol/b/isbn/9780306406157-L.jpg?default=false"]);
    task.abort();
}

#[tokio::test]
async fn missing_isbn_artwork_falls_back_to_title_and_author_search() {
    let (client, provider, task) = provider(StatusCode::NOT_FOUND, 600).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.fallback_image);
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths.len(), 3);
    assert_eq!(paths[0], "/ol/b/isbn/9780306406157-L.jpg?default=false");
    let search = url::Url::parse(&format!("{}{}", provider.origin, paths[1])).unwrap();
    assert_eq!(search.path(), "/ol/search.json");
    let params = search.query_pairs().collect::<BTreeMap<_, _>>();
    assert_eq!(params.get("title").unwrap(), "Utilitarianism");
    assert_eq!(params.get("author").unwrap(), "John Stuart Mill");
    assert_eq!(paths[2], "/ol/b/id/42-L.jpg?default=false");
    task.abort();
}

#[tokio::test]
async fn missing_author_keeps_isbn_lookup() {
    let (client, provider, task) = provider(StatusCode::OK, 600).await;
    let mut book = record();
    book.contributors.clear();
    assert_eq!(client.book_cover(&book).await.unwrap().unwrap(), provider.isbn_image);
    assert_eq!(*provider.requests.lock().unwrap(), ["/ol/b/isbn/9780306406157-L.jpg?default=false"]);
    task.abort();
}

#[tokio::test]
async fn undersized_isbn_artwork_falls_back_to_search() {
    let (client, provider, task) = provider(StatusCode::OK, 50).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.fallback_image);
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
    task.abort();
}

#[tokio::test]
async fn title_search_rejects_unrelated_authors_and_works() {
    let (client, provider, task) = provider(StatusCode::NOT_FOUND, 600).await;
    *provider.search_docs.lock().unwrap() = vec![
        serde_json::json!({"title":"Utilitarianism","author_name":["Other Author"],"cover_i":42}),
        serde_json::json!({"title":"Different Work","author_name":["John Stuart Mill"],"cover_i":42}),
        serde_json::json!({"title":"Utilitarianism: On Liberty and Other Essays","author_name":["John Stuart Mill"],"cover_i":42}),
    ];
    assert!(client.book_cover(&record()).await.unwrap().is_none());
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|path| !path.starts_with("/ol/b/id/")));
    task.abort();
}

#[test]
fn isbn_lookup_normalizes_identifiers_and_respects_scope() {
    assert_eq!(isbn_key("0-306-40615-2"), isbn_key("9780306406157"));
    let mut book = record();
    assert_eq!(isbns(&book), ["9780306406157"]);
    book.book.identifiers[0].scope = Scope::Edition;
    assert!(isbns(&book).is_empty());
    book.book.identifiers.push(Identifier::new("B000FC1FIU", Scheme::Asin, Scope::Edition).unwrap());
    assert_eq!(isbns(&book), ["9780306406157"]);
}

#[tokio::test]
async fn title_search_prioritizes_strict_coauthor_matches_and_preserves_provider_order() {
    let (client, provider, task) = provider(StatusCode::NOT_FOUND, 600).await;
    let mut book = record();
    book.contributors = vec![Contributor::new("John Stuart Mill and Harriet Taylor", AUTHOR_MARC_RELATOR_CODE).unwrap()];
    for (strict_id, expected) in [(40, vec![40, 41, 42]), (42, vec![42])] {
        *provider.search_docs.lock().unwrap() = vec![
            serde_json::json!({"title":"Utilitarianism","author_name":["J. S. Mill"],"cover_i":42}),
            serde_json::json!({"title":"Utilitarianism","author_name":["Mill, John Stuart"],"cover_i":strict_id}),
            serde_json::json!({"title":"Utilitarianism","author_name":["Other Author and Harriét Taylor"],"cover_i":41}),
        ];
        provider.requests.lock().unwrap().clear();
        assert_eq!(client.book_cover(&book).await.unwrap().unwrap(), provider.fallback_image);
        let paths = provider.requests.lock().unwrap();
        let covers = paths.iter().filter(|path| path.starts_with("/ol/b/id/")).cloned().collect::<Vec<_>>();
        assert_eq!(covers, expected.iter().map(|id| format!("/ol/b/id/{id}-L.jpg?default=false")).collect::<Vec<_>>());
    }
    task.abort();
}

#[tokio::test]
async fn metadata_miss_still_downloads_direct_artwork_and_preserves_newer_local_cover() {
    let (covers, provider, task) = provider(StatusCode::OK, 600).await;
    let temporary = tempfile::tempdir().unwrap();
    let db = DatabaseClient::open(":memory:", &BackendExecutor::new().unwrap()).await.unwrap();
    db.initialize_library().await.unwrap();
    let hash = BookId::new(&"a".repeat(64));
    db.commit_import(ImportCommit {
        source_identity: None,
        byte_checksum: None,
        audiobook_content_hash: None,
        parent_id: ROOT_DIR_ID,
        file_name: "book.epub".into(),
        format: BookFormat::Epub,
        book_id: hash,
        size_bytes: 10,
        inspection_version: 1,
        inspection: Some(book_inspection::InspectedBook { metadata: record(), pdf: None, audiobook: None, toc: Vec::new() }),
        published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: Vec::new(),
    })
    .await
    .unwrap();
    db.complete_thumbnail_work(&hash, false).await.unwrap();

    let (events, _) = LibraryEventRouter::new();
    let assets = AssetStore::open(temporary.path().to_str().unwrap()).unwrap();
    let lane = LibraryEnrichment {
        state: Arc::new(tokio::sync::RwLock::new(library_placement::LibraryState::new(db.clone(), assets.clone(), events.clone()))),
        database: db.clone(),
        client: MetadataClient::new(&format!("{}/metadata/", provider.origin)).unwrap(),
        covers,
        cpu: Arc::new(cpu_host::NativeHost),
        events,
    };
    assert_eq!(lane.enrich_books(&HashSet::from([hash])).await.unwrap(), 2);
    let original = assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap();
    assert!(!original.is_empty());
    assert!(db.enrichment_candidates(&[hash], 0).await.unwrap().is_empty());
    assert!(provider.requests.lock().unwrap().iter().all(|path| !path.contains("v3/covers")));
    let state = db.thumbnail_work_state(&hash).await.unwrap();
    let other = cpu_host::cover(&*lane.cpu, provider.fallback_image.clone()).await.unwrap();
    assert_eq!(lane.apply_enriched_cover(hash, &state, Some(&other)).await.unwrap(), None);
    assert_eq!(assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), original);
    task.abort();
}

#[tokio::test]
async fn enrichment_batches_preserve_inputs_and_correlate_reordered_missing_failed_and_duplicate_answers() {
    use axum::{Json, routing::post};
    use book_inspection::{AudiobookMetadata, InspectedBook};
    use book_model::ChapterOrigin;
    use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentResponse, BookEnrichmentStatus, MAX_BOOKS_PER_REQUEST};

    type Requests = Arc<Mutex<Vec<BookEnrichmentRequest>>>;
    async fn answer(State((requests, fail_first)): State<(Requests, bool)>, Json(request): Json<BookEnrichmentRequest>) -> Response {
        let batch = {
            let mut requests = requests.lock().unwrap();
            let batch = requests.len();
            requests.push(request.clone());
            batch
        };
        if fail_first && batch == 0 {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        let mut results: Vec<_> = request
            .books
            .into_iter()
            .enumerate()
            .rev()
            .filter_map(|(index, facts)| {
                if batch == 0 && index == 2 {
                    return None;
                }
                let status = if batch == 0 && index == 1 { BookEnrichmentStatus::Failed } else { BookEnrichmentStatus::Found };
                let mut record = facts.record;
                record.book.year = Some(2000 + record.title.strip_prefix("Story ").unwrap().parse::<i32>().unwrap());
                Some(BookEnrichment { key: facts.key, status, record: (status == BookEnrichmentStatus::Found).then_some(record), toc: None, cover: None, failure: None, detail: String::new() })
            })
            .collect();
        // A consumed early answer must not move the later duplicate ahead of
        // the original answer for that key.
        let first = results.pop().unwrap();
        results.insert(0, first);
        let mut duplicate = results[1].clone();
        duplicate.record.as_mut().unwrap().book.year = Some(9999);
        results.push(BookEnrichment { key: "unrequested".into(), status: BookEnrichmentStatus::Found, record: None, toc: None, cover: None, failure: None, detail: String::new() });
        results.push(duplicate);
        Json(BookEnrichmentResponse { policy: "batch fixture".into(), results }).into_response()
    }

    for fail_first in [false, true] {
        let requests: Requests = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let app = Router::new().route("/v3/books", post(answer)).with_state((requests.clone(), fail_first));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let database = DatabaseClient::open(":memory:", &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        let mut hashes = Vec::new();
        for index in 0..MAX_BOOKS_PER_REQUEST + 2 {
            let hash = BookId::from_bytes([index as u8 + 1; 32]);
            let audio = index == 0;
            let format = if audio { BookFormat::M4b } else { BookFormat::Epub };
            let name = format!("Writer - Story {index}.{}", format.canonical_extension());
            let mut metadata = record();
            metadata.title = format!("Story {index}");
            metadata.description = "the file's description".into();
            let audiobook = audio.then(|| AudiobookMetadata {
                title: metadata.title.clone(),
                authors: metadata.contributors.clone(),
                description: metadata.description.clone(),
                duration_ms: 1000,
                chapters: vec![],
                tracks: vec![],
                chapter_origin: ChapterOrigin::FullAudiobookFallback,
                recording: Default::default(),
            });
            database
                .commit_import(ImportCommit {
                    source_identity: None,
                    byte_checksum: None,
                    audiobook_content_hash: None,
                    parent_id: ROOT_DIR_ID,
                    file_name: name.clone(),
                    format,
                    book_id: hash,
                    size_bytes: 10,
                    inspection_version: 1,
                    inspection: Some(InspectedBook { metadata, pdf: None, audiobook, toc: vec![] }),
                    published: PublishedStagedBook { name: name.clone(), relative_path: format!("/{name}"), published: false, fingerprint: None },
                    request_thumbnail: true,
                    restore_paths: vec![],
                })
                .await
                .unwrap();
            hashes.push(hash);
        }
        let original = database.enrichment_candidates(&hashes, 0).await.unwrap();
        let (events, _) = LibraryEventRouter::new();
        let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let lane = LibraryEnrichment {
            state: Arc::new(tokio::sync::RwLock::new(library_placement::LibraryState::new(database.clone(), assets.clone(), events.clone()))),
            database: database.clone(),

            client: MetadataClient::new(&origin).unwrap(),
            covers: CoverClient::default(),
            cpu: Arc::new(cpu_host::NativeHost),
            events,
        };
        let changed = lane.enrich_books(&hashes.iter().copied().collect()).await.unwrap();
        assert_eq!(changed, if fail_first { 2 } else { hashes.len() - 2 });
        server.abort();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.iter().map(|request| request.books.len()).collect::<Vec<_>>(), [MAX_BOOKS_PER_REQUEST, 2]);
        for (index, facts) in requests.iter().flat_map(|request| &request.books).enumerate() {
            let expected = &original[index];
            assert_eq!(facts.key, (index % MAX_BOOKS_PER_REQUEST).to_string());
            assert_eq!(serde_json::to_value(&facts.record).unwrap(), serde_json::to_value(&expected.record).unwrap());
            assert_eq!(facts.audiobook, expected.audiobook);
            assert!(!facts.needs_cover);
            assert_eq!(serde_json::to_value(&facts.recording).unwrap(), serde_json::to_value(&expected.recording).unwrap());
            assert_eq!(serde_json::to_value(&facts.readings).unwrap(), serde_json::to_value(book_inspection::title_readings(&expected.record, &expected.filenames)).unwrap());
            let failed = if fail_first { index < MAX_BOOKS_PER_REQUEST } else { index == 1 || index == 2 };
            let title = database.browse_book_detail(&expected.book_id).await.unwrap().book.title;
            assert_eq!(title, format!("Story {index}"), "the file's title is immutable");
            let resolved = database.enrichment_cover_record(&expected.book_id).await.unwrap();
            if failed {
                assert!(resolved.is_none());
            } else {
                assert_eq!(resolved.unwrap().book.year, Some(2000 + index as i32));
            }
        }
        let retry = database.enrichment_candidates(&hashes, i64::MAX).await.unwrap();
        let expected = if fail_first { hashes[..MAX_BOOKS_PER_REQUEST].to_vec() } else { vec![hashes[1], hashes[2]] };
        assert_eq!(retry.iter().map(|candidate| candidate.book_id).collect::<Vec<_>>(), expected);
        for candidate in retry {
            let expected = original.iter().find(|original| original.book_id == candidate.book_id).unwrap();
            assert_eq!(candidate.request_key, expected.request_key);
        }
        assert!(database.enrichment_next_retry().await.unwrap().is_some());
    }
}
