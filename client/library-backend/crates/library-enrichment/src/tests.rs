#![cfg(not(target_arch = "wasm32"))]

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use book_model::{BookDetails, BookFormat, BookRecord, Contributor, Identifier, Scheme, Scope, AUTHOR_MARC_RELATOR_CODE};
use image::{DynamicImage, ImageFormat, RgbImage};
use library_database::{Database, ImportCommit};
use library_files::asset_store::AssetStore;
use library_model::{BlobKind, PublishedStagedBook};
use library_runtime::{events::LibraryEventRouter, scheduler::SyncScheduler};
use std::{
    collections::{HashSet, VecDeque},
    io::Cursor,
    sync::{Arc, Mutex},
};
use sync_common::{BookId, ROOT_DIR_ID};

use super::{google_matches, isbn_key, isbns};
use crate::{CoverClient, LibraryEnrichment, MetadataClient};

struct Provider {
    origin: String,
    requests: Mutex<Vec<String>>,
    google_status: Mutex<VecDeque<StatusCode>>,
    google_image: Vec<u8>,
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
            assert!(!std::str::from_utf8(&bytes).unwrap().contains("test-key"));
            axum::Json(serde_json::json!({"policy":"books_v5","results":[{"key":request.books[0].key,"status":"no_match","record":null,"cover":null}]})).into_response()
        }
        "/google/volumes" => {
            let status = {
                let mut statuses = provider.google_status.lock().unwrap();
                if statuses.len() > 1 {
                    statuses.pop_front().unwrap()
                } else {
                    *statuses.front().unwrap()
                }
            };
            if !status.is_success() {
                return status.into_response();
            }
            axum::Json(serde_json::json!({"items":[{"id":"matched","volumeInfo":{"title":"Utilitarianism","authors":["John Stuart Mill"],
                "industryIdentifiers":[{"type":"ISBN_13","identifier":"9780306406157"}],
                "imageLinks":{"medium":format!("{}/images/google.png", provider.origin)}}}]}))
            .into_response()
        }
        "/images/google.png" => ([("content-type", "image/png")], provider.google_image.clone()).into_response(),
        "/ol/b/isbn/9780306406157-L.jpg" => ([("content-type", "image/png")], provider.fallback_image.clone()).into_response(),
        "/ol/search.json" => axum::Json(serde_json::json!({"docs":[]})).into_response(),
        _ => Response::builder().status(404).body(Body::empty()).unwrap(),
    }
}

async fn provider(status: StatusCode, width: u32) -> (CoverClient, Arc<Provider>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let provider = Arc::new(Provider {
        origin: origin.clone(),
        requests: Default::default(),
        google_status: Mutex::new(VecDeque::from([status])),
        google_image: artwork(width, width * 3 / 2, [240, 100, 0]),
        fallback_image: artwork(300, 450, [0, 100, 240]),
    });
    let app = Router::new().fallback(any(serve)).with_state(provider.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut client = CoverClient::new(Some("test-key".into()));
    client.google_origin = format!("{origin}/google/").parse().unwrap();
    client.openlibrary_origin = format!("{origin}/ol/").parse().unwrap();
    client.openlibrary_covers = client.openlibrary_origin.clone();
    client.interval_ms = 0;
    (client, provider, task)
}

#[tokio::test]
async fn google_artwork_is_downloaded_directly_before_openlibrary() {
    let (client, provider, task) = provider(StatusCode::OK, 600).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.google_image);
    let paths = provider.requests.lock().unwrap().clone();
    assert!(paths[0].starts_with("/google/volumes?"));
    assert!(paths[0].contains("key=test-key"));
    let search = url::Url::parse(&format!("{}{}", provider.origin, paths[0])).unwrap();
    assert_eq!(search.query_pairs().find(|(key, _)| key == "q").unwrap().1, "Utilitarianism John Stuart Mill");
    assert_eq!(paths[1], "/images/google.png");
    assert_eq!(paths.len(), 2);
    task.abort();
}

#[tokio::test]
async fn temporary_google_failures_retry_the_same_title_author_query() {
    let (client, provider, task) = provider(StatusCode::OK, 600).await;
    *provider.google_status.lock().unwrap() = VecDeque::from([StatusCode::SERVICE_UNAVAILABLE, StatusCode::SERVICE_UNAVAILABLE, StatusCode::OK]);
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.google_image);
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths.len(), 4);
    assert_eq!(paths[0], paths[1]);
    assert_eq!(paths[1], paths[2]);
    assert_eq!(paths[3], "/images/google.png");
    assert!(client.google_retry.lock().await.is_none());
    task.abort();
}

#[tokio::test]
async fn persistent_google_service_failure_has_bounded_retries_then_falls_back() {
    let (client, provider, task) = provider(StatusCode::SERVICE_UNAVAILABLE, 600).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.fallback_image);
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths.iter().filter(|path| path.starts_with("/google/volumes?")).count(), 4);
    assert_eq!(paths.last().unwrap(), "/ol/b/isbn/9780306406157-L.jpg?default=false");
    assert!(client.google_retry.lock().await.is_none());
    task.abort();
}

#[tokio::test]
async fn missing_author_skips_google_title_search_and_keeps_openlibrary_fallback() {
    let (client, provider, task) = provider(StatusCode::OK, 600).await;
    let mut book = record();
    book.contributors.clear();
    assert_eq!(client.book_cover(&book).await.unwrap().unwrap(), provider.fallback_image);
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths, ["/ol/b/isbn/9780306406157-L.jpg?default=false"]);
    task.abort();
}

#[tokio::test]
async fn google_quota_failure_falls_back_directly_and_waits_before_retrying_google() {
    let (client, provider, task) = provider(StatusCode::TOO_MANY_REQUESTS, 600).await;
    for _ in 0..2 {
        assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.fallback_image);
    }
    let paths = provider.requests.lock().unwrap().clone();
    assert_eq!(paths.iter().filter(|path| path.starts_with("/google/")).count(), 1);
    assert_eq!(paths.iter().filter(|path| path.starts_with("/ol/b/isbn/")).count(), 2);
    assert!(paths.iter().all(|path| !path.contains("v3/covers")));
    task.abort();
}

#[tokio::test]
async fn undersized_google_artwork_does_not_block_openlibrary() {
    let (client, provider, task) = provider(StatusCode::OK, 50).await;
    assert_eq!(client.book_cover(&record()).await.unwrap().unwrap(), provider.fallback_image);
    task.abort();
}

#[test]
fn cover_matching_rejects_other_authors_collections_languages_and_print_references() {
    let record = record();
    let authors = vec!["John Stuart Mill".into()];
    for value in [
        serde_json::json!({"title":"Utilitarianism","authors":["Other Author"]}),
        serde_json::json!({"title":"Utilitarianism","subtitle":"On Liberty and Other Essays","authors":authors}),
        serde_json::json!({"title":"Different Work","authors":authors}),
    ] {
        let info = serde_json::from_value(value).unwrap();
        assert!(!google_matches(&record, &[], &authors, &info));
    }
    let mut reference = record.clone();
    reference.book.identifiers[0].scope = Scope::Edition;
    assert!(isbns(&reference).is_empty());
    let info = serde_json::from_value(serde_json::json!({"title":"Utilitarianism","authors":authors})).unwrap();
    assert!(google_matches(&record, &[], &authors, &info));
    let mut english = record;
    english.book.languages.push(book_model::LanguageTag::parse("en").unwrap());
    let info = serde_json::from_value(serde_json::json!({"title":"Utilitarianism","authors":authors,"language":"fr"})).unwrap();
    assert!(!google_matches(&english, &[], &authors, &info));
    assert_eq!(isbn_key("0-306-40615-2"), isbn_key("9780306406157"));
    let info = serde_json::from_value(serde_json::json!({"industryIdentifiers":[{"type":"ISBN_10","identifier":"0306406152"}]})).unwrap();
    assert!(google_matches(&english, &["9780306406157".into()], &[], &info));
}

#[tokio::test]
async fn metadata_miss_still_downloads_direct_artwork_and_preserves_newer_local_cover() {
    let (covers, provider, task) = provider(StatusCode::OK, 600).await;
    let temporary = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(":memory:").unwrap());
    db.initialize_library().unwrap();
    let hash = BookId::new(&"a".repeat(64));
    db.commit_import(ImportCommit {
        parent_id: ROOT_DIR_ID,
        file_name: "book.epub".into(),
        format: BookFormat::Epub,
        content_hash: hash,
        size_bytes: 10,
        inspection_version: 1,
        inspection: Some(book_inspection::InspectedBook { metadata: record(), pdf: None, audiobook: None, toc: Vec::new() }),
        published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: Vec::new(),
    })
    .unwrap();
    db.complete_thumbnail_work(&hash, false).unwrap();
    let (sync, _) = SyncScheduler::new();
    let (assets_wake, _) = SyncScheduler::new();
    let (files, _) = SyncScheduler::new();
    let (enrichment, _) = SyncScheduler::new();
    let (events, _) = LibraryEventRouter::new(sync, assets_wake, files, enrichment);
    let assets = AssetStore::open(temporary.path().to_str().unwrap()).unwrap();
    let lane = LibraryEnrichment { database: db.clone(), assets: assets.clone(), client: MetadataClient::new(&format!("{}/metadata/", provider.origin)).unwrap(), covers, cpu: Arc::new(client_platform_native::cpu_host::NativeHost), events };
    assert_eq!(lane.enrich_books(&HashSet::from([hash])).await.unwrap(), 2);
    let original = assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap();
    assert!(!original.is_empty());
    assert!(db.enrichment_candidates(&[hash], 0).unwrap().is_empty());
    assert!(provider.requests.lock().unwrap().iter().all(|path| !path.contains("v3/covers")));
    let state = db.thumbnail_work_state(&hash).unwrap();
    let other = cpu_host::cover(&*lane.cpu, provider.fallback_image.clone()).await.unwrap();
    assert_eq!(lane.apply_enriched_cover(hash, &state, Some(&other)).await.unwrap(), None);
    assert_eq!(assets.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), original);
    task.abort();
}
