use super::*;
use catalog_model::{
    CatalogAuthorCredit, CatalogFreshness, CatalogLink, CatalogLinkTarget, CatalogPagination, CatalogPublicationAction, CatalogPublicationIdentity, CatalogResultRange, CatalogResultRangeResponse, CatalogSubjectBrowseRequest, LanguageTag,
};
use publication_model::PublicationSubject;

fn source_id() -> CatalogSourceId {
    CatalogSourceId::parse("opds-test").unwrap()
}

#[test]
fn removing_source_purges_retired_cards_legacy_indexes_and_all_owned_rows() {
    let temporary=tempfile::tempdir().unwrap();
    let cache=CatalogStore::open(temporary.path().join("catalog.db")).unwrap();
    let removed=source_id();
    let kept=CatalogSourceId::parse("opds-kept").unwrap();
    let books=page("https://example.com/opds",vec![importable_epub("retired",None),importable_epub("active",None)]);
    cache.ingest_test_page(&removed,&books.locator,&books,None,None).unwrap();
    let mut keep_page=page("https://other.example/opds",vec![importable_epub("kept",None)]);
    keep_page.source_id=kept.clone();
    cache.ingest_test_page(&kept,&keep_page.locator,&keep_page,None,None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let authority=cache.connection().unwrap();
    authority.execute("DELETE FROM catalog_publication WHERE edition_id='retired'", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let serving=cache.serving.connection().unwrap();
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM projection_retired", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    authority.execute("INSERT INTO catalog_browse_publication VALUES(999,?1,1,'legacy','','legacy',0,1,0,0,0)",[removed.as_str()]).unwrap();
    authority.execute("INSERT INTO catalog_browse_search_document VALUES(999,'legacyneedle')", []).unwrap();
    authority.execute("INSERT INTO catalog_browse_search_fts(rowid,search_text) VALUES(999,'legacyneedle')", []).unwrap();
    cache.remove_source(&removed).unwrap();
    for connection in [&*authority,&*serving] {
        let tables=connection.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").unwrap().query_map([], |r| r.get::<_,String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>();
        for table in tables {
            let quoted=table.replace('"',"\"\"");
            let owns_source=connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name='source_id')",[&table], |r| r.get::<_,bool>(0)).unwrap();
            if owns_source {
                assert_eq!(connection.query_row(&format!("SELECT COUNT(*) FROM \"{quoted}\" WHERE source_id=?1"),[removed.as_str()], |r| r.get::<_,i64>(0)).unwrap(),0,"remaining source rows in {table}");
            }
        }
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM catalog_browse_search_fts WHERE catalog_browse_search_fts MATCH 'legacyneedle'", [], |r| r.get::<_,i64>(0)).unwrap(),0);
        connection.execute("INSERT INTO catalog_browse_search_fts(catalog_browse_search_fts,rank) VALUES('integrity-check',1)", []).unwrap();
    }
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM projection_retired", [], |r| r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    cache.refresh_browse_serving_index().unwrap();
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1,"projector must not resurrect purged books");
}

#[test]
fn serving_projection_updates_only_changed_books_and_retains_immutable_payloads() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog.db")).unwrap();
    let books = page("https://example.com/opds", vec![importable_epub("one",None),importable_epub("two",None)]);
    cache.ingest_test_page(&source_id(), &books.locator, &books, None, None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let serving = cache.serving.connection().unwrap();
    let old_epoch = super::serving_projection::epoch(&serving).unwrap();
    let ids: Vec<u64> = serving.prepare("SELECT entry_id FROM catalog_publication ORDER BY edition_id").unwrap().query_map([], |r| r.get::<_,i64>(0)).unwrap().map(|r| r.unwrap() as u64).collect();
    // Observe actual projection writes; a one-book edit must not touch book two.
    serving.execute_batch("CREATE TABLE test_writes(entry_id INTEGER); CREATE TRIGGER test_write AFTER INSERT ON catalog_browse_publication BEGIN INSERT INTO test_writes VALUES(NEW.entry_id); END;").unwrap();
    let authority = cache.connection().unwrap();
    authority.execute("UPDATE catalog_publication_author SET normalized_name='changedneedle' WHERE edition_id='one'", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let writes: i64 = serving.query_row("SELECT COUNT(*) FROM test_writes", [], |r| r.get(0)).unwrap();
    assert_eq!(writes,1);
    let epoch = super::serving_projection::epoch(&serving).unwrap();
    assert_eq!(epoch,old_epoch,"ordinary edits must not invalidate all cached cards");
    assert_eq!(cache.subject_publication_payloads(&catalog_model::CatalogSubjectPublicationPayloadRequest {epoch:epoch.clone(),entry_ids:ids.clone()}).unwrap().payloads.len(),2);
    let hits: i64 = serving.query_row("SELECT COUNT(*) FROM catalog_browse_search_fts WHERE catalog_browse_search_fts MATCH 'changedneedle'", [], |r| r.get(0)).unwrap();
    assert_eq!(hits,1);
    let mut updated=books.clone();
    updated.publications[0].title="New payload version".to_owned();
    cache.ingest_test_page(&source_id(),&updated.locator,&updated,None,None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let old_cards=cache.subject_publication_payloads(&catalog_model::CatalogSubjectPublicationPayloadRequest {epoch:epoch.clone(),entry_ids:ids.clone()}).unwrap();
    assert_eq!(old_cards.payloads[0].publication.publication.title,"EPUB");
    let new_id=serving.query_row("SELECT entry_id FROM catalog_browse_publication WHERE edition_id='one'", [], |r| r.get::<_,i64>(0)).unwrap() as u64;
    assert_ne!(new_id,ids[0]);
    let new_cards=cache.subject_publication_payloads(&catalog_model::CatalogSubjectPublicationPayloadRequest {epoch:epoch.clone(),entry_ids:vec![new_id]}).unwrap();
    assert_eq!(new_cards.payloads[0].publication.publication.title,"New payload version");
    authority.execute("DELETE FROM catalog_publication WHERE edition_id='one'", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_browse_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_browse_search_fts WHERE catalog_browse_search_fts MATCH 'changedneedle'", [], |r| r.get::<_,i64>(0)).unwrap(),0);
    // A previous index still resolves its exact immutable cards after removal.
    assert_eq!(cache.subject_publication_payloads(&catalog_model::CatalogSubjectPublicationPayloadRequest {epoch:epoch.clone(),entry_ids:ids.clone()}).unwrap().payloads.len(),2);
    serving.execute("UPDATE projection_retired SET retired_at=unixepoch()-3601", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    assert!(cache.subject_publication_payloads(&catalog_model::CatalogSubjectPublicationPayloadRequest {epoch:"stale-incarnation:serving-v3-x".to_owned(),entry_ids:ids}).is_err());
}

#[test]
fn serving_projection_failure_rolls_back_and_commit_before_ack_replays() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog.db")).unwrap();
    let books = page("https://example.com/opds", vec![importable_epub("one",None)]);
    cache.ingest_test_page(&source_id(), &books.locator, &books, None, None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let serving=cache.serving.connection().unwrap();
    let authority=cache.connection().unwrap();
    let revision=serving.query_row("SELECT revision FROM projection_state", [], |r| r.get::<_,i64>(0)).unwrap();
    authority.execute("UPDATE catalog_publication_author SET normalized_name='replayneedle'", []).unwrap();
    serving.execute_batch("CREATE TRIGGER fail_projection BEFORE INSERT ON catalog_browse_search_document BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;").unwrap();
    assert!(cache.refresh_browse_serving_index_batch().is_err());
    assert_eq!(serving.query_row("SELECT revision FROM projection_state", [], |r| r.get::<_,i64>(0)).unwrap(),revision);
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(super::serving_changes::publications(&authority,256).unwrap().len(),1);
    serving.execute_batch("DROP TRIGGER fail_projection").unwrap();
    // Simulate interruption after the sidecar commit but before acknowledgement.
    authority.execute_batch("CREATE TRIGGER fail_ack BEFORE DELETE ON catalog_serving_dirty BEGIN SELECT RAISE(ABORT,'injected acknowledgement failure'); END;").unwrap();
    assert!(cache.refresh_browse_serving_index_batch().is_err());
    assert!(serving.query_row("SELECT revision FROM projection_state", [], |r| r.get::<_,i64>(0)).unwrap()>revision);
    assert_eq!(super::serving_changes::publications(&authority,256).unwrap().len(),1);
    authority.execute_batch("DROP TRIGGER fail_ack").unwrap();
    cache.refresh_browse_serving_index().unwrap();
    assert!(super::serving_changes::publications(&authority,256).unwrap().is_empty());
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_browse_search_fts WHERE catalog_browse_search_fts MATCH 'replayneedle'", [], |r| r.get::<_,i64>(0)).unwrap(),1);
    serving.execute("INSERT INTO catalog_browse_search_fts(catalog_browse_search_fts,rank) VALUES('integrity-check',1)", []).unwrap();
}

#[test]
fn serving_projection_resumes_source_cursor_despite_unrelated_revisions() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog.db");
    let cache = CatalogStore::open(&path).unwrap();
    let books = page("https://example.com/opds", (0..600).map(|n| importable_epub(&format!("book-{n:04}"),None)).collect());
    cache.ingest_test_page(&source_id(), &books.locator, &books, None, None).unwrap();
    assert!(cache.refresh_browse_serving_index_batch().unwrap());
    let serving = cache.serving.connection().unwrap();
    let cursor: String = serving.query_row("SELECT edition_cursor FROM projection_source", [], |r| r.get(0)).unwrap();
    drop(serving);
    drop(cache);
    let cache = CatalogStore::open(&path).unwrap();
    let authority = cache.connection().unwrap();
    let mut batches = 0;
    loop {
        authority.execute("UPDATE catalog_transport_state SET revision=revision+1", []).unwrap();
        batches += 1;
        assert!(batches<10,"projection restarted instead of advancing its cursor");
        if !cache.refresh_browse_serving_index_batch().unwrap() {break;}
    }
    let serving = cache.serving.connection().unwrap();
    let final_cursor: String = serving.query_row("SELECT edition_cursor FROM projection_source", [], |r| r.get(0)).unwrap();
    assert!(final_cursor>cursor);
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),600);
    // Source deletion must drain old serving rows even though the authority
    // no longer contains any publication identities for that source.
    authority.execute("DELETE FROM catalog_source", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_browse_publication", [], |r| r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn serving_projection_recovers_acknowledged_books_when_sidecar_is_recreated() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog.db");
    let cache = CatalogStore::open(&path).unwrap();
    let books = page("https://example.com/opds", vec![importable_epub("one",None)]);
    cache.ingest_test_page(&source_id(), &books.locator, &books, None, None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let old_epoch = super::serving_projection::epoch(&cache.serving.connection().unwrap()).unwrap();
    assert!(super::serving_changes::publications(&cache.connection().unwrap(),256).unwrap().is_empty());
    let sidecar = cache.serving.connection().unwrap().path().unwrap().to_owned();
    drop(cache);
    // Simulate loss of the rebuildable sidecar, not the authoritative catalog.
    std::fs::rename(&sidecar,format!("{sidecar}.previous")).unwrap();
    let cache = CatalogStore::open(&path).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let serving = cache.serving.connection().unwrap();
    assert_ne!(super::serving_projection::epoch(&serving).unwrap(),old_epoch);
    assert_eq!(serving.query_row("SELECT COUNT(*) FROM catalog_publication", [], |r| r.get::<_,i64>(0)).unwrap(),1);
}

#[test]
fn serving_projection_promotes_gutenberg_sibling_after_preferred_removal() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog.db")).unwrap();
    let source = CatalogSourceId::parse("opds-project-gutenberg").unwrap();
    let mut books = page("https://example.com/opds", vec![importable_epub("book.images",None),importable_epub("book.noimages",None)]);
    books.source_id=source.clone();
    for book in &mut books.publications { book.identity.work_id=Some("urn:gutenberg:1".to_owned()); }
    cache.ingest_test_page(&source,&books.locator,&books,None,None).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    let serving=cache.serving.connection().unwrap();
    assert_eq!(serving.query_row("SELECT edition_id FROM catalog_browse_publication", [], |r| r.get::<_,String>(0)).unwrap(),"book.images");
    cache.connection().unwrap().execute("DELETE FROM catalog_publication WHERE edition_id='book.images'", []).unwrap();
    cache.refresh_browse_serving_index().unwrap();
    assert_eq!(serving.query_row("SELECT edition_id FROM catalog_browse_publication", [], |r| r.get::<_,String>(0)).unwrap(),"book.noimages");
}

fn publication(id: &str, title: &str, author: &str, languages: &[&str]) -> CatalogPublication {
    CatalogPublication {
        identity: CatalogPublicationIdentity { edition_id: id.to_owned(), work_id: None },
        title: title.to_owned(),
        authors: vec![CatalogAuthorCredit { name: author.to_owned(), name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None }],
        languages: languages.iter().map(|language| LanguageTag::parse(language).unwrap()).collect(),
        description: None,
        image_url: None,
        metadata: Default::default(),
        actions: Vec::new(),
    }
}

fn importable_epub(id: &str, image_url: Option<&str>) -> CatalogPublication {
    let mut publication = publication(id, "EPUB", "Author", &["en"]);
    publication.image_url = image_url.map(str::to_owned);
    publication.actions.push(CatalogPublicationAction::Import {
        request: catalog_model::CatalogImportRequest {
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
    }
}

trait CatalogStoreTestExt {
    fn ingest_test_page(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogStoreError>;
    fn ingest_test_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[CatalogPageWrite]) -> Result<(), CatalogStoreError>;
    fn stage_test_page(&self, crawl: &CatalogCrawlGeneration, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogStoreError>;
    fn stage_test_pages(&self, crawl: &CatalogCrawlGeneration, pages: &[CatalogPageWrite]) -> Result<(), CatalogStoreError>;
    fn stage_test_subjects(&self, crawl: &CatalogCrawlGeneration, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject]) -> Result<(), CatalogStoreError>;
    fn activate_test_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogStoreError>;
    fn enrich_test_publication(
        &self, source_id: &CatalogSourceId, publication: &CatalogPublication, subjects: &[PublicationSubject], toc: &[publication_model::PublicationTocEntry], popularity: Option<&CatalogPublicationPopularity>,
    ) -> Result<bool, CatalogStoreError>;
    fn search_test_snapshot(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogSearchMatches, CatalogStoreError>;
    fn test_source_state(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogStoreError>;
}

impl CatalogStoreTestExt for CatalogStore {
    fn ingest_test_page(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogStoreError> {
        self.ingest_test_pages(source_id, source_url, &[CatalogPageWrite { page: page.clone(), etag: etag.map(str::to_owned), last_modified: last_modified.map(str::to_owned), provider_paginated: false }])
    }

    fn ingest_test_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[CatalogPageWrite]) -> Result<(), CatalogStoreError> {
        self.put_prepared_pages(source_id, source_url, &CatalogStore::prepare_page_writes(pages.to_vec())?)
    }

    fn stage_test_page(&self, crawl: &CatalogCrawlGeneration, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogStoreError> {
        self.stage_test_pages(crawl, &[CatalogPageWrite { page: page.clone(), etag: etag.map(str::to_owned), last_modified: last_modified.map(str::to_owned), provider_paginated: false }])
    }

    fn stage_test_pages(&self, crawl: &CatalogCrawlGeneration, pages: &[CatalogPageWrite]) -> Result<(), CatalogStoreError> {
        self.stage_prepared_pages(crawl, &CatalogStore::prepare_page_writes(pages.to_vec())?)
    }

    fn stage_test_subjects(&self, crawl: &CatalogCrawlGeneration, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject]) -> Result<(), CatalogStoreError> {
        let assignments = prepare_catalog_subject_assignments(subjects);
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        replace_publication_subjects_prepared(&transaction, &crawl.source_id, crawl.generation, identity, subjects, &assignments)?;
        transaction.commit()?;
        Ok(())
    }

    fn activate_test_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogStoreError> {
        while self.prepare_crawl_activation_batch(crawl, 256)? {}
        let state = self.finalize_crawl_activation(crawl)?;
        while self.reconcile_active_epub_extractions_batch(&crawl.source_id, 256)? {}
        Ok(state)
    }

    fn enrich_test_publication(
        &self, source_id: &CatalogSourceId, publication: &CatalogPublication, subjects: &[PublicationSubject], toc: &[publication_model::PublicationTocEntry], popularity: Option<&CatalogPublicationPopularity>,
    ) -> Result<bool, CatalogStoreError> {
        let mut publication = publication.clone();
        merge_catalog_subject_evidence(&mut publication, subjects);
        let subjects = publication.metadata.subjects.clone();
        let assignments = prepare_catalog_subject_assignments(&subjects);
        self.enrich_active_publication_prepared(source_id, &publication, &subjects, &assignments, toc, popularity, None)
    }

    fn search_test_snapshot(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogSearchMatches, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(CatalogSearchMatches::default()) };
        let query = normalize_search_text(query);
        if query.is_empty() {
            return Ok(CatalogSearchMatches::default());
        }
        super::page_search::search_snapshot(&connection, source_id, generation, &query)
    }

    fn test_source_state(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogStoreError> {
        let connection = self.connection()?;
        source_state_in(&connection, source_id)
    }
}

#[test]
fn cache_connections_use_low_sync_wal_settings() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let connection = cache.connection().unwrap();

    assert_eq!(connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0)).unwrap(), "wal");
    assert_eq!(connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn server_store_installs_its_complete_schema() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");

    let cache = CatalogStore::open(&path).unwrap();
    let connection = cache.connection().unwrap();
    let schema_version = connection.query_row("SELECT schema_version FROM catalog_server_schema_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(schema_version, 6);
    let queue_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_cover_download')", [], |row| row.get::<_, bool>(0)).unwrap();
    assert!(queue_exists);
}

#[test]
fn server_store_preserves_an_unknown_schema_version_instead_of_deleting_the_cache() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogStore::open(&path).unwrap();
    cache.connection().unwrap().execute("UPDATE catalog_server_schema_metadata SET schema_version=99 WHERE singleton=1", []).unwrap();
    drop(cache);

    assert!(CatalogStore::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    let schema_version = connection.query_row("SELECT schema_version FROM catalog_server_schema_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(schema_version, 99);
}

#[test]
fn local_search_separates_title_and_author_matches() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache
        .ingest_test_page(
            &source_id(),
            "https://example.com/opds",
            &page("https://example.com/opds", vec![publication("title", "Cien años de soledad", "Someone", &["es"]), publication("author", "Chronicle", "Gabriel García Márquez", &["es"])]),
            None,
            None,
        )
        .unwrap();
    let matches = cache.search_test_snapshot(&source_id(), "garcia marquez").unwrap();
    assert!(matches.title_matches.is_empty());
    assert_eq!(matches.author_matches[0].publication.identity.edition_id, "author");
    let matches = cache.search_test_snapshot(&source_id(), "cien anos").unwrap();
    assert_eq!(matches.title_matches[0].publication.identity.edition_id, "title");
    assert!(matches.author_matches.is_empty());
}

#[test]
fn local_search_returns_distinct_table_of_contents_matches() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let book = publication("toc", "Collected Essays", "Ada Author", &["en"]);
    cache.ingest_test_page(&source_id(), "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
    let toc = vec![publication_model::PublicationTocEntry {
        title: "Industrial transformation".to_owned(),
        target: "chapter.xhtml".to_owned(),
        children: vec![publication_model::PublicationTocEntry { title: "Swedish railways".to_owned(), target: "chapter.xhtml#rail".to_owned(), children: Vec::new() }],
    }];
    assert!(cache.enrich_test_publication(&source_id(), &book, &[], &toc, None).unwrap());

    let matches = cache.search_test_snapshot(&source_id(), "railways").unwrap();
    assert!(matches.title_matches.is_empty());
    assert!(matches.author_matches.is_empty());
    assert_eq!(matches.toc_matches[0].publication.identity.edition_id, "toc");
}

#[test]
fn failed_staging_keeps_active_generation_and_activation_reuses_unchanged_sequences() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let first = page("https://example.com/opds", vec![publication("one", "One", "Author", &["en"]), publication("two", "Two", "Author", &["en"])]);
    cache.ingest_test_page(&source_id(), "https://example.com/opds", &first, Some("v1"), None).unwrap();
    let original = cache.search_test_snapshot(&source_id(), "one").unwrap().title_matches[0].sequence;

    let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_test_page(&failed, &page("https://example.com/opds", vec![publication("replacement", "Replacement", "Author", &["en"])]), None, None).unwrap();
    cache.abandon_crawl_with_error(&failed, None).unwrap();
    assert_eq!(cache.search_test_snapshot(&source_id(), "one").unwrap().title_matches[0].sequence, original);

    let successful = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_test_page(&successful, &first, Some("v1"), None).unwrap();
    cache.activate_test_crawl(&successful).unwrap();
    assert_eq!(cache.search_test_snapshot(&source_id(), "one").unwrap().title_matches[0].sequence, original);
}

#[test]
fn source_url_changes_invalidate_only_that_source() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ingest_test_page(&source_id(), "https://example.com/one", &page("https://example.com/one", vec![publication("one", "One", "Author", &["en"])]), None, None).unwrap();
    let old_epoch = cache.test_source_state(&source_id()).unwrap().unwrap().epoch;
    cache.ensure_source(&source_id(), "https://example.com/two").unwrap();
    let state = cache.test_source_state(&source_id()).unwrap().unwrap();
    assert_ne!(state.epoch, old_epoch);
    assert!(cache.search_test_snapshot(&source_id(), "one").unwrap().title_matches.is_empty());
}

#[test]
fn page_response_sorts_books_by_open_library_popularity_and_preserves_ties() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let locator = "https://example.com/opds";
    let first = publication("first", "First", "Author", &["en"]);
    let popular = publication("popular", "Popular", "Author", &["en"]);
    let third = publication("third", "Third", "Author", &["en"]);
    cache.ingest_test_page(&source_id(), locator, &page(locator, vec![first, popular.clone(), third]), None, None).unwrap();
    cache.enrich_test_publication(&source_id(), &popular, &[], &[], Some(&CatalogPublicationPopularity { want_to_read_count: 20, currently_reading_count: 3, already_read_count: 7, rating_count: 4, rating_sum: 18 })).unwrap();

    let response = cache.page_response(&source_id(), locator, None, &[]).unwrap().unwrap();
    assert_eq!(response.page.publications.iter().map(|publication| publication.identity.edition_id.as_str()).collect::<Vec<_>>(), vec!["popular", "first", "third"]);
}

#[test]
fn preserves_an_outdated_cache_instead_of_deleting_it_during_redeploy() {
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

    let cache = CatalogStore::open(&path).unwrap();
    let inventory = cache.inventory().unwrap();
    assert_eq!(inventory.active_page_count, 0);
    assert_eq!(inventory.active_publication_count, 0);
    assert_eq!(inventory.active_epub_count, 0);
    drop(cache);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.query_row("SELECT schema_version FROM catalog_server_schema_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).unwrap(), 6);
    assert!(!connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_cache_metadata')", [], |row| row.get::<_, bool>(0)).unwrap());
    assert_eq!(connection.query_row("SELECT value FROM migration_sentinel", [], |row| row.get::<_, String>(0)).unwrap(), "preserved");
}

#[test]
fn current_cache_adds_failure_diagnostics_without_discarding_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    {
        let cache = CatalogStore::open(&path).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    }
    let connection = Connection::open(&path).unwrap();
    connection.execute("DROP TABLE catalog_epub_failure", ()).unwrap();
    drop(connection);

    let cache = CatalogStore::open(&path).unwrap();
    assert_eq!(cache.inventory().unwrap().sources.len(), 1);
    let connection = Connection::open(&path).unwrap();
    assert!(connection.query_row("SELECT COUNT(*) FROM catalog_epub_failure", (), |row| row.get::<_, i64>(0)).is_ok());
}

#[test]
fn crawl_status_and_counts_change_atomically_without_losing_active_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "running");
    cache.stage_test_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();
    cache.activate_test_crawl(&crawl).unwrap();

    let succeeded = cache.inventory().unwrap();
    assert_eq!((succeeded.active_page_count, succeeded.active_publication_count, succeeded.active_epub_count), (1, 1, 1));
    assert_eq!(succeeded.sources[0].crawl_status, "succeeded");
    assert!(succeeded.sources[0].last_crawl_started_at.is_some());
    assert!(succeeded.sources[0].last_crawl_succeeded_at.is_some());

    let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    cache.stage_test_page(&failed, &page("https://example.com/opds", Vec::new()), None, None).unwrap();
    cache.abandon_crawl_with_error(&failed, Some("upstream timeout")).unwrap();
    let inventory = cache.inventory().unwrap();
    assert_eq!((inventory.active_page_count, inventory.active_publication_count, inventory.active_epub_count), (1, 1, 1));
    assert_eq!(inventory.sources[0].crawl_status, "failed");
    assert_eq!(inventory.sources[0].last_crawl_error.as_deref(), Some("upstream timeout"));
}

#[test]
fn crawl_activation_and_extraction_reconciliation_are_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    cache.ensure_source(&source, "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    cache.stage_test_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None), importable_epub("two", None), importable_epub("three", None)]), None, None).unwrap();

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
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    cache.begin_incremental_crawl(&source, "https://example.com/opds").unwrap();
    cache.ingest_test_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();

    let running = cache.inventory().unwrap();
    // Incremental page writes intentionally avoid an exact recount on every
    // batch; completion performs the bounded operation once.
    assert_eq!((running.active_page_count, running.active_publication_count), (0, 0));
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
        let cache = CatalogStore::open(&path).unwrap();
        cache.ensure_source(&source, "https://example.com/opds").unwrap();
        cache.queue_crawls(std::slice::from_ref(&source)).unwrap();
        assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "queued");
        cache.queue_epub_extractions(&source, &[succeeded.clone(), failed.clone(), interrupted.clone()], false).unwrap();
        let succeeded_attempt = cache.begin_epub_extraction(&source, &succeeded).unwrap().unwrap();
        cache.finish_epub_extraction(&source, &succeeded, succeeded_attempt, 12_345, &publication("one", "One", "Author", &["en"]), None).unwrap();
        let failed_attempt = cache.begin_epub_extraction(&source, &failed).unwrap().unwrap();
        cache.fail_epub_extraction(&source, &failed, failed_attempt, "invalid EPUB", "invalid_publication", false).unwrap();
        cache.begin_epub_extraction(&source, &interrupted).unwrap();
        let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 1, 1, 1));
    }

    let reopened = CatalogStore::open(&path).unwrap();
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
fn stale_epub_extraction_attempt_cannot_publish_after_retry() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let url = "https://example.com/retried.epub";
    cache.ensure_source(&source, "https://example.com/opds").unwrap();
    cache.queue_epub_extractions(&source, &[url.to_owned()], false).unwrap();

    let first = cache.begin_epub_extraction(&source, url).unwrap().unwrap();
    cache.fail_epub_extraction(&source, url, first, "temporary failure", "network_timeout", true).unwrap();
    let second = cache.begin_epub_extraction(&source, url).unwrap().unwrap();
    assert!(second > first);

    assert!(!cache.set_epub_extraction_phase(&source, url, first, "enriching").unwrap());
    assert!(!cache.finish_epub_extraction(&source, url, first, 1, &publication("stale", "Stale", "Author", &["en"]), None).unwrap());
    assert_eq!(cache.epub_extraction_status(&source, url).unwrap().as_deref(), Some("running"));
    assert!(cache.finish_epub_extraction(&source, url, second, 2, &publication("fresh", "Fresh", "Author", &["en"]), None).unwrap());
    assert_eq!(cache.epub_extraction_status(&source, url).unwrap().as_deref(), Some("succeeded"));
}

#[test]
fn description_backfill_is_separate_and_only_fills_missing_descriptions() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogStore::open(&path).unwrap();
    let source = source_id();
    let missing = importable_epub("missing-description", None);
    let without_open_library_description = importable_epub("without-open-library-description", None);
    let mut existing = importable_epub("existing-description", None);
    existing.description = Some(CatalogDescription::PlainText { text: "Feed description".to_owned() });
    cache.ingest_test_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![missing.clone(), without_open_library_description.clone(), existing.clone()]), None, None).unwrap();

    for book in [&missing, &without_open_library_description, &existing] {
        let acquisition_url = publication_extraction(book).unwrap().acquisition_url.to_owned();
        let attempt = cache.begin_epub_extraction(&source, &acquisition_url).unwrap().unwrap();
        cache.finish_epub_extraction(&source, &acquisition_url, attempt, 123, book, None).unwrap();
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
                .then(|| vec![publication_model::PublicationSubject::new(None, "HIS015000", "openlibrary:2026-08-01:work", Some("bisac".to_owned()), Some("HIS015000".to_owned())).unwrap()])
                .unwrap_or_default(),
            open_library_description_missing: job.publication == without_open_library_description.identity,
            attempt: job.attempt,
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
fn durable_extraction_queue_claims_bounded_jobs_and_discards_stale_acquisitions() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let first = importable_epub("one", Some("urn:cover:one"));
    let second = importable_epub("two", None);
    cache.ingest_test_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![first.clone(), second.clone()]), None, None).unwrap();

    let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    assert_eq!(claimed.publication, first.identity);
    assert_eq!(claimed.acquisition_url, "https://example.com/one.epub");
    assert_eq!(claimed.book_format, CatalogBookFormat::Epub);
    assert_eq!(claimed.cover_key.as_deref(), Some("urn:cover:one"));
    let inventory = cache.inventory().unwrap().epub_extractions;
    assert_eq!((inventory.pending, inventory.running), (1, 1));

    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    cache.stage_test_page(&crawl, &page("https://example.com/opds", vec![second.clone()]), None, None).unwrap();
    cache.activate_test_crawl(&crawl).unwrap();

    assert_eq!(cache.epub_extraction_status(&source, "https://example.com/one.epub").unwrap(), None);
    let remaining = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    assert_eq!(remaining.publication, second.identity);
    assert!(cache.claim_next_epub_extraction(&source).unwrap().is_none());
}

#[test]
fn ocr_backfill_claims_after_metadata_and_releases_durably() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogStore::open(&path).unwrap();
    let source = source_id();
    let book = importable_epub("ocr", None);
    cache.ingest_test_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
    let acquisition_url = publication_extraction(&book).unwrap().acquisition_url.to_owned();
    let attempt = cache.begin_epub_extraction(&source, &acquisition_url).unwrap().unwrap();
    cache
        .finish_epub_extraction(
            &source,
            &acquisition_url,
            attempt,
            12,
            &book,
            Some(&CatalogEpubMetadata { book: publication_model::BookMetadataState { title: String::new(), contributors: Vec::new(), description: String::new(), publication: Default::default() }, toc: Vec::new() }),
        )
        .unwrap();

    let connection = Connection::open(&path).unwrap();
    connection
        .execute("INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status,ocr_status) VALUES(?1,?2,?3,'succeeded','pending')", params![source.as_str(), acquisition_url, CATALOG_METADATA_BACKFILL_VERSION])
        .unwrap();
    let jobs = cache.claim_ocr_backfill_batch(1).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].publication, book.identity);
    assert_eq!(jobs[0].book_format, CatalogBookFormat::Epub);
    assert_eq!(connection.query_row("SELECT ocr_status FROM catalog_metadata_backfill", [], |row| row.get::<_, String>(0)).unwrap(), "running");

    cache.release_ocr_backfill(&jobs, "test retry").unwrap();
    assert_eq!(connection.query_row("SELECT ocr_status FROM catalog_metadata_backfill", [], |row| row.get::<_, String>(0)).unwrap(), "pending");
}

#[test]
fn page_batches_commit_incremental_and_replacement_pages_atomically() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let root = "https://example.com/opds";
    let child = "https://example.com/child";
    cache
        .ingest_test_pages(
            &source,
            root,
            &[
                CatalogPageWrite { page: page(root, vec![publication("one", "One", "Author", &["en"])]), etag: Some("root-v1".to_owned()), last_modified: None, provider_paginated: false },
                CatalogPageWrite { page: page(child, vec![publication("two", "Two", "Author", &["en"])]), etag: Some("child-v1".to_owned()), last_modified: None, provider_paginated: false },
            ],
        )
        .unwrap();
    assert!(cache.page(&source, root).unwrap().is_some());
    assert!(cache.page(&source, child).unwrap().is_some());
    assert_eq!(cache.page(&source, child).unwrap().unwrap().etag.as_deref(), Some("child-v1"));
    assert_eq!(cache.inventory().unwrap().active_page_count, 0, "incremental batches must not recount the active generation");
    cache.finish_incremental_crawl(&source, true).unwrap();
    assert_eq!(cache.inventory().unwrap().active_page_count, 2, "crawl completion performs the single exact recount");

    let crawl = cache.begin_crawl(&source, root).unwrap();
    cache
        .stage_test_pages(
            &crawl,
            &[
                CatalogPageWrite { page: page(root, vec![publication("three", "Three", "Author", &["en"])]), etag: Some("root-v2".to_owned()), last_modified: None, provider_paginated: false },
                CatalogPageWrite { page: page(child, Vec::new()), etag: Some("child-v2".to_owned()), last_modified: None, provider_paginated: false },
            ],
        )
        .unwrap();
    assert_eq!(cache.page(&source, root).unwrap().unwrap().page.publications[0].identity.edition_id, "one", "staging must not leak before activation");
    cache.activate_test_crawl(&crawl).unwrap();
    assert_eq!(cache.page(&source, root).unwrap().unwrap().page.publications[0].identity.edition_id, "three");
    assert_eq!(cache.page(&source, child).unwrap().unwrap().etag.as_deref(), Some("child-v2"));
}

#[test]
fn unchanged_publications_preserve_sequence_and_fts_metadata_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    let cache = CatalogStore::open(&path).unwrap();
    let source = source_id();
    let root = "https://example.com/opds";
    let books = page(root, vec![publication("same", "Same", "Stable Author", &["en"])]);
    cache.ingest_test_page(&source, root, &books, None, None).unwrap();
    let before = cache.page_response(&source, root, None, &[]).unwrap().unwrap().payloads[0].sequence;
    let rowid_before = Connection::open(&path).unwrap().query_row("SELECT rowid FROM catalog_publication_author WHERE source_id=?1 AND normalized_name='stable author'", [source.as_str()], |row| row.get::<_, i64>(0)).unwrap();

    cache.ingest_test_page(&source, root, &books, None, None).unwrap();

    let after = cache.page_response(&source, root, None, &[]).unwrap().unwrap().payloads[0].sequence;
    let rowid_after = Connection::open(&path).unwrap().query_row("SELECT rowid FROM catalog_publication_author WHERE source_id=?1 AND normalized_name='stable author'", [source.as_str()], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(after, before);
    assert_eq!(rowid_after, rowid_before, "unchanged author metadata must not be deleted and reinserted");
}

#[test]
fn page_graph_frontier_and_enrichment_are_resolved_without_loading_all_pages() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let root_url = "https://example.com/opds";
    let child_url = "https://example.com/child";
    let mut root = page(root_url, vec![importable_epub("one", None)]);
    root.navigation.push(CatalogLink { title: "Child".to_owned(), target: CatalogLinkTarget::Page { locator: child_url.to_owned() }, relations: Vec::new(), number_of_items: None, active: false });
    cache.ingest_test_page(&source, root_url, &root, None, None).unwrap();
    assert!(cache.has_missing_page_frontier(&source, root_url).unwrap());

    let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
    let mut enriched = importable_epub("one", None);
    enriched.title = "Embedded title".to_owned();
    cache.finish_epub_extraction(&source, &claimed.acquisition_url, claimed.attempt, 10, &enriched, None).unwrap();
    let mut refreshed_root = root.clone();
    cache.enrich_page_from_epub_extractions(&source, &mut refreshed_root).unwrap();
    assert_eq!(refreshed_root.publications[0].title, "Embedded title");

    cache.ingest_test_page(&source, root_url, &page(child_url, Vec::new()), None, None).unwrap();
    assert!(!cache.has_missing_page_frontier(&source, root_url).unwrap());
}

#[test]
fn epub_extraction_inventory_discards_urls_no_longer_in_the_catalogue() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let retained = importable_epub("retained", None);
    cache.ingest_test_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![retained]), None, None).unwrap();
    cache.queue_epub_extractions(&source, &["https://example.com/removed.epub".to_owned()], false).unwrap();
    while cache.reconcile_active_epub_extractions_batch(&source, 256).unwrap() {}

    let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
    assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (1, 0, 0, 0));
}

#[test]
fn subject_browse_reuses_projected_hierarchy_and_paginates_sourced_books() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    let mut books = page("https://example.com/opds", vec![importable_epub("history-one", None), importable_epub("history-two", None)]);
    books.publications[0].title = "First History".to_owned();
    books.publications[1].title = "Second History".to_owned();
    cache.stage_test_page(&crawl, &books, None, None).unwrap();
    let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
    let dewey = PublicationSubject::new(None, "515.33", "openlibrary", Some("ddc".to_owned()), Some("515.33".to_owned())).unwrap();
    cache.stage_test_subjects(&crawl, &books.publications[0].identity, &[history.clone(), dewey]).unwrap();
    cache.stage_test_subjects(&crawl, &books.publications[1].identity, &[history]).unwrap();
    cache.activate_test_crawl(&crawl).unwrap();
    cache
        .enrich_test_publication(
            &source_id(),
            &books.publications[1],
            &[PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap()],
            &[],
            Some(&CatalogPublicationPopularity { want_to_read_count: 40, currently_reading_count: 2, already_read_count: 8, rating_count: 3, rating_sum: 13 }),
        )
        .unwrap();
    cache.refresh_browse_serving_index().unwrap();

    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap();
    assert_eq!(root.navigation.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), ["Humanities"]);
    assert!(root.publications.is_empty());
    let humanities = root.navigation.iter().find(|link| link.title == "Humanities").unwrap();
    let CatalogLinkTarget::Page { locator } = &humanities.target else { panic!("subject roots must be internal links") };
    let humanities_locator = locator.clone();
    let mut locator = locator.clone();
    for expected_child in ["History", "History by Region", "North American History", "U.S. History", "Historical Eras", "20th Century"] {
        let level = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator), supported_languages: vec!["en".to_owned()], range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap();
        let child = level.navigation.iter().find(|link| link.title == expected_child).unwrap_or_else(|| panic!("missing unified subject child {expected_child}"));
        let CatalogLinkTarget::Page { locator: child_locator } = &child.target else { panic!("subject children must be internal links") };
        locator = child_locator.clone();
    }

    let first = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator.clone()), supported_languages: vec!["en".to_owned()], range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap();
    assert!(first.navigation.is_empty());
    assert_eq!(first.publications.len(), 1);
    assert_eq!(first.publications[0].source_id, source_id());
    assert_eq!(first.publications[0].publication.identity.edition_id, "history-two", "popular books sort first within an equally relevant subject");
    assert_eq!(first.range, CatalogResultRangeResponse { offset: 0, returned_items: 1, has_more: true });
    let second = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator), supported_languages: vec!["en".to_owned()], range: CatalogResultRange::new(1, 1), ..Default::default() }).unwrap();
    assert_eq!(second.publications.len(), 1);
    assert_ne!(first.publications[0].publication.identity, second.publications[0].publication.identity);

    let excluded_source = CatalogSourceId::parse("another-source").unwrap();
    let filtered_root = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![excluded_source], range: CatalogResultRange::new(0, 10), ..Default::default() });
    assert!(filtered_root.unwrap().navigation.is_empty());

    cache.refresh_search_index().unwrap();
    let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest {
        locator: Some(humanities_locator),
        supported_languages: vec!["en".to_owned()],
        source_ids: vec![source_id()],
        query: "FIRST HISTORY".to_owned(),
        range: CatalogResultRange::new(0, 10),
        ..Default::default()
    });
    let searched = searched.unwrap();
    assert!(searched.navigation.is_empty());
    assert_eq!(searched.publications.len(), 1);
    assert_eq!(searched.publications[0].publication.identity.edition_id, "history-one");
}

#[test]
fn subject_browse_uses_gutenberg_downloads_when_open_library_popularity_is_missing() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
    let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
    let mut alphabetical_first = importable_epub("urn:gutenberg:1:1", None);
    alphabetical_first.title = "A lesser-known book".to_owned();
    alphabetical_first.description = Some(CatalogDescription::PlainText { text: "Downloads: 12 Language: English".to_owned() });
    let mut popular = importable_epub("urn:gutenberg:2:1", None);
    popular.title = "The popular book".to_owned();
    popular.description = Some(CatalogDescription::PlainText { text: "Downloads: 45000 Language: English".to_owned() });
    let books = page("https://example.com/opds", vec![alphabetical_first, popular]);
    cache.stage_test_page(&crawl, &books, None, None).unwrap();
    let fiction = PublicationSubject::new(None, "Fiction / General", "dc:subject", Some("bisac".to_owned()), Some("FIC000000".to_owned())).unwrap();
    for publication in &books.publications {
        cache.stage_test_subjects(&crawl, &publication.identity, std::slice::from_ref(&fiction)).unwrap();
    }
    cache.activate_test_crawl(&crawl).unwrap();

    cache.refresh_search_index().unwrap();
    let response = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], query: "book".to_owned(), range: CatalogResultRange::new(0, 10), ..Default::default() }).unwrap();
    assert_eq!(response.publications[0].publication.title, "The popular book");
}

#[test]
fn subject_browse_filters_and_paginates_inside_sqlite() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = CatalogStore::open(temporary.path().join("catalog-cache.db")).unwrap();
    let source = source_id();
    let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
    let mut books = page("https://example.com/opds", vec![importable_epub("sqlite-one", None), importable_epub("sqlite-two", None)]);
    books.publications[0].title = "First indexed history".to_owned();
    books.publications[1].title = "Second indexed history".to_owned();
    cache.stage_test_page(&crawl, &books, None, None).unwrap();
    cache.activate_test_crawl(&crawl).unwrap();
    let generation = cache.test_source_state(&source).unwrap().unwrap().active_generation;
    let mut connection = cache.connection().unwrap();
    let transaction = connection.transaction().unwrap();
    let assignment = CatalogSubjectAssignment { system_id: "unified".to_owned(), subject_path: "Humanities / History".to_owned(), matcher_version: 1, evidence_positions: Vec::new(), lcc_evidence: Vec::new() };
    for publication in &books.publications {
        insert_prepared_subject_assignments(&transaction, source.as_str(), storage_i64(generation, "generation").unwrap(), &publication.identity.edition_id, "", std::slice::from_ref(&assignment)).unwrap();
    }
    transaction.commit().unwrap();
    drop(connection);

    let warming = cache.browse_subjects(&CatalogSubjectBrowseRequest { query: "indexed history".to_owned(), range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap_err();
    assert!(warming.to_string().contains("warming"));
    assert!(cache.refresh_search_index().unwrap());

    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source.clone()], range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap();
    assert_eq!(root.navigation.len(), 1);
    assert_eq!(root.navigation[0].title, "Humanities");
    assert_eq!(root.navigation[0].number_of_items, None);
    assert!(root.publications.is_empty());

    let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source], query: "FIRST".to_owned(), range: CatalogResultRange::new(0, 1), ..Default::default() }).unwrap();
    assert_eq!(searched.publications.len(), 1);
    assert_eq!(searched.publications[0].publication.identity.edition_id, "sqlite-one");
    assert_eq!(searched.range, CatalogResultRangeResponse { offset: 0, returned_items: 1, has_more: false });

    let first_search_page = cache
        .browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "indexed history".to_owned(), range: CatalogResultRange::new(0, 1), ..Default::default() })
        .unwrap();
    assert_eq!(first_search_page.publications.len(), 1);
    assert_eq!(first_search_page.range, CatalogResultRangeResponse { offset: 0, returned_items: 1, has_more: true });
    let second_search_page = cache
        .browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "indexed history".to_owned(), range: CatalogResultRange::new(1, 1), ..Default::default() })
        .unwrap();
    assert_eq!(second_search_page.publications.len(), 1);
    assert_eq!(second_search_page.range, CatalogResultRangeResponse { offset: 1, returned_items: 1, has_more: false });
    assert_ne!(first_search_page.publications[0].publication.identity, second_search_page.publications[0].publication.identity);

    let connection = cache.connection().unwrap();
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
        let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: query.to_owned(), range: CatalogResultRange::new(0, 10), ..Default::default() });
        let searched = searched.unwrap();
        assert_eq!(searched.publications.len(), 1, "FTS trigger did not index {query}");
        assert_eq!(searched.publications[0].publication.identity.edition_id, "sqlite-two");
    }

    assert!(cache.refresh_browse_serving_index().unwrap());
    assert!(!cache.refresh_browse_serving_index().unwrap());
    // A completed serving snapshot remains readable while a newer catalogue
    // revision waits to be projected; readers never fall back to the slow
    // ingestion schema during a rebuild.
    cache.connection().unwrap().execute("UPDATE catalog_transport_state SET revision=revision+1 WHERE singleton=1", []).unwrap();
    for query in ["indexed history", "authorneedle", "tocneedle"] {
        let expected_matches = if query == "indexed history" { 2 } else { 1 };
        let request = CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: query.to_owned(), range: CatalogResultRange::new(0, 10), ..Default::default() };
        let searched = cache.browse_subjects(&request).unwrap();
        assert!(!searched.publications.is_empty(), "serving FTS did not index {query}");
        assert_eq!(searched.capabilities.book_formats.iter().find(|facet| facet.value == CatalogBookFormat::Epub).map(|facet| facet.number_of_items), Some(expected_matches));
        assert_eq!(searched.capabilities.languages.iter().find(|facet| facet.value == "en").map(|facet| facet.number_of_items), Some(expected_matches));
    }
    let past_end = cache
        .browse_subjects(&CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], source_ids: vec![source_id()], query: "indexed history".to_owned(), range: CatalogResultRange::new(99, 1), ..Default::default() })
        .unwrap();
    assert!(past_end.publications.is_empty());
    assert!(!past_end.range.has_more);
}

#[test]
fn existing_cache_backfill_rebuilds_the_unified_subject_projection() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("catalog-cache.db");
    {
        let cache = CatalogStore::open(&path).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        let books = page("https://example.com/opds", vec![importable_epub("history", None)]);
        cache.stage_test_page(&crawl, &books, None, None).unwrap();
        let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
        cache.stage_test_subjects(&crawl, &books.publications[0].identity, &[history]).unwrap();
        cache.activate_test_crawl(&crawl).unwrap();

        let connection = cache.connection().unwrap();
        connection.execute("DELETE FROM catalog_publication_subject_assignment WHERE system_id=?1", [subject_projection::UNIFIED_SYSTEM_ID]).unwrap();
        connection.execute("DELETE FROM catalog_subject_projection_metadata", []).unwrap();
    }

    let cache = CatalogStore::open(&path).unwrap();
    cache.refresh_unified_subject_assignments().unwrap();
    let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { range: CatalogResultRange::new(0, 10), ..Default::default() }).unwrap();
    assert!(root.navigation.iter().any(|link| link.title == "Humanities"));
    let connection = cache.connection().unwrap();
    let unified_count = connection.query_row("SELECT COUNT(*) FROM catalog_publication_subject_assignment WHERE system_id=?1", [subject_projection::UNIFIED_SYSTEM_ID], |row| row.get::<_, i64>(0)).unwrap();
    assert!(unified_count > 0);
}

#[test]
fn lcc_recovery_evidence_survives_storage_backfill_and_enrichment_positions() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("lcc-evidence.db");
    let source = source_id();
    let originals = ["QA76; UNKNOWN", "QA76.", "QA76.76.063", "QA76.73.R87"];
    {
        let cache = CatalogStore::open(&path).unwrap();
        let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
        let books = page("https://example.com/opds", vec![importable_epub("lcc-evidence", None)]);
        cache.stage_test_page(&crawl, &books, None, None).unwrap();
        let subjects = originals.iter().map(|raw| PublicationSubject::new(None, *raw, "marc", Some("lcc".to_owned()), Some((*raw).to_owned())).unwrap()).collect::<Vec<_>>();
        cache.stage_test_subjects(&crawl, &books.publications[0].identity, &subjects).unwrap();
        cache.activate_test_crawl(&crawl).unwrap();
        let connection = cache.connection().unwrap();
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM catalog_subject_lcc_evidence", [], |row| row.get(0)).unwrap();
        assert!(count >= 4);
        // Existing databases acquire the new table and reconstruct evidence
        // from their preserved sources on the next matcher-version backfill.
        connection.execute("DROP TABLE catalog_subject_lcc_evidence", []).unwrap();
        connection.execute("DELETE FROM catalog_subject_projection_metadata", []).unwrap();
        connection
            .execute(
                "INSERT INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code)
             SELECT source_id,generation,edition_id,work_id,1000007,'QA76.76 .063 2000','epub','lcc','QA76.76 .063 2000'
             FROM catalog_publication_subject WHERE position=0",
                [],
            )
            .unwrap();
    }
    let cache = CatalogStore::open(&path).unwrap();
    cache.refresh_unified_subject_assignments().unwrap();
    let connection = cache.connection().unwrap();
    let rows = connection
        .prepare("SELECT source_position,original,details_json FROM catalog_subject_lcc_evidence ORDER BY source_position")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (position, original) in originals.iter().enumerate() {
        assert!(rows.iter().any(|r| r.0 == position as i64 && r.1 == *original));
    }
    let methods = rows
        .iter()
        .flat_map(|r| {
            let value: serde_json::Value = serde_json::from_str(&r.2).unwrap();
            assert_eq!(value["matcher_version"], subject_projection::UNIFIED_MATCHER_VERSION);
            value["evidence"].as_array().unwrap().iter().map(|e| e["method"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    for method in ["exact", "normalized", "recovered_component", "broad_fallback"] {
        assert!(methods.contains(method), "{methods:?}");
    }
    let recovered: serde_json::Value = serde_json::from_str(&rows.iter().find(|r| r.0 == 0).unwrap().2).unwrap();
    assert!(recovered["unresolved_fragments"].as_array().unwrap().iter().any(|v| v == "UNKNOWN"));
    let enriched = rows.iter().find(|r| r.0 == 1000007).unwrap();
    assert_eq!(enriched.1, "QA76.76 .063 2000");
    let details: serde_json::Value = serde_json::from_str(&enriched.2).unwrap();
    assert!(details["evidence"].as_array().unwrap().iter().any(|e| e["interpretations"].as_array().unwrap().iter().any(|v| v == "QA76.76 .O63 2000")));
    connection.execute("DELETE FROM catalog_publication_subject WHERE position=1000007", []).unwrap();
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM catalog_subject_lcc_evidence WHERE source_position=1000007", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    connection.execute("DELETE FROM catalog_publication_subject_assignment WHERE system_id='unified'", []).unwrap();
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM catalog_subject_lcc_evidence", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}
