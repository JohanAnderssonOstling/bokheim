use super::*;
use crate::http::load_operational_stats;
use crate::identity::edition_candidate_query;
use crate::import::core::{completed_phase, configure_import, create_schema, import_editions, register_import_inputs};
use crate::import::wikidata::import_wikidata;
use crate::query::install_query_deadline;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

fn dump(path: &Path, records: &[serde_json::Value]) {
    let file = File::create(path).unwrap();
    let mut gzip = GzEncoder::new(file, Compression::fast());
    for record in records {
        writeln!(gzip, "/type\t/key\t1\t2026-08-01T00:00:00.000000\t{record}").unwrap();
    }
    gzip.finish().unwrap();
}

fn tsv_dump(path: &Path, lines: &[&str]) {
    let file = File::create(path).unwrap();
    let mut gzip = GzEncoder::new(file, Compression::fast());
    for line in lines {
        writeln!(gzip, "{line}").unwrap();
    }
    gzip.finish().unwrap();
}

fn entity_claim(id: &str) -> serde_json::Value {
    serde_json::json!({"mainsnak":{"datavalue":{"value":{"id":id}}}})
}

fn snapshot_from(directory: &Path, edition_records: &[serde_json::Value], work_records: &[serde_json::Value], author_records: &[serde_json::Value]) -> PathBuf {
    let editions = directory.join("ol_dump_editions_2026-08-01.txt.gz");
    let works = directory.join("ol_dump_works_2026-08-01.txt.gz");
    let authors = directory.join("ol_dump_authors_2026-08-01.txt.gz");
    let database = directory.join("snapshot.sqlite");
    dump(&editions, edition_records);
    dump(&works, work_records);
    dump(&authors, author_records);
    import_snapshot(&editions, &works, &authors, &database, None).unwrap();
    database
}

fn edition_query(title: &str, publishers: Vec<String>, publication_year: Option<i32>) -> EditionIdentityRequest {
    EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "query".to_owned(), title: title.to_owned(), authors: vec!["Alex Smith".to_owned()], publishers, publication_year }] }
}

fn wikidata_dump(path: &Path, records: &[serde_json::Value]) {
    let mut file = File::create(path).unwrap();
    writeln!(file, "[").unwrap();
    for (index, record) in records.iter().enumerate() {
        writeln!(file, "{record}{}", if index + 1 == records.len() { "" } else { "," }).unwrap();
    }
    writeln!(file, "]").unwrap();
}

#[test]
fn edition_identity_candidate_query_uses_title_and_work_indexes() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
            "title": "The Example Book", "isbn_13": ["9780306406157"]
        })],
        &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work"})],
        &[],
    );
    let connection = Connection::open(database).unwrap();
    // A one-row fixture makes SQLite rationally scan the outer detail
    // table. Give the planner production-like cardinality so this test
    // distinguishes that harmless fixture choice from the old candidate
    // full scan.
    connection
        .execute_batch(
            "WITH RECURSIVE ids(value) AS (SELECT 2 UNION ALL SELECT value+1 FROM ids WHERE value<10000)
                 INSERT INTO edition(edition_id,work_id) SELECT value,NULL FROM ids;
                 ANALYZE;",
        )
        .unwrap();
    let query = edition_candidate_query(1);
    let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap();
    let plan = statement
        .query_map(params![0, "the example book", 16, i64::try_from(MAX_RAW_TITLE_CANDIDATES).unwrap(), i64::try_from(MAX_RAW_TITLE_CANDIDATES).unwrap(), i64::try_from(MAX_REVERSE_TITLE_CANDIDATES + 1).unwrap()], |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");

    assert!(plan.contains("edition_bibliography_by_title"), "edition title index missing from plan:\n{plan}");
    assert!(plan.contains("work_bibliography_by_title"), "work title index missing from plan:\n{plan}");
    assert!(plan.contains("edition_by_work"), "work-to-edition index missing from plan:\n{plan}");
    assert!(plan.contains("SEARCH bibliography USING COVERING INDEX edition_bibliography_by_title"), "edition candidate lookup does not seek by title:\n{plan}");
    assert!(plan.contains("SEARCH work_bibliography USING COVERING INDEX work_bibliography_by_title"), "work candidate lookup does not seek by title:\n{plan}");
    assert!(plan.contains("SEARCH edition USING COVERING INDEX edition_by_work"), "work-to-edition lookup does not seek by work:\n{plan}");
    assert!(!plan.lines().any(|line| line.trim() == "SCAN edition"), "candidate lookup full-scans edition:\n{plan}");
}

#[test]
fn read_pool_bounds_connections_and_sqlite_memory_budgets() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    let service = MetadataService::open(database).unwrap();
    let connections = (0..QUERY_CONCURRENCY).map(|_| service.connection().unwrap()).collect::<Vec<_>>();

    assert!(service.state.pool.try_get().is_none(), "pool exceeded its configured connection ceiling");
    assert_eq!(service.state.query_slots.available_permits(), QUERY_CONCURRENCY as usize);
    assert_eq!(connections[0].query_row("PRAGMA cache_size", (), |row| row.get::<_, i64>(0)).unwrap(), -(READ_CACHE_KIB as i64));
    assert_eq!(connections[0].query_row("PRAGMA mmap_size", (), |row| row.get::<_, i64>(0)).unwrap(), READ_MMAP_BYTES as i64);
}

#[test]
fn description_capability_follows_the_snapshot_table() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    assert!(MetadataService::open(&database).unwrap().state.descriptions_available);

    let connection = Connection::open(&database).unwrap();
    connection.execute("DROP TABLE work_description", ()).unwrap();
    connection.execute("UPDATE snapshot SET schema_version=?1", [LEGACY_QUERY_SCHEMA_VERSION]).unwrap();
    drop(connection);

    assert!(!MetadataService::open(&database).unwrap().state.descriptions_available);
}

#[test]
fn description_sidecar_enriches_a_legacy_core_without_changing_it() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
            "isbn_13": ["9780306406157"], "dewey_decimal_class": ["500"]
        })],
        &[serde_json::json!({
            "key": "/works/OL10W",
            "description": {"value": "Sidecar description"},
            "subjects": ["History", "Europe", "Great Britain", "Unrelated noisy heading"]
        })],
        &[],
    );
    let connection = Connection::open(&database).unwrap();
    connection.execute("DROP TABLE work_description", ()).unwrap();
    connection.execute("UPDATE snapshot SET schema_version=?1", [LEGACY_QUERY_SCHEMA_VERSION]).unwrap();
    drop(connection);

    let sidecar = temporary.path().join("descriptions.sqlite");
    import_description_snapshot(&temporary.path().join("ol_dump_works_2026-08-01.txt.gz"), &sidecar, "2026-08-01", None).unwrap();
    let service = MetadataService::open_with_descriptions(&database, Some(&sidecar)).unwrap();
    let response = service.classify_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();

    assert!(service.state.descriptions_available);
    assert_eq!(response.results[0].matches[0].description.as_deref(), Some("Sidecar description"));
    assert!(response.results[0].matches[0].classifications.iter().any(|classification| classification.scheme == ClassificationScheme::Bisac && classification.source == ClassificationSource::Work));
    let sidecar_connection = Connection::open(sidecar).unwrap();
    assert_eq!(sidecar_connection.query_row("SELECT description_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(sidecar_connection.query_row("SELECT subject_heading_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 4);
    assert_eq!(sidecar_connection.query_row("SELECT matched_subject_heading_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 3);
    assert!(sidecar_connection.query_row("SELECT bisac_assignment_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap() >= 1);
    assert_eq!(sidecar_connection.query_row("SELECT COUNT(*) FROM work_bisac_subject WHERE source_subject='Unrelated noisy heading'", (), |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(sidecar_connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='edition')", (), |row| row.get::<_, bool>(0)).unwrap(), false);
}

#[test]
fn author_profile_sidecar_enriches_explicit_authorship_without_altering_the_core_schema() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
            "authors": [{"key": "/authors/OL20A"}], "isbn_13": ["9780306406157"]
        })],
        &[serde_json::json!({
            "key": "/works/OL10W", "authors": [{"author": {"key": "/authors/OL20A"}}]
        })],
        &[serde_json::json!({
            "key": "/authors/OL20A", "name": "Alex Smith", "bio": {"value": "Author biography"}
        })],
    );
    let profile = temporary.path().join("author-profiles.sqlite");
    import_author_profile_snapshot(&temporary.path().join("ol_dump_authors_2026-08-01.txt.gz"), &profile, "2026-08-01", None).unwrap();

    let service = MetadataService::open_with_supplements(&database, None::<&Path>, Some(&profile)).unwrap();
    let response = service.enrich_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    let author = &response.results[0].matches[0].authors[0];
    assert_eq!(author.open_library_author_id, "OL20A");
    assert_eq!(author.name.as_deref(), Some("Alex Smith"));
    assert_eq!(author.biography.as_deref(), Some("Author biography"));

    let profile_connection = Connection::open(profile).unwrap();
    assert!(!profile_connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name IN ('work_author','edition_author'))", (), |row| row.get::<_, bool>(0)).unwrap());
    let core_connection = Connection::open(database).unwrap();
    assert!(!core_connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('author') WHERE name='biography')", (), |row| row.get::<_, bool>(0)).unwrap());
    assert_eq!(core_connection.query_row("SELECT COUNT(*) FROM work_author WHERE work_id=10 AND author_id=20", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(core_connection.query_row("SELECT COUNT(*) FROM edition_author WHERE edition_id=1 AND author_id=20", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn description_sidecar_must_match_the_core_dump_date() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    let works = temporary.path().join("sidecar-works.txt.gz");
    dump(&works, &[]);
    let sidecar = temporary.path().join("descriptions.sqlite");
    import_description_snapshot(&works, &sidecar, "2026-07-31", None).unwrap();

    let failure = MetadataService::open_with_descriptions(&database, Some(&sidecar)).err().expect("mismatched sidecar must be rejected");
    assert!(failure.to_string().contains("does not match metadata snapshot"));
}

#[test]
fn database_query_deadline_interrupts_expensive_sql() {
    let connection = Connection::open_in_memory().unwrap();
    install_query_deadline(&connection, Duration::ZERO);
    let failure = connection.query_row("WITH RECURSIVE numbers(value) AS (VALUES(1) UNION ALL SELECT value+1 FROM numbers WHERE value<10000000) SELECT SUM(value) FROM numbers", (), |row| row.get::<_, i64>(0)).unwrap_err();
    assert!(matches!(failure, rusqlite::Error::SqliteFailure(error, _) if error.code == rusqlite::ErrorCode::OperationInterrupted));
}

#[test]
fn popularity_snapshot_uses_current_schema_without_resolved_wikidata_entities() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
    let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
    let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
    let ratings = temporary.path().join("ol_dump_ratings_2026-08-01.txt.gz");
    let reading_log = temporary.path().join("ol_dump_reading-log_2026-08-01.txt.gz");
    let database = temporary.path().join("popularity.sqlite");
    dump(
        &editions,
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
            "title": "Example", "isbn_13": ["9780306406157"], "subject_places": ["Paris"]
        })],
    );
    dump(&works, &[serde_json::json!({"key": "/works/OL10W", "title": "Example", "description": {"type": "/type/text", "value": "Open Library summary."}})]);
    dump(&authors, &[]);
    tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t5\t2026-01-01"]);
    tsv_dump(&reading_log, &["/works/OL10W\t/books/OL1M\tWant to Read\t2026-01-01"]);

    import_snapshot_with_popularity(&editions, &works, &authors, &ratings, &reading_log, &database, None).unwrap();
    let validation = Connection::open(&database).unwrap();
    assert_eq!(validation.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    assert_eq!(validation.query_row("SELECT COUNT(*) FROM entity", (), |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(validation.query_row("SELECT rating_count FROM work_popularity WHERE work_id=10", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
    drop(validation);

    let service = MetadataService::open(&database).unwrap();
    let response = service.enrich_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    assert_eq!(response.results[0].matches[0].popularity.as_ref().map(|value| value.rating_sum), Some(5));
    assert_eq!(response.results[0].matches[0].description.as_deref(), Some("Open Library summary."));
    let classified = service.classify_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    assert_eq!(classified.results[0].matches[0].description.as_deref(), Some("Open Library summary."));
    assert!(response.places.is_empty());
    assert!(response.people.is_empty());
}

#[test]
fn popularity_can_be_added_without_rescanning_open_library() {
    let temporary = tempfile::tempdir().unwrap();
    let source = snapshot_from(temporary.path(), &[serde_json::json!({"key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "isbn_13": ["9780306406157"]})], &[serde_json::json!({"key": "/works/OL10W", "title": "Example"})], &[]);
    let source_connection = Connection::open(&source).unwrap();
    source_connection.execute("UPDATE snapshot SET schema_version=7 WHERE singleton=1", ()).unwrap();
    source_connection.execute("DROP TABLE work_popularity", ()).unwrap();
    drop(source_connection);
    let ratings = temporary.path().join("ratings.txt.gz");
    let reading_log = temporary.path().join("reading-log.txt.gz");
    let output = temporary.path().join("with-popularity.sqlite");
    tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t4\t2026-01-01"]);
    tsv_dump(&reading_log, &["/works/OL10W\t/books/OL1M\tAlready Read\t2026-01-01"]);

    add_popularity_to_snapshot(&source, &ratings, &reading_log, &output, None).unwrap();

    let source_connection = Connection::open(&source).unwrap();
    assert_eq!(source_connection.query_row("SELECT schema_version FROM snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 7);
    let output_connection = Connection::open(&output).unwrap();
    assert_eq!(output_connection.query_row("SELECT schema_version FROM snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    assert_eq!(output_connection.query_row("SELECT already_read_count FROM work_popularity WHERE work_id=10", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn snapshot_enrichment_resolves_work_authors_and_external_ids() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
    let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
    let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
    let wikidata = temporary.path().join("wikidata-2026-08-01.json");
    let ratings = temporary.path().join("ol_dump_ratings_2026-08-01.txt.gz");
    let reading_log = temporary.path().join("ol_dump_reading-log_2026-08-01.txt.gz");
    let database = temporary.path().join("snapshot.sqlite");
    dump(
        &editions,
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
                "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"],
                "subject_places": ["Paris"], "subject_people": ["Ada Lovelace (1815-1852)"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                "dewey_decimal_class": ["999"], "lc_classifications": ["QA76.73.R87"]
            }),
            serde_json::json!({
                "key": "/books/OL3M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2005", "isbn_13": ["9780821338278"]
            }),
        ],
    );
    dump(
        &works,
        &[serde_json::json!({
            "key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}], "lc_classifications": ["QA76.6"],
            "subject_places": ["France", "Paris"], "subject_people": ["Charles Babbage"]
        })],
    );
    dump(
        &authors,
        &[serde_json::json!({
            "key": "/authors/OL100A", "name": "Alex Smith", "remote_ids": {"viaf": "12345", "wikidata": "Q42"}
        })],
    );
    wikidata_dump(
        &wikidata,
        &[
            serde_json::json!({"id":"Q90","labels":{"en":{"value":"Paris"}},"aliases":{},"claims":{"P31":[entity_claim("Q515")],"P131":[entity_claim("Q161741")],"P17":[entity_claim("Q142")],"P625":[{"mainsnak":{"datavalue":{"value":{"latitude":48.8566,"longitude":2.3522}}}}]}}),
            serde_json::json!({"id":"Q161741","labels":{"en":{"value":"Île-de-France"}},"aliases":{},"claims":{"P131":[entity_claim("Q142")]}}),
            serde_json::json!({"id":"Q142","labels":{"en":{"value":"France"}},"aliases":{},"claims":{"P31":[entity_claim("Q6256")],"P30":[entity_claim("Q46")]}}),
            serde_json::json!({"id":"Q46","labels":{"en":{"value":"Europe"}},"aliases":{},"claims":{"P31":[entity_claim("Q5107")]}}),
            serde_json::json!({"id":"Q7259","labels":{"en":{"value":"Ada Lovelace"}},"aliases":{},"claims":{"P31":[entity_claim("Q5")]}}),
            serde_json::json!({"id":"Q1510","labels":{"en":{"value":"Charles Babbage"}},"aliases":{},"claims":{"P31":[entity_claim("Q5")]}}),
            serde_json::json!({"id":"Q999999999","labels":{"en":{"value":"Unreferenced City"}},"aliases":{"en":[{"value":"Unused place"}]},"claims":{"P31":[entity_claim("Q515")],"P17":[entity_claim("Q142")]}}),
        ],
    );
    tsv_dump(&ratings, &["/works/OL10W\t/books/OL1M\t5\t2026-01-01", "/works/OL10W\t\\N\t3\t2026-01-02", "/works/OL999W\t\\N\t5\t2026-01-03"]);
    tsv_dump(
        &reading_log,
        &[
            "/works/OL10W\t/books/OL1M\tWant to Read\t2026-01-01",
            "/works/OL10W\t\\N\tWant to Read\t2026-01-02",
            "/works/OL10W\t\\N\tCurrently Reading\t2026-01-03",
            "/works/OL10W\t\\N\tAlready Read\t2026-01-04",
            "/works/OL999W\t\\N\tAlready Read\t2026-01-05",
        ],
    );

    import_rich_snapshot_with_popularity(&editions, &works, &authors, &wikidata, &ratings, &reading_log, &database, None).unwrap();
    let validation = Connection::open(&database).unwrap();
    assert_eq!(validation.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).unwrap(), WIKIDATA_SCHEMA_VERSION);
    assert_eq!(
        validation.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE '%_stage'", (), |row| row.get::<_, i64>(0)).unwrap(),
        0,
        "all append-only import staging tables must be removed from the immutable snapshot"
    );
    assert_eq!(
        validation
            .query_row("SELECT COUNT(*) FROM import_checkpoint WHERE phase IN ('editions_materialize','works_materialize','work_titles_materialize','authors_materialize','wikidata_materialize') AND completed=1", (), |row| row
                .get::<_, i64>(0))
            .unwrap(),
        5
    );
    let service = MetadataService::open(&database).unwrap();
    let operations = load_operational_stats(&service.state).unwrap();
    assert_eq!(operations.status, "available");
    assert_eq!(operations.schema_version, WIKIDATA_SCHEMA_VERSION);
    assert!(operations.database_bytes > 0);
    assert!(operations.edition_records > 0);
    assert!(operations.work_records > 0);
    assert!(operations.author_records > 0);
    assert!(!operations.metrics.is_empty());
    let response = service.enrich(MetadataEnrichmentRequest { isbns: vec!["978-0-306-40615-7".to_owned()] }).unwrap();
    assert_eq!(response.snapshot.dump_date, "2026-08-01");
    assert_eq!(response.results[0].status, LookupStatus::Matched);
    let matched = &response.results[0].matches[0];
    assert_eq!(matched.open_library_work_id.as_deref(), Some("OL10W"));
    assert_eq!(matched.authors.len(), 1);
    assert_eq!(matched.authors[0].open_library_author_id, "OL100A");
    assert_eq!(matched.authors[0].name.as_deref(), Some("Alex Smith"));
    assert_eq!(matched.authors[0].source, AuthorSource::ExactEdition);
    assert_eq!(matched.authors[0].position, 0);
    assert_eq!(matched.authors[0].identifiers, vec![AuthorIdentifier { authority: "viaf".to_owned(), value: "12345".to_owned() }, AuthorIdentifier { authority: "wikidata".to_owned(), value: "Q42".to_owned() }]);
    assert!(matched.classifications.iter().any(|classification| classification.notation == "005.1" && classification.source == ClassificationSource::ExactEdition));
    assert!(matched.classifications.iter().any(|classification| classification.notation == "QA76.6" && classification.source == ClassificationSource::Work));
    assert!(matched.classifications.iter().any(|classification| classification.notation == "QA76.73.R87" && classification.source == ClassificationSource::Work));
    assert!(matched.classifications.iter().any(|classification| classification.notation == "999" && classification.source == ClassificationSource::Work));

    let rich = service.enrich_rich(MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    let matched = &rich.results[0].matches[0];
    assert_eq!(matched.places.iter().map(|place| (&place.name, place.source)).collect::<Vec<_>>(), vec![(&"France".to_owned(), EntitySource::Work), (&"Paris".to_owned(), EntitySource::ExactEdition)]);
    assert_eq!(matched.people.len(), 2);
    assert_eq!(matched.places.iter().find(|place| place.name == "Paris").and_then(|place| place.wikidata_id.as_deref()), Some("Q90"));
    assert_eq!(matched.people.iter().find(|person| person.name.starts_with("Ada Lovelace")).and_then(|person| person.wikidata_id.as_deref()), Some("Q7259"));
    assert!(rich.places.iter().any(|place| place.wikidata_id == "Q90" && place.country_ids == ["Q142"]));
    assert!(rich.places.iter().any(|place| place.wikidata_id == "Q46"));
    assert!(rich.people.iter().any(|person| person.wikidata_id == "Q7259"));
    assert_eq!(matched.popularity, Some(metadata_contract::WorkPopularity { want_to_read_count: 2, currently_reading_count: 1, already_read_count: 1, rating_count: 2, rating_sum: 8 }));
    let identity = service
        .resolve_editions(EditionIdentityRequest {
            queries: vec![
                EditionIdentityQuery { query_id: "local-1".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Smith, Alex".to_owned()], publishers: vec!["North-Wind Press".to_owned()], publication_year: Some(2004) },
                EditionIdentityQuery { query_id: "work-title".to_owned(), title: "Canonical Example Work".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: vec!["North Wind Press".to_owned()], publication_year: Some(2004) },
                EditionIdentityQuery { query_id: "ambiguous".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: vec!["North Wind Press".to_owned()], publication_year: None },
                EditionIdentityQuery { query_id: "insufficient".to_owned(), title: "The Example Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
            ],
        })
        .unwrap();
    assert_eq!(identity.results[0].status, EditionIdentityStatus::Matched);
    let identity_match = identity.results[0].matched.as_ref().unwrap();
    assert_eq!(identity_match.canonical_isbn13, "9780306406157");
    assert_eq!(identity_match.open_library_edition_id, "OL1M");
    assert_eq!(identity_match.title_match, EditionTitleMatch::Edition);
    assert!(identity_match.matched_publisher && identity_match.matched_publication_year);
    assert_eq!(identity.results[1].status, EditionIdentityStatus::Matched);
    assert_eq!(identity.results[1].matched.as_ref().unwrap().title_match, EditionTitleMatch::Work);
    for result in &identity.results[2..=3] {
        assert_eq!(result.status, EditionIdentityStatus::Matched);
        assert_eq!(result.matched.as_ref().and_then(|matched| matched.open_library_work_id.as_deref()), Some("OL10W"));
    }
    let retained = Connection::open(&database).unwrap().query_row("SELECT COUNT(*) FROM entity WHERE wikidata_id=999999999", (), |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(retained, 0, "unreferenced Wikidata geography must be pruned from the snapshot");
}

#[test]
fn wikidata_scan_resumes_from_committed_record_checkpoint() {
    let temporary = tempfile::tempdir().unwrap();
    let wikidata = temporary.path().join("wikidata.json");
    wikidata_dump(
        &wikidata,
        &[
            serde_json::json!({"id":"Q90","labels":{"en":{"value":"Paris"}},"aliases":{},"claims":{"P31":[entity_claim("Q515")],"P17":[entity_claim("Q142")]}}),
            serde_json::json!({"id":"Q142","labels":{"en":{"value":"France"}},"aliases":{},"claims":{"P31":[entity_claim("Q6256")],"P30":[entity_claim("Q46")]}}),
            serde_json::json!({"id":"Q46","labels":{"en":{"value":"Europe"}},"aliases":{},"claims":{"P31":[entity_claim("Q5107")]}}),
        ],
    );
    let database = temporary.path().join("resumable.sqlite");
    let mut connection = Connection::open(&database).unwrap();
    configure_import(&connection).unwrap();
    create_schema(&connection).unwrap();
    connection
        .execute_batch(
            "INSERT INTO work_entity_evidence(work_id,kind,name,normalized_name) VALUES(1,1,'Paris','paris');
                 INSERT INTO entity_stage(wikidata_id,kind,label,fictional) VALUES(90,1,'Paris',0);
                 INSERT INTO place_country_stage(place_id,country_id) VALUES(90,142);
                 INSERT INTO wikidata_candidate_stage(kind,normalized_name,wikidata_id) VALUES(1,'paris',90);
                 INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('wikidata_scan',1,0);",
        )
        .unwrap();

    assert_eq!(import_wikidata(&mut connection, &wikidata, None).unwrap(), 3);
    assert_eq!(completed_phase(&connection, "wikidata_scan").unwrap(), Some(3));
    assert_eq!(completed_phase(&connection, "wikidata_finalize").unwrap(), Some(3));
    assert_eq!(completed_phase(&connection, "wikidata_materialize").unwrap(), Some(0));
    let retained = connection.query_row("SELECT group_concat(wikidata_id,',') FROM (SELECT wikidata_id FROM entity ORDER BY wikidata_id)", (), |row| row.get::<_, String>(0)).unwrap();
    assert_eq!(retained, "46,90,142");
    assert_eq!(import_wikidata(&mut connection, &wikidata, None).unwrap(), 3, "a completed materialized phase is idempotent");
}

#[test]
fn open_library_scan_resumes_from_committed_record_checkpoint() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = temporary.path().join("editions.txt.gz");
    dump(&editions, &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]}), serde_json::json!({"key":"/books/OL2M","isbn_13":["9780821338278"]})]);
    let database = temporary.path().join("resumable-open-library.sqlite");
    let mut connection = Connection::open(&database).unwrap();
    configure_import(&connection).unwrap();
    create_schema(&connection).unwrap();
    connection
        .execute_batch(
            "INSERT INTO edition_stage(edition_id) VALUES(1);
                 INSERT INTO edition_isbn_stage(isbn13,edition_id) VALUES(9780306406157,1);
                 INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('editions',1,0);",
        )
        .unwrap();

    assert_eq!(import_editions(&mut connection, &editions, None).unwrap(), 2);
    assert_eq!(completed_phase(&connection, "editions").unwrap(), Some(2));
    assert_eq!(completed_phase(&connection, "editions_materialize").unwrap(), Some(0));
    let retained = connection.query_row("SELECT group_concat(edition_id,',') FROM (SELECT edition_id FROM edition ORDER BY edition_id)", (), |row| row.get::<_, String>(0)).unwrap();
    assert_eq!(retained, "1,2");
    assert_eq!(import_editions(&mut connection, &editions, None).unwrap(), 2, "a completed materialized phase is idempotent");
}

/// An edition with no `/works` link leaves `work_bibliography` unmatched by
/// the candidate LEFT JOIN, so its title-comparison column is NULL rather
/// than false. Reading it as a plain `bool` fails the entire batch request.
#[test]
fn edition_identity_resolves_an_edition_that_has_no_linked_work() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
            "title": "Standalone Edition", "publishers": ["North Wind Press"], "publish_date": "2004",
            "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
        })],
        &[],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("Standalone Edition", vec!["North Wind Press".to_owned()], Some(2004))).expect("an edition without a work must not fail the request");
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    let matched = response.results[0].matched.as_ref().unwrap();
    assert_eq!(matched.open_library_edition_id, "OL1M");
    assert_eq!(matched.open_library_work_id, None);
    assert_eq!(matched.title_match, EditionTitleMatch::Edition);
}

#[test]
fn edition_identity_rejects_an_edition_without_lcc_or_ddc() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "title": "Unclassified Edition", "isbn_13": ["9780306406157"]
        })],
        &[],
        &[],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("Unclassified Edition", Vec::new(), None)).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::NoMatch);
    assert!(response.results[0].matched.is_none());
}

/// An untitled sibling edition of the same work has no `edition_bibliography`
/// row, so its title-comparison column is NULL. One such row in the
/// candidate set must not destroy the lookup for the edition that does match.
#[test]
fn edition_identity_survives_an_untitled_sibling_edition_of_the_same_work() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
                "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("Canonical Example Work", vec!["North Wind Press".to_owned()], Some(2004))).expect("an untitled candidate must not fail the request");
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    let matched = response.results[0].matched.as_ref().unwrap();
    assert_eq!(matched.open_library_edition_id, "OL1M");
    assert_eq!(matched.title_match, EditionTitleMatch::Work);
}

/// Matches are emitted per ISBN but the match/ambiguity decision counts
/// them, so a single edition that carries more than one ISBN is reported
/// ambiguous and callers that require `Matched` silently give up on it.
#[test]
fn edition_identity_matches_a_single_edition_that_carries_several_isbns() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
            "title": "The Example Book", "publishers": ["North Wind Press"], "publish_date": "2004",
            "isbn_13": ["9780306406157"], "isbn_10": ["0821338277"], "dewey_decimal_class": ["005.1"]
        })],
        &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("The Example Book", vec!["North Wind Press".to_owned()], Some(2004))).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched, "two ISBNs on one edition are still one edition");
    let matched = response.results[0].matched.as_ref().unwrap();
    assert_eq!(matched.open_library_edition_id, "OL1M");
    assert!(matched.matched_publisher && matched.matched_publication_year);
}

#[test]
fn edition_identity_accepts_a_local_subtitle_and_newer_publication_details() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
            "title": "The Example Book", "publishers": ["Old Press"], "publish_date": "2004", "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
        })],
        &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("Cambridge concise histories: The Example Book (Routledge Classics)", vec!["New Press".to_owned()], Some(2026))).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    let matched = response.results[0].matched.as_ref().unwrap();
    assert_eq!(matched.open_library_edition_id, "OL1M");
    assert!(!matched.matched_publisher);
    assert!(!matched.matched_publication_year);
}

#[test]
fn edition_identity_prefers_the_longest_title_match_before_the_review_limit() {
    let temporary = tempfile::tempdir().unwrap();
    let mut editions = (1..=300)
        .map(|edition_id| {
            serde_json::json!({
                "key": format!("/books/OL{edition_id}M"),
                "authors": [{"key": "/authors/OL100A"}],
                "title": "Physical Chemistry",
                "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            })
        })
        .collect::<Vec<_>>();
    editions.push(serde_json::json!({
        "key": "/books/OL1000M",
        "authors": [{"key": "/authors/OL100A"}],
        "title": "Physical Chemistry: A Very Short Introduction",
        "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
    }));
    let database = snapshot_from(temporary.path(), &editions, &[], &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})]);
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("Physical Chemistry: A Very Short Introduction", Vec::new(), None)).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    assert!(!response.results[0].candidates_truncated);
    assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL1000M");
}

#[test]
fn edition_identity_uses_author_only_to_break_a_longest_substring_tie() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                "title": "A Concise History of Spain", "publishers": ["First Press"], "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "authors": [{"key": "/authors/OL200A"}],
                "title": "A Concise History of Spain", "publishers": ["Preferred Press"], "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"}), serde_json::json!({"key": "/authors/OL200A", "name": "Jamie Jones"})],
    );
    let service = MetadataService::open(&database).unwrap();
    let title = "Cambridge concise histories: A Concise History of Spain (New edition)".to_owned();
    let response = service
        .resolve_editions(EditionIdentityRequest {
            queries: vec![
                EditionIdentityQuery { query_id: "title-only".to_owned(), title: title.clone(), authors: Vec::new(), publishers: vec!["Preferred Press".to_owned()], publication_year: None },
                EditionIdentityQuery { query_id: "with-author".to_owned(), title, authors: vec!["Alex Smith".to_owned()], publishers: vec!["Preferred Press".to_owned()], publication_year: None },
            ],
        })
        .unwrap();

    assert_eq!(response.results[0].status, EditionIdentityStatus::Ambiguous);
    assert_eq!(response.results[0].candidates.len(), 2);
    assert_eq!(response.results[1].status, EditionIdentityStatus::Matched);
    assert_eq!(response.results[1].matched.as_ref().unwrap().open_library_edition_id, "OL1M");
}

#[test]
fn edition_identity_requires_an_author_match_for_automatic_acceptance() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[serde_json::json!({
            "key": "/books/OL1M", "authors": [{"key": "/authors/OL200A"}],
            "title": "Complete Works Of", "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
        })],
        &[],
        &[serde_json::json!({"key": "/authors/OL200A", "name": "Jamie Jones"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service
        .resolve_editions(EditionIdentityRequest {
            queries: vec![EditionIdentityQuery { query_id: "wrong-author".to_owned(), title: "Delphi Complete Works Of Athenaeus".to_owned(), authors: vec!["Athenaeus".to_owned()], publishers: Vec::new(), publication_year: None }],
        })
        .unwrap();

    assert_eq!(response.results[0].status, EditionIdentityStatus::NoMatch);
    assert!(response.results[0].matched.is_none());
    assert!(response.results[0].candidates.is_empty());
}

#[test]
fn edition_identity_accepts_one_work_after_author_matched_editions_are_collapsed() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "Complete Works of Homer", "publishers": ["First Press"], "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "Complete Works of Homer", "publishers": ["Second Press"], "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[serde_json::json!({"key": "/works/OL10W", "title": "Complete Works of Homer", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Homer"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service
        .resolve_editions(EditionIdentityRequest {
            queries: vec![EditionIdentityQuery {
                query_id: "same-work".to_owned(),
                title: "Delphi Complete Works of Homer".to_owned(),
                authors: vec!["Homer".to_owned()],
                publishers: vec!["Delphi Classics".to_owned()],
                publication_year: None,
            }],
        })
        .unwrap();

    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_work_id.as_deref(), Some("OL10W"));
    assert!(!response.results[0].matched.as_ref().unwrap().matched_publisher);
}

#[test]
fn edition_identity_combines_inferred_author_and_year_to_remove_other_candidates() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                "title": "Algorithms", "publish_date": "1999", "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "authors": [{"key": "/authors/OL100A"}],
                "title": "Algorithms", "publish_date": "2019", "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL3M", "authors": [{"key": "/authors/OL200A"}],
                "title": "Algorithms", "publish_date": "2019", "isbn_13": ["9781501168697"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Jeff Erickson"}), serde_json::json!({"key": "/authors/OL200A", "name": "Fethi Rabhi"})],
    );
    let service = MetadataService::open(&database).unwrap();
    let request = EditionIdentityRequest {
        queries: vec![EditionIdentityQuery { query_id: "noisy-filename".to_owned(), title: "Algorithms".to_owned(), authors: vec!["Jeff Erickson".to_owned()], publishers: Vec::new(), publication_year: Some(2019) }],
    };

    let response = service.resolve_editions(request).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
}

#[test]
fn edition_identity_batch_keeps_results_attached_to_each_query() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "authors": [{"key": "/authors/OL100A"}],
                "title": "First Book", "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "authors": [{"key": "/authors/OL100A"}],
                "title": "Second Book", "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();
    let request = EditionIdentityRequest {
        queries: vec![
            EditionIdentityQuery { query_id: "second".to_owned(), title: "Second Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
            EditionIdentityQuery { query_id: "missing".to_owned(), title: "Missing Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
            EditionIdentityQuery { query_id: "first".to_owned(), title: "First Book".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: None },
        ],
    };

    let response = service.resolve_editions(request).unwrap();
    assert_eq!(response.results.iter().map(|result| result.query_id.as_str()).collect::<Vec<_>>(), vec!["second", "missing", "first"]);
    assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
    assert_eq!(response.results[1].status, EditionIdentityStatus::NoMatch);
    assert_eq!(response.results[2].matched.as_ref().unwrap().open_library_edition_id, "OL1M");
}

#[test]
fn edition_identity_uses_publisher_only_to_break_a_title_author_tie() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["Old Press"], "publish_date": "2004", "isbn_13": ["9780306406157"], "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}], "authors": [{"key": "/authors/OL100A"}],
                "title": "The Example Book", "publishers": ["Preferred Press"], "publish_date": "2010", "isbn_13": ["9780821338278"], "dewey_decimal_class": ["005.1"]
            }),
        ],
        &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "authors": [{"author": {"key": "/authors/OL100A"}}]})],
        &[serde_json::json!({"key": "/authors/OL100A", "name": "Alex Smith"})],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.resolve_editions(edition_query("The Example Book", vec!["Preferred Press".to_owned()], None)).unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Matched);
    assert_eq!(response.results[0].matched.as_ref().unwrap().open_library_edition_id, "OL2M");
}

#[test]
fn edition_identity_combines_classification_sources_and_suggests_only_one_edition_per_work() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                "title": "The Example Book", "isbn_13": ["9780306406157"],
                "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                "title": "The Example Book", "isbn_13": ["9780821338278"],
                "lc_classifications": ["QA76.73.R87"]
            }),
        ],
        &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book", "lc_classifications": ["QA76.6"]})],
        &[],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service
        .resolve_editions(EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "query".to_owned(), title: "The Example Book".to_owned(), authors: Vec::new(), publishers: Vec::new(), publication_year: None }] })
        .unwrap();
    assert_eq!(response.results[0].status, EditionIdentityStatus::Ambiguous);
    assert_eq!(response.results[0].candidates.len(), 1);
    assert_eq!(response.results[0].candidates[0].open_library_work_id.as_deref(), Some("OL10W"));
    assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "005.1" && classification.source == ClassificationSource::ExactEdition));
    assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "QA76.6" && classification.source == ClassificationSource::Work));
    assert!(response.results[0].candidates[0].classifications.iter().any(|classification| classification.notation == "QA76.73.R87" && classification.source == ClassificationSource::Work));
}

#[test]
fn classification_lookup_requires_consensus_when_sibling_evidence_is_sparse() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key": "/books/OL1M", "works": [{"key": "/works/OL10W"}],
                "title": "The Example Book", "isbn_13": ["9780306406157"],
                "dewey_decimal_class": ["005.1"]
            }),
            serde_json::json!({
                "key": "/books/OL2M", "works": [{"key": "/works/OL10W"}],
                "title": "The Example Book", "isbn_13": ["9780821338278"],
                "lc_classifications": ["QA76.73.R87"]
            }),
        ],
        &[serde_json::json!({"key": "/works/OL10W", "title": "The Example Book"})],
        &[],
    );
    let service = MetadataService::open(&database).unwrap();

    let response = service.lookup(ClassificationRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    assert_eq!(response.results[0].matches[0].classifications, vec![Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "005.1".to_owned(), source: ClassificationSource::ExactEdition }]);
}

#[test]
fn classification_lookup_uses_all_supported_codes_for_a_large_work_union() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = [
        serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780306406157"]}),
        serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780821338278"],"lc_classifications":["QA76.6.A1"],"dewey_decimal_class":["005.1"]}),
        serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9783161484100"],"lc_classifications":["QA76.6.B2"],"dewey_decimal_class":["005.1"]}),
        serde_json::json!({"key":"/books/OL4M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9781861972712"],"lc_classifications":["PN1993.A1"],"dewey_decimal_class":["791.43"]}),
        serde_json::json!({"key":"/books/OL5M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780140328721"],"lc_classifications":["PN1993.B2"],"dewey_decimal_class":["791.43"]}),
    ];
    let noisy = (1..=13).map(|number| format!("B{number}")).collect::<Vec<_>>();
    let works = [serde_json::json!({"key":"/works/OL10W","title":"Example","lc_classifications":noisy})];
    let database = snapshot_from(temporary.path(), &editions, &works, &[]);
    let service = MetadataService::open(&database).unwrap();

    let response = service.lookup(ClassificationRequest { isbns: vec!["9780306406157".to_owned()] }).unwrap();
    let classifications = &response.results[0].matches[0].classifications;
    assert_eq!(
        classifications,
        &[
            Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "PN1993".to_owned(), source: ClassificationSource::Work },
            Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.6".to_owned(), source: ClassificationSource::Work },
            Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "005.1".to_owned(), source: ClassificationSource::Work },
            Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "791.43".to_owned(), source: ClassificationSource::Work },
        ]
    );
}

#[test]
fn serving_rejects_a_snapshot_outside_the_supported_schema_set() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    let connection = Connection::open(&database).unwrap();
    connection.execute("UPDATE snapshot SET schema_version=?1 WHERE singleton=1", [LEGACY_QUERY_SCHEMA_VERSION - 1]).unwrap();
    drop(connection);

    let failure = match MetadataService::open(&database) {
        Ok(_) => panic!("an unsupported snapshot must not be served"),
        Err(failure) => failure,
    };
    assert!(failure.to_string().contains(&format!("schema {}", LEGACY_QUERY_SCHEMA_VERSION - 1)), "unexpected rejection: {failure}");
    assert!(failure.to_string().contains(&format!("expected {LEGACY_QUERY_SCHEMA_VERSION}, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")), "unexpected rejection: {failure}");
}

/// Only the current builder version is supported, so an unfinished build
/// from an older builder must be rejected without mutating its checkpoints.
#[test]
fn unfinished_build_from_an_older_builder_is_rejected_without_being_mutated() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = temporary.path().join("ol_dump_editions_2026-08-01.txt.gz");
    let works = temporary.path().join("ol_dump_works_2026-08-01.txt.gz");
    let authors = temporary.path().join("ol_dump_authors_2026-08-01.txt.gz");
    dump(&editions, &[serde_json::json!({"key": "/books/OL1M", "works": [{"key": "/works/OL10W"}], "title": "The Example Book", "isbn_13": ["9780306406157"]})]);
    dump(&works, &[serde_json::json!({"key": "/works/OL10W", "title": "Canonical Example Work"})]);
    dump(&authors, &[]);
    let database = temporary.path().join("snapshot.sqlite");
    let building = building_path(&database);

    // Model an unfinished v6 build by removing the later work-title tables.
    let connection = Connection::open(&building).unwrap();
    configure_import(&connection).unwrap();
    create_schema(&connection).unwrap();
    register_import_inputs(&connection, &editions, &works, &authors, None, None, None).unwrap();
    connection
        .execute_batch(
            "DROP INDEX work_bibliography_by_title;
                 DROP TABLE work_bibliography;
                 DROP TABLE work_bibliography_stage;
                 UPDATE metadata_metric SET value=6 WHERE name='builder_schema_version';",
        )
        .unwrap();
    drop(connection);

    let failure = import_snapshot(&editions, &works, &authors, &database, None).expect_err("an unfinished build from an older builder must be rejected");
    assert!(failure.to_string().contains("builder schema"), "unexpected rejection: {failure}");

    let connection = Connection::open(&building).unwrap();
    assert_eq!(connection.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", (), |row| row.get::<_, i64>(0)).unwrap(), 6, "a rejected resume must not restamp the builder version");
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='work_bibliography'", (), |row| row.get::<_, i64>(0)).unwrap(), 0, "a rejected resume must not commit schema changes");
}
