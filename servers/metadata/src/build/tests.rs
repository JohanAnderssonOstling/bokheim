use crate::*;
use crate::build::import::core::{completed_phase, configure_import, create_schema, import_editions, register_import_inputs};
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

pub(crate) fn snapshot_from(directory: &Path, edition_records: &[serde_json::Value], work_records: &[serde_json::Value], author_records: &[serde_json::Value]) -> PathBuf {
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

#[test]
fn description_sidecar_enriches_a_core_without_embedded_descriptions() {
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
    drop(connection);

    let sidecar = temporary.path().join("descriptions.sqlite");
    import_description_snapshot(&temporary.path().join("ol_dump_works_2026-08-01.txt.gz"), &sidecar, "2026-08-01", None).unwrap();
    let service = FixtureService::open_with_descriptions(&database, Some(&sidecar)).unwrap();
    let response = service.enrich_rich(vec!["9780306406157".to_owned()]).unwrap();

    assert_eq!(response[0].matches[0].description.as_deref(), Some("Sidecar description"));
    assert!(response[0].matches[0].classifications.iter().any(|classification| classification.scheme == ClassificationScheme::Bisac && classification.source == ClassificationSource::Work));
    let sidecar_connection = Connection::open(sidecar).unwrap();
    assert_eq!(sidecar_connection.query_row("SELECT description_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(sidecar_connection.query_row("SELECT subject_heading_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 4);
    assert_eq!(sidecar_connection.query_row("SELECT matched_subject_heading_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap(), 3);
    assert!(sidecar_connection.query_row("SELECT bisac_assignment_records FROM description_snapshot", (), |row| row.get::<_, i64>(0)).unwrap() >= 1);
    assert_eq!(sidecar_connection.query_row("SELECT COUNT(*) FROM work_bisac_subject WHERE source_subject='Unrelated noisy heading'", (), |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(sidecar_connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='edition')", (), |row| row.get::<_, bool>(0)).unwrap(), false);
}


#[test]
fn description_sidecar_must_match_the_core_dump_date() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    let works = temporary.path().join("sidecar-works.txt.gz");
    dump(&works, &[]);
    let sidecar = temporary.path().join("descriptions.sqlite");
    import_description_snapshot(&works, &sidecar, "2026-07-31", None).unwrap();

    let failure = FixtureService::open_with_descriptions(&database, Some(&sidecar)).err().expect("mismatched sidecar must be rejected");
    assert!(failure.to_string().contains("does not match metadata snapshot"));
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
    let service = FixtureService::open(&database).unwrap();

    let response = service.lookup(vec!["9780306406157".to_owned()]).unwrap();
    assert_eq!(response[0].matches[0].classifications, vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.73".into(), source: ClassificationSource::Work }]);
}

#[test]
fn classification_lookup_prefers_direct_work_codes_to_sibling_codes() {
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
    let service = FixtureService::open(&database).unwrap();

    let response = service.lookup(vec!["9780306406157".to_owned()]).unwrap();
    let classifications = &response[0].matches[0].classifications;
    assert_eq!(classifications, &[Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "PN1993".to_owned(), source: ClassificationSource::Work },]);
}

#[test]
fn classification_lookup_prefers_exact_edition_codes_to_work_and_sibling_codes() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temporary.path(),
        &[
            serde_json::json!({
                "key":"/books/OL1M", "works":[{"key":"/works/OL10W"}], "title":"Albion's Seed",
                "isbn_13":["9780306406157"], "lc_classifications":["E169.1.F539"]
            }),
            serde_json::json!({
                "key":"/books/OL2M", "works":[{"key":"/works/OL10W"}], "title":"Albion's Seed",
                "isbn_13":["9780821338278"], "lc_classifications":["E163"]
            }),
        ],
        &[serde_json::json!({"key":"/works/OL10W", "title":"Albion's Seed", "lc_classifications":["E162"]})],
        &[],
    );
    let service = FixtureService::open(&database).unwrap();

    let response = service.lookup(vec!["9780306406157".to_owned()]).unwrap();
    assert_eq!(response[0].matches[0].classifications, vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "E169.1.F539".to_owned(), source: ClassificationSource::ExactEdition }]);
}

#[test]
fn serving_rejects_a_snapshot_outside_the_supported_schema_set() {
    let temporary = tempfile::tempdir().unwrap();
    let database = snapshot_from(temporary.path(), &[], &[], &[]);
    let connection = Connection::open(&database).unwrap();
    connection.execute("UPDATE snapshot SET schema_version=?1 WHERE singleton=1", [SCHEMA_VERSION + 1]).unwrap();
    drop(connection);

    let failure = match FixtureService::open(&database) {
        Ok(_) => panic!("an unsupported snapshot must not be served"),
        Err(failure) => failure,
    };
    assert!(failure.to_string().contains(&format!("schema {}", SCHEMA_VERSION + 1)), "unexpected rejection: {failure}");
    assert!(failure.to_string().contains(&format!("expected {SCHEMA_VERSION}")), "unexpected rejection: {failure}");
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
    register_import_inputs(&connection, &editions, &works, &authors, None).unwrap();
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

#[test]
fn loc_dump_extends_existing_answer_isbn_lookup() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL1W"}],"isbn_13":["9780306406157"],"dewey_decimal_class":["940"]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL2W"}],"isbn_13":["9780062097729"]}),
            serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL3W"}],"isbn_13":["9780062097729"]}),
        ],
        &[],
        &[],
    );
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE isbn_lcc(isbn13 INTEGER,notation TEXT,lc_record_id TEXT,PRIMARY KEY(isbn13,notation,lc_record_id)) WITHOUT ROWID;
        INSERT INTO isbn_lcc VALUES(9780306406157,'DC161','lc1'),(9780306406157,'DC161','lc2'),(9780131103627,'QA76.73.C15','lc3'),(9780062097729,'E169','lc4');",
        )
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE lc_record(lc_record_id TEXT PRIMARY KEY,metadata TEXT); CREATE TABLE lc_record_isbn(isbn13 INTEGER,lc_record_id TEXT,PRIMARY KEY(isbn13,lc_record_id)) WITHOUT ROWID;
        INSERT INTO lc_record_isbn VALUES(9780306406157,'lc1'),(9780131103627,'lc3');",
        )
        .unwrap();
    let extra = serde_json::json!({"descriptions":["An LC authority summary."],"subjects":[{"source":"library_of_congress","authority":"lcsh","components":["France","History"],"authority_uri":null}]}).to_string();
    connection.execute("INSERT INTO lc_record VALUES('lc1',?1),('lc3',?1)", [extra]).unwrap();
    drop(connection);
    let service = FixtureService::open(&database).unwrap();
    let request = vec!["0-306-40615-2".into(), "9780131103627".into(), "9780062097729".into(), "invalid".into()];
    let response = service.lookup(request.clone()).unwrap();
    assert_eq!(response[0].status, LookupStatus::Matched);
    assert_eq!(response[0].matches[0].exact_edition_ids, vec!["OL1M"]);
    let codes = &response[0].matches[0].classifications;
    assert_eq!(codes.iter().filter(|c| c.notation == "DC161").count(), 1);
    assert!(codes.iter().all(|c| c.scheme != ClassificationScheme::DeweyDecimal));
    assert_eq!(response[1].status, LookupStatus::Matched);
    assert_eq!(response[1].matches[0].open_library_work_id, None);
    assert!(response[1].matches[0].exact_edition_ids.is_empty());
    assert_eq!(response[1].matches[0].classifications[0].notation, "QA76.73.C15");
    assert_eq!(response[2].status, LookupStatus::Ambiguous);
    assert!(response[2].matches.iter().all(|m| m.classifications.is_empty()));
    assert_eq!(response[3].status, LookupStatus::InvalidIsbn);
    for enrich in [service.enrich_rich(request.clone()).unwrap()] {
        assert!(enrich[0].matches[0].classifications.iter().any(|c| c.notation == "DC161"));
        assert_eq!(enrich[1].matches[0].classifications[0].notation, "QA76.73.C15");
        assert_eq!(enrich[1].matches[0].description.as_deref(), Some("An LC authority summary."));
        assert_eq!(enrich[1].matches[0].subjects[0].source, "library_of_congress");
    }
}

pub(super) fn duplicate_work_fixture(path: &Path, candidate_title: &str, candidate_author: &str) -> PathBuf {
    snapshot_from(
        path,
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Empire of Wealth","isbn_13":["9780061847646"],"authors":[{"key":"/authors/OL1A"}]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL20W"}],"title":candidate_title,"isbn_13":["9780060093624"],"authors":[{"key":"/authors/OL2A"}],"lc_classifications":["HC103 .G673 2004"]}),
        ],
        &[
            serde_json::json!({"key":"/works/OL10W","title":"Empire of Wealth","authors":[{"author":{"key":"/authors/OL1A"}}]}),
            serde_json::json!({"key":"/works/OL20W","title":"An Empire of Wealth","authors":[{"author":{"key":"/authors/OL2A"}}]}),
        ],
        &[serde_json::json!({"key":"/authors/OL1A","name":"John Steele Gordon"}), serde_json::json!({"key":"/authors/OL2A","name":candidate_author})],
    )
}

#[test]
fn duplicate_work_recovers_lcc_preserving_identity_and_provenance_in_answer_file() {
    let temp = tempfile::tempdir().unwrap();
    let database = duplicate_work_fixture(temp.path(), "An Empire of Wealth: The Epic History of American Economic Power", "Gordon, John Steele");
    for service in [FixtureService::open(&database).unwrap()] {
        let response = service.lookup(vec!["9780061847646".into()]).unwrap();
        let matched = &response[0].matches[0];
        assert_eq!(response[0].canonical_isbn13.as_deref(), Some("9780061847646"));
        assert_eq!(matched.open_library_work_id.as_deref(), Some("OL10W"));
        assert_eq!(matched.exact_edition_ids, vec!["OL1M"]);
        assert_eq!(matched.classifications.len(), 1);
        let c = &matched.classifications[0];
        assert_eq!(c.notation, "HC103");
        assert_eq!(c.source, ClassificationSource::Work);
        assert_eq!(c.evidence[0].isbn13, "9780060093624");
        assert_eq!(c.evidence[0].open_library_work_id, "OL20W");
        for enriched in [service.enrich_rich(vec!["9780061847646".into()]).unwrap()] {
            assert_eq!(enriched[0].matches[0].classifications, matched.classifications);
            let merged = metadata_contract::merge_rich_work_matches(&enriched[0].matches, |_, _| Vec::new());
            assert_eq!(merged.classifications, matched.classifications);
        }
    }
}

#[test]
fn duplicate_work_rejects_different_authors_adaptations_and_volumes() {
    for (title, author) in [
        ("An Empire of Wealth", "John Smith"),
        ("An Empire of Wealth: Study Guide", "John Steele Gordon"),
        ("An Empire of Wealth (Abridged)", "John Steele Gordon"),
        ("An Empire of Wealth II", "John Steele Gordon"),
        ("An Empire of Wealth: Volume 2", "John Steele Gordon"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let database = duplicate_work_fixture(temp.path(), title, author);
        let response = FixtureService::open(database).unwrap().lookup(vec!["9780061847646".into()]).unwrap();
        assert!(response[0].matches[0].classifications.is_empty(), "{title} / {author}");
    }
}

#[test]
fn duplicate_work_collects_differing_codes_and_keeps_existing_lcc() {
    let temp = tempfile::tempdir().unwrap();
    let database = duplicate_work_fixture(temp.path(), "An Empire of Wealth", "John Steele Gordon");
    let c = Connection::open(&database).unwrap();
    c.execute("INSERT INTO edition_classification(edition_id,scheme,notation) VALUES(2,?1,'D1')", [LCC_SCHEME]).unwrap();
    c.execute_batch("INSERT INTO edition VALUES(3,30); INSERT INTO edition_isbn VALUES(9780306406157,3); INSERT INTO edition_bibliography VALUES(3,'Empire of Wealth','empire of wealth',2005); INSERT INTO edition_author VALUES(3,0,2);")
        .unwrap();
    c.execute("INSERT INTO edition_classification(edition_id,scheme,notation) VALUES(3,?1,'HC103')", [LCC_SCHEME]).unwrap();
    let request = || vec!["9780061847646".into()];
    let recovered = FixtureService::open(&database).unwrap().lookup(request()).unwrap();
    let codes = &recovered[0].matches[0].classifications;
    assert!(codes.iter().any(|c| c.notation == "D1"));
    assert!(codes.iter().any(|c| c.notation == "HC103"));
    assert!(codes.iter().all(|c| !c.evidence.is_empty()));
    c.execute("INSERT INTO edition_classification(edition_id,scheme,notation) VALUES(1,?1,'E1')", [LCC_SCHEME]).unwrap();
    let response = FixtureService::open(database).unwrap().lookup(request()).unwrap();
    assert_eq!(response[0].matches[0].classifications[0].notation, "E1");
    assert!(response[0].matches[0].classifications[0].evidence.is_empty());
}

#[test]
fn duplicate_work_does_not_pick_an_arbitrary_match_from_truncated_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let database = duplicate_work_fixture(temp.path(), "An Empire of Wealth", "John Steele Gordon");
    let c = Connection::open(&database).unwrap();
    for id in 100..230 {
        c.execute("INSERT INTO edition VALUES(?1,?1)", [id]).unwrap();
        c.execute("INSERT INTO edition_bibliography VALUES(?1,'Empire of Wealth','empire of wealth',2000)", [id]).unwrap();
        c.execute("INSERT INTO edition_author VALUES(?1,0,2)", [id]).unwrap();
    }
    let result = FixtureService::open(database).unwrap().lookup(vec!["9780061847646".into()]).unwrap();
    assert!(result[0].matches[0].classifications.is_empty());
}

#[test]
fn duplicate_work_filters_unrelated_authors_before_candidate_limit() {
    let temp = tempfile::tempdir().unwrap();
    let database = duplicate_work_fixture(temp.path(), "An Empire of Wealth", "John Steele Gordon");
    let c = Connection::open(&database).unwrap();
    c.execute("INSERT INTO author VALUES(99,'Unrelated Writer')", []).unwrap();
    for id in 100..400 {
        c.execute("INSERT INTO edition VALUES(?1,?1)", [id]).unwrap();
        c.execute("INSERT INTO edition_bibliography VALUES(?1,'Empire of Wealth','empire of wealth',2000)", [id]).unwrap();
        c.execute("INSERT INTO edition_author VALUES(?1,0,99)", [id]).unwrap();
    }
    let result = FixtureService::open(database).unwrap().lookup(vec!["9780061847646".into()]).unwrap();
    assert_eq!(result[0].matches[0].classifications[0].notation, "HC103");
}

#[test]
fn single_classified_sibling_is_used_until_another_sibling_has_codes() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780306406157"]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780821338278"],"lc_classifications":["HE2751 .A1","HE2751.A2"],"dewey_decimal_class":["385"]}),
            serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9783161484100"]}),
        ],
        &[serde_json::json!({"key":"/works/OL10W","title":"Example"})],
        &[],
    );
    for conflicting in [false, true] {
        if conflicting {
            Connection::open(&database).unwrap().execute("INSERT INTO edition_classification VALUES(3,?1,'D1')", [LCC_SCHEME]).unwrap();
        }
        for service in [FixtureService::open(&database).unwrap()] {
            let response = service.lookup(vec!["9780306406157".into()]).unwrap();
            let codes = &response[0].matches[0].classifications;
            if conflicting {
                assert!(codes.is_empty());
            } else {
                assert_eq!(codes.len(), 1);
                assert!(codes.iter().any(|c| c.notation == "HE2751" && c.source == ClassificationSource::Work));
                assert!(!codes.iter().any(|c| c.notation == "385"));
            }
        }
    }
}


#[test]
fn wikidata_ingestion_fills_missing_schemes_and_links_authors_without_overwriting_existing_data() {
    let temp = tempfile::tempdir().unwrap();
    let base = snapshot_from(
        temp.path(),
        &[serde_json::json!({"key":"/books/OL1M","title":"Existing book","isbn_13":["9780306406157"],"lc_classifications":["QA1"],"authors":[{"key":"/authors/OL1A"}]})],
        &[],
        &[serde_json::json!({"key":"/authors/OL1A","name":"Alex Smith"})],
    );
    let input = temp.path().join("source.sqlite");
    let c = Connection::open(&input).unwrap();
    c.execute_batch(include_str!("../../wikidata-author-schema.sql")).unwrap();
    c.execute_batch(
        "INSERT INTO state VALUES('status','complete'),('schema_version','1'),('ol_dump_date','2026-08-01');
        INSERT INTO isbn VALUES('9780306406157','Q10');
        INSERT INTO profile VALUES('Q1','Alexander Smith','[\"A. Smith\"]',1900,1980,'[{\"authority\":\"openlibrary\",\"value\":\"OL1A\"}]','Portrait.jpg',NULL);
        INSERT INTO credit VALUES('Q10','Q1',0);
        INSERT INTO isbn_author VALUES('9780306406157','Q10','Q10','Q1','Q10');
        INSERT INTO classification VALUES('Q10','lcc','QA1','P8360',0,'Q10'),('Q10','lcc','QA2','P8360',0,'Q10'),('Q10','ddc','510','P1036',0,'Q10');",
    )
    .unwrap();
    drop(c);
    let target = temp.path().join("authorities.sqlite");
    std::fs::copy(&base, &target).unwrap();
    ingest_wikidata_authorities(&FixtureService::open(&base).unwrap(), &input, &target).unwrap();
    std::fs::remove_file(&input).unwrap();
    let service = FixtureService::open(&target).unwrap();
    let result = service.enrich_rich(vec!["9780306406157".into()]).unwrap().remove(0);
    assert_eq!(result.matches[0].authors[0].name.as_deref(), Some("Alex Smith"));
    assert!(result.matches[0].authors[0].identifiers.iter().any(|i| i.authority == "wikidata" && i.value == "Q1"));
    assert!(result.matches[0].classifications.iter().any(|c| c.notation == "QA1"));
    assert!(!result.matches[0].classifications.iter().any(|c| c.notation == "QA2"));
    let db = Connection::open(&target).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM isbn_classification WHERE isbn13=9780306406157 AND scheme=2 AND notation='QA1'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM isbn_classification WHERE isbn13=9780306406157 AND scheme=1 AND notation='510'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(db.query_row("SELECT COUNT(*) FROM title_author_classification WHERE normalized_title='existing book' AND scheme=1 AND notation='510'", [], |r| r.get::<_, i64>(0)).unwrap() > 0);
    assert_eq!(db.query_row("SELECT birth_year FROM author WHERE author_id=1", [], |r| r.get::<_, i64>(0)).unwrap(), 1900);
    drop(db);
    drop(service);
    let c = Connection::open(&target).unwrap();
    c.execute("UPDATE wikidata_ingestion SET status='building'", []).unwrap();
    assert!(FixtureService::open(&target).is_err());
}


#[test]
fn wikidata_isbn_fast_path_defers_unrelated_authors_and_incomplete_bibliography() {
    let temp = tempfile::tempdir().unwrap();
    let base = snapshot_from(temp.path(), &[], &[], &[]);
    let input = temp.path().join("input.sqlite");
    let c = Connection::open(&input).unwrap();
    c.execute_batch(include_str!("../../wikidata-author-schema.sql")).unwrap();
    c.execute_batch(
        "INSERT INTO state VALUES('status','complete'),('schema_version','1'),('ol_dump_date','2026-08-01'),('bibliography_status','building');
        INSERT INTO profile VALUES('Q10','Fast book','[]',NULL,NULL,'[]',NULL,NULL),('Q1','Jane Smith','[]',NULL,NULL,'[]',NULL,NULL),('Q2','Other Author','[]',NULL,NULL,'[]',NULL,NULL);
        INSERT INTO isbn VALUES('9780306406157','Q10');
        INSERT INTO credit VALUES('Q10','Q1',0),('Q20','Q2',0);
        INSERT INTO isbn_author VALUES('9780306406157','Q10','Q10','Q1','Q10');
        INSERT INTO classification VALUES('Q10','lcc','BC71','P8360',0,'Q10');",
    )
    .unwrap();
    let target = temp.path().join("target.sqlite");
    std::fs::copy(&base, &target).unwrap();
    let service = FixtureService::open(base).unwrap();
    assert!(ingest_wikidata_authorities(&service, &input, &target).is_err());
    c.execute("INSERT INTO state VALUES('ingestion_scope','isbn')", []).unwrap();
    drop(c);
    ingest_wikidata_authorities(&service, &input, &target).unwrap();
    let response = FixtureService::open(&target).unwrap().enrich_rich(vec!["9780306406157".into()]).unwrap();
    assert_eq!(response[0].matches[0].authors[0].identity_key(), "wikidata:Q1");
    assert_eq!(response[0].matches[0].classifications[0].notation, "BC71");
    let c = Connection::open(target).unwrap();
    assert_eq!(c.query_row("SELECT count(*) FROM author WHERE author_id=-2", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}


struct FixtureService {
    resolver: Resolver,
    server: MetadataService,
}
impl std::ops::Deref for FixtureService {
    type Target = Resolver;
    fn deref(&self) -> &Resolver {
        &self.resolver
    }
}
impl FixtureService {
    fn open(path: impl Into<PathBuf>) -> Result<Self, MetadataError> {
        Self::open_with_descriptions(path, None::<PathBuf>)
    }
    fn open_with_descriptions(path: impl Into<PathBuf>, descriptions: Option<impl Into<PathBuf>>) -> Result<Self, MetadataError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = path.into();
        let descriptions = descriptions.map(Into::into);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let answers = path.with_extension(format!("answers-{n}.sqlite"));
        let demand = path.with_extension(format!("demand-{n}.sqlite"));
        build_answers(&path, None, None, descriptions.as_deref(), &answers)?;
        Ok(Self { resolver: Resolver::open(&path, descriptions.as_deref())?, server: MetadataService::open(&answers, &demand)? })
    }
    fn enrich_rich(&self, isbns: Vec<String>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
        let expected = self.resolver.enrich_rich(isbns.clone())?;
        let results = self.server.enrich_rich(isbns)?;
        assert_eq!(serde_json::to_value(&results).unwrap(), serde_json::to_value(&expected).unwrap(), "built ISBN answers differ from the resolver");
        Ok(results)
    }
    fn lookup(&self, isbns: Vec<String>) -> Result<Vec<IsbnClassificationResult>, MetadataError> {
        let expected = self.resolver.lookup(isbns.clone())?;
        let results = self
            .server
            .enrich_rich(isbns)?
            .into_iter()
            .map(|r| IsbnClassificationResult {
                requested_isbn: r.requested_isbn,
                canonical_isbn13: r.canonical_isbn13,
                status: r.status,
                matches: r.matches.into_iter().map(|m| WorkClassificationMatch { open_library_work_id: m.open_library_work_id, exact_edition_ids: m.exact_edition_ids, classifications: m.classifications }).collect(),
            })
            .collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(&results).unwrap(), serde_json::to_value(&expected).unwrap(), "built classification answers differ from the resolver");
        Ok(results)
    }
}
