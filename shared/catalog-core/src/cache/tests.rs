use super::*;
use catalog_contract::{CatalogAuthorCredit, CatalogFreshness, CatalogPagination, CatalogPublicationIdentity, CatalogRecommendationExclusion, CatalogRecommendationRequest, CatalogSubjectPreference};
use library_model::PublicationSubject;

fn source_id() -> CatalogSourceId {
    CatalogSourceId::parse("opds-test").unwrap()
}

fn publication(id: &str, title: &str, author: &str, languages: &[&str]) -> CatalogPublication {
    CatalogPublication {
        identity: CatalogPublicationIdentity { edition_id: id.to_owned(), work_id: None },
        title: title.to_owned(),
        authors: vec![CatalogAuthorCredit { name: author.to_owned(), sort_name: None, identifiers: Vec::new(), provider_link: None }],
        languages: languages.iter().map(|language| (*language).to_owned()).collect(),
        description: None,
        image_url: None,
        actions: Vec::new(),
    }
}

fn importable_epub(id: &str, image_url: Option<&str>) -> CatalogPublication {
    let mut publication = publication(id, "EPUB", "Author", &["en"]);
    publication.image_url = image_url.map(str::to_owned);
    publication.actions.push(CatalogPublicationAction::Import {
        request: catalog_contract::CatalogImportRequest {
            source_id: source_id(),
            catalog_locator: "https://example.com/opds".to_owned(),
            publication: publication.identity.clone(),
            download_url: format!("https://example.com/{id}.epub"),
            book_format: CatalogBookFormat::Epub,
        },
    });
    publication
}

fn page(locator: &str, publications: Vec<CatalogPublication>) -> CatalogPage {
    CatalogPage {
        source_id: source_id(),
        freshness: CatalogFreshness::Current,
        title: "Books".to_owned(),
        locator: locator.to_owned(),
        navigation: Vec::new(),
        publications,
        groups: Vec::new(),
        facets: Vec::new(),
        pagination: CatalogPagination::default(),
        search: None,
    }
}

#[test]
fn cache_connections_use_low_sync_wal_settings() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let connection = cache.pool.get().unwrap();

    assert_eq!(connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0)).unwrap(), "wal");
    assert_eq!(connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn local_search_separates_title_and_author_matches() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache
        .put_page(
            &source_id(),
            "https://example.com/opds",
            &page("https://example.com/opds", vec![publication("title", "Cien años de soledad", "Someone", &["es"]), publication("author", "Chronicle", "Gabriel García Márquez", &["es"])]),
            None,
            None,
        )
        .unwrap();
    let matches = cache.search(&source_id(), "garcia marquez").unwrap();
    assert!(matches.title_matches.is_empty());
    assert_eq!(matches.author_matches[0].publication.identity.edition_id, "author");
    let matches = cache.search(&source_id(), "cien anos").unwrap();
    assert_eq!(matches.title_matches[0].publication.identity.edition_id, "title");
    assert!(matches.author_matches.is_empty());
}

#[test]
fn local_search_returns_distinct_table_of_contents_matches() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let book = publication("toc", "Collected Essays", "Ada Author", &["en"]);
    cache.put_page(&source_id(), "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
    let toc = vec![library_model::PublicationTocEntry {
        title: "Industrial transformation".to_owned(),
        target: "chapter.xhtml".to_owned(),
        children: vec![library_model::PublicationTocEntry { title: "Swedish railways".to_owned(), target: "chapter.xhtml#rail".to_owned(), children: Vec::new() }],
    }];
    assert!(cache.enrich_active_publication(&source_id(), &book, &[], &toc, None).unwrap());

    let matches = cache.search(&source_id(), "railways").unwrap();
    assert!(matches.title_matches.is_empty());
    assert!(matches.author_matches.is_empty());
    assert_eq!(matches.toc_matches[0].publication.identity.edition_id, "toc");
}

#[test]
fn failed_staging_keeps_active_generation_and_activation_reuses_unchanged_sequences() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let first = page("https://example.com/opds", vec![publication("one", "One", "Author", &["en"]), publication("two", "Two", "Author", &["en"])]);
    cache.put_page(&source_id(), "https://example.com/opds", &first, Some("v1"), None).unwrap();
    let original = cache.search(&source_id(), "one").unwrap().title_matches[0].sequence;

    let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_page(&failed, &page("https://example.com/opds", vec![publication("replacement", "Replacement", "Author", &["en"])]), None, None).unwrap();
    cache.abandon_crawl(&failed).unwrap();
    assert_eq!(cache.search(&source_id(), "one").unwrap().title_matches[0].sequence, original);

    let successful = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_page(&successful, &first, Some("v1"), None).unwrap();
    cache.activate_crawl(&successful).unwrap();
    assert_eq!(cache.search(&source_id(), "one").unwrap().title_matches[0].sequence, original);
}

#[test]
fn source_url_and_provider_epoch_changes_invalidate_only_that_source() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.put_page(&source_id(), "https://example.com/one", &page("https://example.com/one", vec![publication("one", "One", "Author", &["en"])]), None, None).unwrap();
    assert!(!cache.set_provider_epoch(&source_id(), "https://example.com/one", "server-a").unwrap());
    assert!(cache.set_provider_epoch(&source_id(), "https://example.com/one", "server-b").unwrap());
    assert!(cache.search(&source_id(), "one").unwrap().title_matches.is_empty());

    cache.put_page(&source_id(), "https://example.com/one", &page("https://example.com/one", vec![publication("one", "One", "Author", &["en"])]), None, None).unwrap();
    let old_epoch = cache.source_state(&source_id()).unwrap().unwrap().epoch;
    cache.ensure_source(&source_id(), "https://example.com/two").unwrap();
    let state = cache.source_state(&source_id()).unwrap().unwrap();
    assert_ne!(state.epoch, old_epoch);
    assert!(cache.search(&source_id(), "one").unwrap().title_matches.is_empty());
}

#[test]
fn known_page_publications_returns_references_without_building_a_response() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let locator = "https://example.com/opds";
    cache.put_page(&source_id(), locator, &page(locator, vec![publication("one", "One", "Author", &["en"]), publication("two", "Two", "Author", &["en"])]), None, None).unwrap();

    let references = cache.known_page_publications(&source_id(), locator).unwrap();
    assert_eq!(references.iter().map(|reference| reference.identity.edition_id.as_str()).collect::<Vec<_>>(), vec!["one", "two"]);
    assert!(references.iter().all(|reference| reference.sequence > 0));
    assert!(cache.known_page_publications(&source_id(), "https://example.com/missing").unwrap().is_empty());
}

#[test]
fn page_response_sorts_books_by_open_library_popularity_and_preserves_ties() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let locator = "https://example.com/opds";
    let first = publication("first", "First", "Author", &["en"]);
    let popular = publication("popular", "Popular", "Author", &["en"]);
    let third = publication("third", "Third", "Author", &["en"]);
    cache.put_page(&source_id(), locator, &page(locator, vec![first, popular.clone(), third]), None, None).unwrap();
    cache.enrich_active_publication(&source_id(), &popular, &[], &[], Some(&CatalogPublicationPopularity { want_to_read_count: 20, currently_reading_count: 3, already_read_count: 7, rating_count: 4, rating_sum: 18 })).unwrap();

    let response = cache.page_response(&source_id(), locator, None, &[]).unwrap().unwrap();
    assert_eq!(response.page.publications.iter().map(|publication| publication.identity.edition_id.as_str()).collect::<Vec<_>>(), vec!["popular", "first", "third"]);
}

#[test]
fn replaces_an_outdated_cache_instead_of_migrating_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version=1;
                 CREATE TABLE migration_sentinel(value TEXT NOT NULL);
                 INSERT INTO migration_sentinel VALUES('preserved');",
        )
        .unwrap();
    drop(connection);

    let cache = CatalogCache::open(&path).unwrap();
    let inventory = cache.inventory().unwrap();
    assert_eq!(inventory.active_page_count, 0);
    assert_eq!(inventory.active_publication_count, 0);
    assert_eq!(inventory.active_epub_count, 0);
    drop(cache);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), CATALOG_CACHE_FILE_VERSION);
    assert_eq!(connection.query_row("SELECT schema_version FROM catalog_cache_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).unwrap(), CATALOG_CACHE_SCHEMA_VERSION);
    assert!(connection.query_row("SELECT value FROM migration_sentinel", [], |row| row.get::<_, String>(0)).is_err());
}

#[test]
fn current_cache_adds_failure_diagnostics_without_discarding_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    {
        let cache = CatalogCache::open(&path).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    }
    let connection = Connection::open(&path).unwrap();
    connection.execute("DROP TABLE catalog_epub_failure", ()).unwrap();
    drop(connection);

    let cache = CatalogCache::open(&path).unwrap();
    assert_eq!(cache.inventory().unwrap().sources.len(), 1);
    let connection = Connection::open(&path).unwrap();
    assert!(connection.query_row("SELECT COUNT(*) FROM catalog_epub_failure", (), |row| row.get::<_, i64>(0)).is_ok());
}

#[test]
fn crawl_status_and_counts_change_atomically_without_losing_active_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "running");
    cache.stage_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();
    cache.activate_crawl(&crawl).unwrap();

    let succeeded = cache.inventory().unwrap();
    assert_eq!((succeeded.active_page_count, succeeded.active_publication_count, succeeded.active_epub_count), (1, 1, 1));
    assert_eq!(succeeded.sources[0].crawl_status, "succeeded");
    assert!(succeeded.sources[0].last_crawl_started_at.is_some());
    assert!(succeeded.sources[0].last_crawl_succeeded_at.is_some());

    let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_page(&failed, &page("https://example.com/opds", Vec::new()), None, None).unwrap();
    cache.abandon_crawl_with_error(&failed, Some("upstream timeout")).unwrap();
    let inventory = cache.inventory().unwrap();
    assert_eq!((inventory.active_page_count, inventory.active_publication_count, inventory.active_epub_count), (1, 1, 1));
    assert_eq!(inventory.sources[0].crawl_status, "failed");
    assert_eq!(inventory.sources[0].last_crawl_error.as_deref(), Some("upstream timeout"));
}

#[test]
fn crawl_activation_and_extraction_reconciliation_are_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    cache.ensure_source(&source, "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    cache.stage_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None), importable_epub("two", None), importable_epub("three", None)]), None, None).unwrap();

    assert!(cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
    assert!(cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
    assert!(!cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
    cache.finalize_crawl_activation(&crawl).unwrap();

    assert!(cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
    assert!(cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
    assert!(!cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
    assert_eq!(cache.inventory().unwrap().epub_extractions.pending, 3);
}

#[test]
fn incremental_crawl_publishes_pages_and_preserves_partial_progress() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    cache.begin_incremental_crawl(&source, "https://example.com/opds").unwrap();
    cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();

    let running = cache.inventory().unwrap();
    assert_eq!((running.active_page_count, running.active_publication_count), (1, 1));
    assert_eq!(running.sources[0].crawl_status, "running");

    cache.finish_incremental_crawl(&source, false).unwrap();
    let partial = cache.inventory().unwrap();
    assert_eq!((partial.active_page_count, partial.active_publication_count), (1, 1));
    assert_eq!(partial.sources[0].crawl_status, "partial");
    assert!(cache.page(&source, "https://example.com/opds").unwrap().is_some());
}

#[test]
fn epub_extraction_inventory_persists_and_requeues_interrupted_work() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let source = source_id();
    let succeeded = "https://example.com/succeeded.epub".to_owned();
    let failed = "https://example.com/failed.epub".to_owned();
    let interrupted = "https://example.com/interrupted.epub".to_owned();

    {
        let cache = CatalogCache::open(&path).unwrap();
        cache.ensure_source(&source, "https://example.com/opds").unwrap();
        cache.queue_crawls(std::slice::from_ref(&source)).unwrap();
        assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "queued");
        cache.queue_epub_extractions(&source, &[succeeded.clone(), failed.clone(), interrupted.clone()], false).unwrap();
        assert!(cache.begin_epub_extraction(&source, &succeeded).unwrap());
        cache.finish_epub_extraction(&source, &succeeded, 12_345, &publication("one", "One", "Author", &["en"]), None).unwrap();
        cache.begin_epub_extraction(&source, &failed).unwrap();
        cache.fail_epub_extraction(&source, &failed, "invalid EPUB", "invalid_publication", false).unwrap();
        cache.begin_epub_extraction(&source, &interrupted).unwrap();
        let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 1, 1, 1));
    }

    let reopened = CatalogCache::open(&path).unwrap();
    assert_eq!(reopened.inventory().unwrap().sources[0].crawl_status, "queued");
    let extraction = &reopened.inventory().unwrap().sources[0].epub_extractions;
    assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (1, 0, 1, 1));
    assert_eq!(extraction.attempts, 3);
    assert_eq!(extraction.downloaded_bytes, 12_345);
    assert_eq!(reopened.successful_epub_extractions(&source).unwrap(), HashSet::from([succeeded]));
    assert_eq!(reopened.pending_epub_extractions(&source).unwrap(), HashSet::from([interrupted]));
    let failures = reopened.epub_failure_diagnostics().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!((failures[0].category.as_str(), failures[0].retryable, failures[0].count), ("invalid_publication", false, 1));
    assert_eq!(failures[0].examples[0].acquisition_host, "example.com");
    reopened.retry_failed_epub_extractions(&source).unwrap();
    assert_eq!(reopened.inventory().unwrap().epub_extractions.failed, 1, "permanent failures must not be blindly requeued");
}

#[test]
fn saved_metadata_backfill_is_durable_and_does_not_requeue_downloads() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let source = source_id();
    let book = importable_epub("saved", None);
    {
        let cache = CatalogCache::open(&path).unwrap();
        cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
        let extraction = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
        let metadata =
            CatalogEpubMetadata { book: library_model::BookMetadataState { title: "Saved".to_owned(), contributors: Vec::new(), description: String::new(), publication: library_model::PublicationMetadata::default() }, toc: Vec::new() };
        cache.finish_epub_extraction(&source, &extraction.acquisition_url, 123, &book, Some(&metadata)).unwrap();
        assert_eq!(cache.schedule_metadata_backfill().unwrap(), 1);
        assert_eq!(cache.claim_metadata_backfill_batch(128).unwrap().len(), 1);
        assert_eq!(cache.inventory().unwrap().epub_extractions.attempts, 1);
    }

    let cache = CatalogCache::open(&path).unwrap();
    let jobs = cache.claim_metadata_backfill_batch(128).unwrap();
    assert_eq!(jobs.len(), 1, "an interrupted metadata-only batch must resume after restart");
    cache
        .finish_metadata_backfill_batch(&[CatalogMetadataBackfillUpdate {
            source_id: source.clone(),
            publication: book.identity,
            acquisition_url: jobs[0].acquisition_url.clone(),
            subjects: Vec::new(),
            popularity: None,
            metadata: jobs[0].metadata.clone(),
            identity_result: None,
        }])
        .unwrap();
    assert!(cache.claim_metadata_backfill_batch(128).unwrap().is_empty());
    assert_eq!(cache.inventory().unwrap().epub_extractions.attempts, 1, "metadata backfill must not claim or download the publication again");
}

#[test]
fn description_backfill_is_separate_and_only_fills_missing_descriptions() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogCache::open(&path).unwrap();
    let source = source_id();
    let missing = importable_epub("missing-description", None);
    let without_open_library_description = importable_epub("without-open-library-description", None);
    let mut existing = importable_epub("existing-description", None);
    existing.description = Some(CatalogDescription::PlainText { text: "Feed description".to_owned() });
    cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![missing.clone(), without_open_library_description.clone(), existing.clone()]), None, None).unwrap();

    for book in [&missing, &without_open_library_description, &existing] {
        let acquisition_url = publication_extraction(book).unwrap().acquisition_url.to_owned();
        assert!(cache.begin_epub_extraction(&source, &acquisition_url).unwrap());
        cache.finish_epub_extraction(&source, &acquisition_url, 123, book, None).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO catalog_metadata_index(source_id,acquisition_url,version,title,publication_year,authors_payload,publishers_payload,isbns_payload) VALUES(?1,?2,?3,?4,NULL,?5,?5,?6)",
                params![source.as_str(), acquisition_url, CATALOG_METADATA_INDEX_VERSION, book.title, wire::encode(&Vec::<String>::new()).unwrap(), wire::encode(&vec!["9780306406157".to_owned()]).unwrap()],
            )
            .unwrap();
        connection.execute("INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status) VALUES(?1,?2,?3,'succeeded')", params![source.as_str(), acquisition_url, CATALOG_METADATA_BACKFILL_VERSION]).unwrap();
    }

    assert_eq!(cache.schedule_description_backfill().unwrap(), 3);
    assert_eq!(cache.inventory().unwrap().description_backfill.pending, 3);
    let jobs = cache.claim_description_backfill_batch(10).unwrap();
    assert_eq!(jobs.len(), 3);
    assert_eq!(cache.inventory().unwrap().description_backfill.running, 3);
    assert_eq!(jobs.iter().find(|job| job.publication == missing.identity).unwrap().isbns, ["9780306406157"]);
    assert_eq!(jobs.iter().find(|job| job.publication == existing.identity).unwrap().isbns, ["9780306406157"], "books with descriptions still need validated work subjects");

    let updates = jobs
        .iter()
        .map(|job| CatalogDescriptionBackfillUpdate {
            source_id: job.source_id.clone(),
            publication: job.publication.clone(),
            acquisition_url: job.acquisition_url.clone(),
            description: (job.publication != without_open_library_description.identity).then(|| "Open Library description".to_owned()),
            subjects: (job.publication == missing.identity)
                .then(|| vec![library_model::PublicationSubject::new(None, "HIS015000", "openlibrary:2026-08-01:work", Some("bisac".to_owned()), Some("HIS015000".to_owned())).unwrap()])
                .unwrap_or_default(),
            open_library_description_missing: job.publication == without_open_library_description.identity,
        })
        .collect::<Vec<_>>();
    cache.finish_description_backfill_batch(&updates).unwrap();

    assert_eq!(cache.active_publication(&source, &missing.identity).unwrap().unwrap().description, Some(CatalogDescription::PlainText { text: "Open Library description".to_owned() }));
    assert_eq!(cache.active_publication(&source, &without_open_library_description.identity).unwrap().unwrap().description, None);
    assert_eq!(cache.active_publication(&source, &existing.identity).unwrap().unwrap().description, Some(CatalogDescription::PlainText { text: "Feed description".to_owned() }));
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM catalog_publication_subject WHERE source_id=?1 AND edition_id=?2 AND authority='bisac' AND code='HIS015000'", params![source.as_str(), missing.identity.edition_id], |row| row.get::<_, i64>(0),)
            .unwrap(),
        1
    );
    let mut refreshed = page("https://example.com/opds", vec![missing.clone()]);
    cache.enrich_page_from_epub_extractions(&source, &mut refreshed).unwrap();
    assert_eq!(refreshed.publications[0].description, Some(CatalogDescription::PlainText { text: "Open Library description".to_owned() }));
    assert!(cache.claim_description_backfill_batch(10).unwrap().is_empty());
    assert_eq!(cache.inventory().unwrap().description_backfill.succeeded, 2);
    assert_eq!(cache.inventory().unwrap().description_backfill.no_description, 1);
    assert_eq!(cache.schedule_metadata_backfill().unwrap(), 0, "description backfill must not requeue regular metadata enrichment");
}

#[test]
fn ambiguous_identity_backfill_can_be_reviewed_and_resolved_manually() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let book = importable_epub("review", None);
    cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
    let extraction = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    let metadata = CatalogEpubMetadata {
        book: library_model::BookMetadataState { title: "A title needing review".to_owned(), contributors: Vec::new(), description: String::new(), publication: library_model::PublicationMetadata::default() },
        toc: Vec::new(),
    };
    cache.finish_epub_extraction(&source, &extraction.acquisition_url, 123, &book, Some(&metadata)).unwrap();
    cache.schedule_metadata_backfill().unwrap();
    let job = cache.claim_metadata_backfill_batch(1).unwrap().pop().unwrap();
    let candidate = metadata_contract::EditionIdentityMatch {
        canonical_isbn13: "9780306406157".to_owned(),
        open_library_edition_id: "OL123M".to_owned(),
        open_library_work_id: Some("OL456W".to_owned()),
        title: "A title needing review".to_owned(),
        work_title: None,
        title_match: metadata_contract::EditionTitleMatch::Edition,
        authors: Vec::new(),
        publishers: Vec::new(),
        publication_year: None,
        matched_publisher: false,
        matched_publication_year: false,
        classifications: Vec::new(),
    };
    cache
        .finish_metadata_backfill_batch(&[CatalogMetadataBackfillUpdate {
            source_id: source.clone(),
            publication: book.identity,
            acquisition_url: job.acquisition_url,
            subjects: Vec::new(),
            popularity: None,
            metadata: job.metadata,
            identity_result: Some(metadata_contract::EditionIdentityResult {
                query_id: "review-query".to_owned(),
                status: metadata_contract::EditionIdentityStatus::Ambiguous,
                matched: None,
                candidates: vec![candidate],
                candidates_truncated: false,
            }),
        }])
        .unwrap();

    let inventory = cache.identity_review_inventory_for_source(&source).unwrap();
    assert_eq!(inventory.ambiguous, 1);
    let review = cache.identity_reviews(Some(&source), None, 10).unwrap().pop().unwrap();
    assert_eq!(review.candidates[0].isbn13, "9780306406157");
    assert_eq!(cache.identity_review_candidate_isbn(&review.review_id, "OL123M", "9780306406157").unwrap().as_deref(), Some("9780306406157"));
    assert!(cache.resolve_identity_review_with_isbn(&review.review_id, "9780306406157").unwrap());
    assert_eq!(cache.identity_review_inventory_for_source(&source).unwrap().resolved, 1);
    assert!(cache.identity_reviews(Some(&source), None, 10).unwrap().is_empty());
    assert_eq!(cache.claim_metadata_backfill_batch(1).unwrap().len(), 1, "manual selection must queue classification backfill without downloading the EPUB again");
    assert!(cache.epub_metadata(&source, "https://example.com/review.epub").unwrap().unwrap().book.publication.identifiers.iter().any(|identifier| identifier.value() == "9780306406157"));
}

#[test]
fn durable_extraction_queue_claims_bounded_jobs_and_discards_stale_acquisitions() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let first = importable_epub("one", Some("urn:cover:one"));
    let second = importable_epub("two", None);
    cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![first.clone(), second.clone()]), None, None).unwrap();

    let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    assert_eq!(claimed.publication, first.identity);
    assert_eq!(claimed.acquisition_url, "https://example.com/one.epub");
    assert_eq!(claimed.book_format, CatalogBookFormat::Epub);
    assert_eq!(claimed.cover_key.as_deref(), Some("urn:cover:one"));
    let inventory = cache.inventory().unwrap().epub_extractions;
    assert_eq!((inventory.pending, inventory.running), (1, 1));

    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    cache.stage_page(&crawl, &page("https://example.com/opds", vec![second.clone()]), None, None).unwrap();
    cache.activate_crawl(&crawl).unwrap();

    assert_eq!(cache.epub_extraction_status(&source, "https://example.com/one.epub").unwrap(), None);
    let remaining = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    assert_eq!(remaining.publication, second.identity);
    assert!(cache.claim_next_epub_extraction(&source).unwrap().is_none());
}

#[test]
fn page_batches_commit_incremental_and_replacement_pages_atomically() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let root = "https://example.com/opds";
    let child = "https://example.com/child";
    cache
        .put_pages(
            &source,
            root,
            &[
                CatalogPageWrite { page: page(root, vec![publication("one", "One", "Author", &["en"])]), etag: Some("root-v1".to_owned()), last_modified: None },
                CatalogPageWrite { page: page(child, vec![publication("two", "Two", "Author", &["en"])]), etag: Some("child-v1".to_owned()), last_modified: None },
            ],
        )
        .unwrap();
    assert_eq!(cache.pages(&source).unwrap().len(), 2);
    assert_eq!(cache.page(&source, child).unwrap().unwrap().etag.as_deref(), Some("child-v1"));
    assert_eq!(cache.inventory().unwrap().active_page_count, 0, "incremental batches must not recount the active generation");
    cache.finish_incremental_crawl(&source, true).unwrap();
    assert_eq!(cache.inventory().unwrap().active_page_count, 2, "crawl completion performs the single exact recount");

    let crawl = cache.begin_crawl(&source, root).unwrap();
    cache
        .stage_pages(
            &crawl,
            &[
                CatalogPageWrite { page: page(root, vec![publication("three", "Three", "Author", &["en"])]), etag: Some("root-v2".to_owned()), last_modified: None },
                CatalogPageWrite { page: page(child, Vec::new()), etag: Some("child-v2".to_owned()), last_modified: None },
            ],
        )
        .unwrap();
    assert_eq!(cache.page(&source, root).unwrap().unwrap().page.publications[0].identity.edition_id, "one", "staging must not leak before activation");
    cache.activate_crawl(&crawl).unwrap();
    assert_eq!(cache.page(&source, root).unwrap().unwrap().page.publications[0].identity.edition_id, "three");
    assert_eq!(cache.page(&source, child).unwrap().unwrap().etag.as_deref(), Some("child-v2"));
}

#[test]
fn unchanged_publications_preserve_sequence_and_fts_metadata_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogCache::open(&path).unwrap();
    let source = source_id();
    let root = "https://example.com/opds";
    let books = page(root, vec![publication("same", "Same", "Stable Author", &["en"])]);
    cache.put_page(&source, root, &books, None, None).unwrap();
    let before = cache.page_response(&source, root, None, &[]).unwrap().unwrap().payloads[0].sequence;
    let rowid_before = Connection::open(&path).unwrap().query_row("SELECT rowid FROM catalog_publication_author WHERE source_id=?1 AND normalized_name='stable author'", [source.as_str()], |row| row.get::<_, i64>(0)).unwrap();

    cache.put_page(&source, root, &books, None, None).unwrap();

    let after = cache.page_response(&source, root, None, &[]).unwrap().unwrap().payloads[0].sequence;
    let rowid_after = Connection::open(&path).unwrap().query_row("SELECT rowid FROM catalog_publication_author WHERE source_id=?1 AND normalized_name='stable author'", [source.as_str()], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(after, before);
    assert_eq!(rowid_after, rowid_before, "unchanged author metadata must not be deleted and reinserted");
}

#[test]
fn page_graph_frontier_and_enrichment_are_resolved_without_loading_all_pages() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let root_url = "https://example.com/opds";
    let child_url = "https://example.com/child";
    let mut root = page(root_url, vec![importable_epub("one", None)]);
    root.navigation.push(CatalogLink { title: "Child".to_owned(), target: CatalogLinkTarget::Page { locator: child_url.to_owned() }, number_of_items: None, active: false });
    cache.put_page(&source, root_url, &root, None, None).unwrap();
    assert_eq!(cache.missing_page_frontier(&source, root_url).unwrap(), [child_url]);

    let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    let mut enriched = importable_epub("one", None);
    enriched.title = "Embedded title".to_owned();
    cache.finish_epub_extraction(&source, &claimed.acquisition_url, 10, &enriched, None).unwrap();
    let mut refreshed_root = root.clone();
    cache.enrich_page_from_epub_extractions(&source, &mut refreshed_root).unwrap();
    assert_eq!(refreshed_root.publications[0].title, "Embedded title");

    cache.put_page(&source, root_url, &page(child_url, Vec::new()), None, None).unwrap();
    assert!(cache.missing_page_frontier(&source, root_url).unwrap().is_empty());
}

#[test]
fn epub_extraction_inventory_discards_urls_no_longer_in_the_catalogue() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let retained = "https://example.com/retained.epub".to_owned();
    let removed = "https://example.com/removed.epub".to_owned();
    cache.ensure_source(&source, "https://example.com/opds").unwrap();
    cache.queue_epub_extractions(&source, &[retained.clone(), removed], false).unwrap();
    cache.retain_epub_extractions(&source, &HashSet::from([retained])).unwrap();

    let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
    assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (1, 0, 0, 0));
}

#[test]
fn subject_browse_reuses_projected_hierarchy_and_paginates_sourced_books() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    let mut books = page("https://example.com/opds", vec![importable_epub("history-one", None), importable_epub("history-two", None)]);
    books.publications[0].title = "First History".to_owned();
    books.publications[1].title = "Second History".to_owned();
    cache.stage_page(&crawl, &books, None, None).unwrap();
    let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
    let dewey = PublicationSubject::new(None, "515.33", "openlibrary", Some("ddc".to_owned()), Some("515.33".to_owned())).unwrap();
    cache.stage_publication_subjects(&crawl, &books.publications[0].identity, &[history.clone(), dewey]).unwrap();
    cache.stage_publication_subjects(&crawl, &books.publications[1].identity, &[history]).unwrap();
    cache.activate_crawl(&crawl).unwrap();
    cache
        .enrich_active_publication(
            &source_id(),
            &books.publications[1],
            &[PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap()],
            &[],
            Some(&CatalogPublicationPopularity { want_to_read_count: 40, currently_reading_count: 2, already_read_count: 8, rating_count: 3, rating_sum: 13 }),
        )
        .unwrap();

    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
    assert_eq!(root.navigation.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), ["Humanities", "Science & Nature"]);
    assert!(root.publications.is_empty());
    let humanities = root.navigation.iter().find(|link| link.title == "Humanities").unwrap();
    let CatalogLinkTarget::Page { locator } = &humanities.target else { panic!("subject roots must be internal links") };
    let humanities_locator = locator.clone();
    let mut locator = locator.clone();
    for expected_child in ["History", "History by Region", "Americas", "North America", "United States", "Historical Eras", "20th Century"] {
        let level = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
        let child = level.navigation.iter().find(|link| link.title == expected_child).unwrap_or_else(|| panic!("missing unified subject child {expected_child}"));
        let CatalogLinkTarget::Page { locator: child_locator } = &child.target else { panic!("subject children must be internal links") };
        locator = child_locator.clone();
    }

    let first = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
    assert!(first.navigation.is_empty());
    assert_eq!(first.publications.len(), 1);
    assert_eq!(first.publications[0].source_id, source_id());
    assert_eq!(first.publications[0].publication.identity.edition_id, "history-two", "popular books sort first within an equally relevant subject");
    assert_eq!(first.pagination.number_of_items, Some(2));
    let second = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: first.pagination.next.clone(), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
    assert_eq!(second.publications.len(), 1);
    assert_ne!(first.publications[0].publication.identity, second.publications[0].publication.identity);

    let excluded_source = CatalogSourceId::parse("another-source").unwrap();
    let filtered_root = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: vec![excluded_source], query: String::new(), limit: 10 });
    assert!(filtered_root.unwrap().navigation.is_empty());

    let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(humanities_locator), supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "FIRST HISTORY".to_owned(), limit: 10 });
    let searched = searched.unwrap();
    assert!(searched.navigation.is_empty());
    assert_eq!(searched.publications.len(), 1);
    assert_eq!(searched.publications[0].publication.identity.edition_id, "history-one");
}

#[test]
fn subject_browse_uses_gutenberg_downloads_when_open_library_popularity_is_missing() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    let mut alphabetical_first = importable_epub("urn:gutenberg:1:1", None);
    alphabetical_first.title = "A lesser-known book".to_owned();
    alphabetical_first.description = Some(CatalogDescription::PlainText { text: "Downloads: 12 Language: English".to_owned() });
    let mut popular = importable_epub("urn:gutenberg:2:1", None);
    popular.title = "The popular book".to_owned();
    popular.description = Some(CatalogDescription::PlainText { text: "Downloads: 45000 Language: English".to_owned() });
    let books = page("https://example.com/opds", vec![alphabetical_first, popular]);
    cache.stage_page(&crawl, &books, None, None).unwrap();
    let fiction = PublicationSubject::new(None, "Fiction / General", "dc:subject", Some("bisac".to_owned()), Some("FIC000000".to_owned())).unwrap();
    for publication in &books.publications {
        cache.stage_publication_subjects(&crawl, &publication.identity, std::slice::from_ref(&fiction)).unwrap();
    }
    cache.activate_crawl(&crawl).unwrap();

    let response = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: "book".to_owned(), limit: 10 }).unwrap();
    assert_eq!(response.publications[0].publication.title, "The popular book");
}

#[test]
fn subject_browse_filters_and_paginates_inside_sqlite() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    let mut books = page("https://example.com/opds", vec![importable_epub("sqlite-one", None), importable_epub("sqlite-two", None)]);
    books.publications[0].title = "First indexed history".to_owned();
    books.publications[1].title = "Second indexed history".to_owned();
    cache.stage_page(&crawl, &books, None, None).unwrap();
    cache.activate_crawl(&crawl).unwrap();
    let generation = cache.source_state(&source).unwrap().unwrap().active_generation;
    let connection = cache.pool.get().unwrap();
    for publication in &books.publications {
        connection
            .execute(
                "INSERT INTO catalog_publication_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version)
                     VALUES(?1,?2,?3,'','unified','Humanities / History',1)",
                params![source.as_str(), storage_i64(generation, "generation").unwrap(), publication.identity.edition_id],
            )
            .unwrap();
    }
    drop(connection);

    assert!(!cache.search_index_ready().unwrap());
    assert!(cache.refresh_search_index().unwrap());
    assert!(cache.search_index_ready().unwrap());

    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: vec![source.clone()], query: String::new(), limit: 1 }).unwrap();
    assert_eq!(root.navigation.len(), 1);
    assert_eq!(root.navigation[0].title, "Humanities");
    assert_eq!(root.navigation[0].number_of_items, Some(2));
    assert!(root.publications.is_empty());

    let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: vec![source], query: "FIRST".to_owned(), limit: 1 }).unwrap();
    assert_eq!(searched.publications.len(), 1);
    assert_eq!(searched.publications[0].publication.identity.edition_id, "sqlite-one");
    assert_eq!(searched.pagination.number_of_items, None);
    assert_eq!(searched.pagination.last, None);

    let first_search_page = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "indexed history".to_owned(), limit: 1 }).unwrap();
    assert_eq!(first_search_page.publications.len(), 1);
    assert!(first_search_page.pagination.next.is_some());
    assert_eq!(first_search_page.pagination.number_of_items, None);
    let second_search_page =
        cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: first_search_page.pagination.next, supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "indexed history".to_owned(), limit: 1 }).unwrap();
    assert_eq!(second_search_page.publications.len(), 1);
    assert!(second_search_page.pagination.next.is_none());
    assert_ne!(first_search_page.publications[0].publication.identity, second_search_page.publications[0].publication.identity);

    let connection = cache.pool.get().unwrap();
    let query_plan = connection
        .prepare("EXPLAIN QUERY PLAN SELECT rowid FROM catalog_publication_title_fts WHERE catalog_publication_title_fts MATCH '\"history\"'")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(query_plan.iter().any(|detail| detail.contains("VIRTUAL TABLE INDEX")), "FTS query must use the trigram virtual-table index: {query_plan:?}");

    connection
        .execute(
            "INSERT OR REPLACE INTO catalog_publication_author(source_id,generation,edition_id,work_id,position,normalized_name)
                 VALUES(?1,?2,'sqlite-two','',0,'authorneedle')",
            params![source_id().as_str(), storage_i64(generation, "generation").unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO catalog_publication_toc(source_id,generation,edition_id,work_id,position,normalized_title)
                 VALUES(?1,?2,'sqlite-two','',0,'tocneedle')",
            params![source_id().as_str(), storage_i64(generation, "generation").unwrap()],
        )
        .unwrap();
    drop(connection);

    for query in ["authorneedle", "tocneedle"] {
        let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: query.to_owned(), limit: 10 });
        let searched = searched.unwrap();
        assert_eq!(searched.publications.len(), 1, "FTS trigger did not index {query}");
        assert_eq!(searched.publications[0].publication.identity.edition_id, "sqlite-two");
    }
}

#[test]
fn existing_cache_backfill_rebuilds_the_unified_subject_projection() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    {
        let cache = CatalogCache::open(&path).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        let books = page("https://example.com/opds", vec![importable_epub("history", None)]);
        cache.stage_page(&crawl, &books, None, None).unwrap();
        let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
        cache.stage_publication_subjects(&crawl, &books.publications[0].identity, &[history]).unwrap();
        cache.activate_crawl(&crawl).unwrap();

        let connection = cache.pool.get().unwrap();
        connection.execute("DELETE FROM catalog_publication_subject_assignment WHERE system_id=?1", [library_model::UNIFIED_SYSTEM_ID]).unwrap();
        connection.execute("DELETE FROM catalog_subject_projection_metadata", []).unwrap();
    }

    let cache = CatalogCache::open(&path).unwrap();
    cache.refresh_unified_subject_assignments().unwrap();
    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: Vec::new(), source_ids: Vec::new(), query: String::new(), limit: 10 }).unwrap();
    assert!(root.navigation.iter().any(|link| link.title == "Humanities"));
    let connection = cache.pool.get().unwrap();
    let unified_count = connection.query_row("SELECT COUNT(*) FROM catalog_publication_subject_assignment WHERE system_id=?1", [library_model::UNIFIED_SYSTEM_ID], |row| row.get::<_, i64>(0)).unwrap();
    assert!(unified_count > 0);
}

#[test]
fn canonical_subject_recommendations_rank_filter_exclude_and_survive_recrawl() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let first = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    let books = page("https://example.com/opds", vec![importable_epub("history-en", None), importable_epub("history-sv", None), importable_epub("fiction", None)]);
    cache.stage_page(&first, &books, None, None).unwrap();
    let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
    let fiction = PublicationSubject::new(None, "Fiction / General", "dc:subject", Some("bisac".to_owned()), Some("FIC000000".to_owned())).unwrap();
    cache.stage_publication_subjects(&first, &books.publications[0].identity, std::slice::from_ref(&history)).unwrap();
    cache.stage_publication_subjects(&first, &books.publications[1].identity, std::slice::from_ref(&history)).unwrap();
    cache.stage_publication_subjects(&first, &books.publications[2].identity, std::slice::from_ref(&fiction)).unwrap();
    cache.activate_crawl(&first).unwrap();

    let request = CatalogRecommendationRequest {
        subjects: vec![CatalogSubjectPreference { system_id: "bisac".to_owned(), subject_path: "History / United States".to_owned(), weight: 5 }],
        supported_languages: vec!["en".to_owned()],
        excluded_publications: vec![CatalogRecommendationExclusion { source_id: source_id(), publication: books.publications[1].identity.clone() }],
        limit: 10,
    };
    let recommendations = cache.recommendations(&request).unwrap().recommendations;
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].publication.identity.edition_id, "history-en");
    assert_eq!(recommendations[0].matched_subjects[0].subject_path, "History / United States / 20th Century");

    let second = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_page(&second, &books, None, None).unwrap();
    cache.activate_crawl(&second).unwrap();
    assert_eq!(cache.recommendations(&request).unwrap().recommendations.len(), 1, "subject sidecars are copied into an unchanged generation");
}
