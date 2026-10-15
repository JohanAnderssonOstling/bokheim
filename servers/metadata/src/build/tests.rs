use crate::build::import::core::{completed_phase, configure_import, create_schema, import_editions, register_import_inputs};
use crate::*;
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
fn classification_lookup_keeps_sibling_domains_when_work_evidence_is_incomplete() {
    let temporary = tempfile::tempdir().unwrap();
    let editions = [
        serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780306406157"]}),
        serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780821338278"],"lc_classifications":["QA76.6.A1"],"dewey_decimal_class":["005.1"]}),
        serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9783161484100"],"lc_classifications":["QA76.6.B2"],"dewey_decimal_class":["005.1"]}),
        serde_json::json!({"key":"/books/OL4M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9781861972712"],"lc_classifications":["PN1993.A1"],"dewey_decimal_class":["791.43"]}),
        serde_json::json!({"key":"/books/OL5M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780140328721"],"lc_classifications":["PN1993.B2"],"dewey_decimal_class":["791.43"]}),
    ];
    let works = [serde_json::json!({"key":"/works/OL10W","title":"Example","lc_classifications":["QA76.6"]})];
    let database = snapshot_from(temporary.path(), &editions, &works, &[]);
    let service = FixtureService::open(&database).unwrap();

    let response = service.lookup(vec!["9780306406157".to_owned()]).unwrap();
    let classifications = &response[0].matches[0].classifications;
    assert_eq!(classifications.iter().map(|code| code.notation.as_str()).collect::<Vec<_>>(), ["PN1993", "QA76.6"]);
    assert!(classifications.iter().all(|code| code.source == ClassificationSource::Work));
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
fn work_subtree_policy_agrees_across_single_batch_published_and_title_candidates() {
    for (work_codes, records, expected) in [
        (vec!["PR4030"], (0..20).map(|id| if id < 18 { vec!["PR4034"] } else { vec!["PE1128"] }).collect::<Vec<_>>(), vec!["PR4030"]),
        (vec!["PE1128"], (0..20).map(|id| if id < 18 { vec!["PR4750"] } else { vec!["PE1128"] }).collect(), vec!["PR4750"]),
        (vec!["AC7"], (0..10).map(|id| if id < 2 { vec!["HB161", "AC7"] } else { vec!["HB161"] }).collect(), vec!["AC7", "HB161"]),
        (vec!["B3376"], (0..26).map(|id| if id < 9 { vec!["B3376", "BC135"] } else { vec!["B3376"] }).collect(), vec!["B3376", "BC135"]),
        (vec![], vec![vec!["PR4030", "PR4034"], vec!["PR4030", "PR4034"]], vec!["PR4030", "PR4034"]),
        (vec!["PE1128"], vec![vec!["PR4034"]], vec!["PE1128", "PR4034"]),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut editions = vec![serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Example","isbn_13":["9780306406157"],"authors":[{"key":"/authors/OL1A"}]})];
        editions.extend(records.iter().enumerate().map(|(id, codes)| serde_json::json!({"key":format!("/books/OL{}M",id+2),"works":[{"key":"/works/OL10W"}],"isbn_13":["9780821338278"],"lc_classifications":codes})));
        let database = snapshot_from(temp.path(), &editions, &[serde_json::json!({"key":"/works/OL10W","title":"Example","lc_classifications":work_codes})], &[serde_json::json!({"key":"/authors/OL1A","name":"Example Author"})]);
        let resolver = Resolver::open(&database, None).unwrap();
        let c = resolver.connection().unwrap();
        let requested = vec!["9780306406157".to_owned()];
        let single = lookup_one(&c, requested[0].clone()).unwrap();
        let batch = crate::build::batch::lookup_many(&c, requested.clone()).unwrap();
        assert_eq!(single.matches[0].classifications.iter().map(|code| code.notation.as_str()).collect::<Vec<_>>(), expected, "work {work_codes:?}");
        assert_eq!(serde_json::to_value(&single).unwrap(), serde_json::to_value(&batch[0]).unwrap());
        let service = FixtureService::open(&database).unwrap();
        let published = service.lookup(requested).unwrap();
        assert_eq!(serde_json::to_value(&single).unwrap(), serde_json::to_value(&published[0]).unwrap());
        let answers = Connection::open(&service.server.state.database).unwrap();
        let codes: Vec<u8> = answers.query_row("SELECT classifications FROM edition_candidate WHERE edition_id=1", [], |r| r.get(0)).unwrap();
        let codes: Vec<Classification> = serde_json::from_slice(&codes).unwrap();
        assert_eq!(codes.iter().map(|code| code.notation.as_str()).collect::<Vec<_>>(), expected);
    }
}

#[test]
fn previous_classification_recipe_uses_targeted_migration() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(temp.path(), &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]})], &[], &[]);
    let base = temp.path().join("base.sqlite");
    let next = temp.path().join("next.sqlite");
    build_answers(&database, None, None, None, &base).unwrap();
    Connection::open(&base).unwrap().execute("UPDATE answer_build SET recipe_version=recipe_version-1", []).unwrap();
    let report = build_answers_incremental(&database, None, None, None, None, &base, &next).unwrap();
    assert_eq!(report.mode, "reclassified");
    assert_eq!(report.resolved_rows, 0);
    assert_eq!(report.reused_rows, 1);
    assert_eq!(report.reclassified_rows, 0);
}

#[test]
fn in_place_reclassification_preserves_rich_fields_updates_search_and_supports_legacy_files() {
    for legacy in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = snapshot_from(temp.path(), &[
            serde_json::json!({"key":"/books/OL1M","title":"Emma","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"]}),
            serde_json::json!({"key":"/books/OL2M","title":"Emma","works":[{"key":"/works/OL10W"}],"lc_classifications":["PR4034 .E5 1994"]}),
            serde_json::json!({"key":"/books/OL3M","title":"Emma","works":[{"key":"/works/OL10W"}],"lc_classifications":["PR4034 .E5 2002"]}),
            serde_json::json!({"key":"/books/OL4M","title":"Unchanged","isbn_13":["9780131103627"],"lc_classifications":["QA76"]}),
            serde_json::json!({"key":"/books/OL5M","title":"Sparse work","works":[{"key":"/works/OL20W"}],"isbn_13":["9781404309982"]}),
            serde_json::json!({"key":"/books/OL6M","title":"Bare code","works":[{"key":"/works/OL30W"}],"isbn_13":["9781324096900"]}),
            serde_json::json!({"key":"/books/OL7M","title":"Ambiguous","works":[{"key":"/works/OL10W"}],"isbn_13":["9780141439518"]}),
            serde_json::json!({"key":"/books/OL8M","title":"Ambiguous","works":[{"key":"/works/OL20W"}],"isbn_13":["9780141439518"]}),
            serde_json::json!({"key":"/books/OL9M","title":"Exact literature","works":[{"key":"/works/OL50W"}],"isbn_13":["9780000000002"],"lc_classifications":["PR4034"]}),
            serde_json::json!({"key":"/books/OL10M","title":"Exact economics","works":[{"key":"/works/OL50W"}],"isbn_13":["9780000000019"],"lc_classifications":["HB161"]}),
            serde_json::json!({"key":"/books/OL11M","title":"Unclassified sibling","works":[{"key":"/works/OL50W"}],"isbn_13":["9780000000026"]}),
        ], &[
            serde_json::json!({"key":"/works/OL10W","title":"Emma","lc_classifications":["PR4030"],"description":"Shared description","subjects":["Fiction"]}),
            serde_json::json!({"key":"/works/OL20W","title":"Sparse work","lc_classifications":["HB161"]}),
            serde_json::json!({"key":"/works/OL30W","title":"Bare code","lc_classifications":["PR"]}),
            serde_json::json!({"key":"/works/OL50W","title":"Unchanged cross-root work"}),
        ], &[]);
        let base = temp.path().join("answers.sqlite");
        let fresh = temp.path().join("fresh.sqlite");
        build_answers(&source, None, None, None, &base).unwrap();
        build_answers(&source, None, None, None, &fresh).unwrap();
        let c = Connection::open(&base).unwrap();
        let unchanged: Vec<u8> = c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=9780131103627", [], |r| r.get(0)).unwrap();
        for isbn in [9780306406157_i64, 9781404309982, 9781324096900, 9780141439518] {
            let json: Vec<u8> = c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get(0)).unwrap();
            let mut answer: schema::StoredAnswer = serde_json::from_slice(&json).unwrap();
            for m in &mut answer.result.matches {
                m.classifications.retain(|c| c.scheme != ClassificationScheme::LibraryOfCongress);
                if isbn != 9781404309982 {
                    m.classifications.push(Classification { scheme: ClassificationScheme::LibraryOfCongress, notation:"PE1128".into(), source:ClassificationSource::Work, evidence:vec![] });
                }
            }
            c.execute("UPDATE isbn_answer SET answer=?1 WHERE isbn13=?2", params![serde_json::to_vec(&answer).unwrap(),isbn]).unwrap();
        }
        let old_codes=serde_json::to_vec(&vec![Classification{scheme:ClassificationScheme::LibraryOfCongress,notation:"PE1128".into(),source:ClassificationSource::Work,evidence:vec![]}]).unwrap();
        c.execute("UPDATE edition_candidate SET classifications=?1 WHERE edition_id=1",[old_codes]).unwrap();
        c.execute_batch("DELETE FROM edition_title_key WHERE edition_id IN (5,6,7,8); DELETE FROM edition_candidate WHERE edition_id IN (5,6,7,8); DELETE FROM answer_classification; DELETE FROM answer_work_lcc; DELETE FROM wanted;
            UPDATE answer_build SET recipe_version=2,json_bytes=(SELECT sum(length(answer)) FROM isbn_answer);").unwrap();
        if legacy {
            c.execute_batch("DROP TABLE answer_build; DROP TABLE answer_classification; DROP TABLE answer_work_lcc; DROP TABLE answer_provider; DROP TABLE answer_demand;").unwrap();
        }
        drop(c);
        // A reader already connected before the write sees its committed data.
        let server = MetadataService::open(&base, &temp.path().join("serve-demand.sqlite")).unwrap();
        let report = reclassify_answers(&source, None, None, None, None, &base).unwrap();
        assert_eq!(report.mode, "reclassified");
        assert_eq!(report.reclassified_rows, 4);
        assert_eq!(report.resolved_rows, 0);
        assert_eq!(report.isbn_rows, 8);
        let full = MetadataService::open(&fresh, &temp.path().join("full-demand.sqlite")).unwrap();
        let isbns = ["9780306406157","9780131103627","9781404309982","9781324096900","9780141439518"].into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(server.enrich_rich(isbns.clone()).unwrap()).unwrap(), serde_json::to_value(full.enrich_rich(isbns).unwrap()).unwrap());
        let c = Connection::open(&base).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM wanted WHERE work_id=50",[],|r|r.get::<_,i64>(0)).unwrap(),0,"unchanged work queues must remain pruned");
        assert_eq!(c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=9780131103627", [], |r| r.get::<_,Vec<u8>>(0)).unwrap(), unchanged);
        for table in ["edition_candidate","edition_title_key","wanted"] {
            let a: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            let b: i64 = Connection::open(&fresh).unwrap().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(a,b,"{table}");
        }
        let totals: (i64,i64) = c.query_row("SELECT count(*),sum(length(answer)) FROM isbn_answer", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(c.query_row("SELECT isbn_rows,json_bytes FROM answer_build", [], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap(),totals);
        let queries = ["Emma","Sparse work","Bare code"].into_iter().map(|title| EditionIdentityQuery { query_id:title.into(),title:title.into(),authors:vec![],author_surname:None,publishers:vec![],book_year:None }).collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(server.resolve_editions(queries.clone()).unwrap()).unwrap(),serde_json::to_value(full.resolve_editions(queries).unwrap()).unwrap());
        let next = temp.path().join("next.sqlite");
        assert_eq!(build_answers_incremental(&source,None,None,None,None,&base,&next).unwrap().mode,"reused");
    }
}

#[test]
fn failed_in_place_reclassification_rolls_back_answers_indexes_and_bookkeeping() {
    let temp = tempfile::tempdir().unwrap();
    let source = snapshot_from(temp.path(), &[
        serde_json::json!({"key":"/books/OL1M","title":"First","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"]}),
        serde_json::json!({"key":"/books/OL2M","title":"Second","works":[{"key":"/works/OL20W"}],"isbn_13":["9781404309982"]}),
    ], &[
        serde_json::json!({"key":"/works/OL10W","lc_classifications":["PR4030"]}),
        serde_json::json!({"key":"/works/OL20W","lc_classifications":["HB161"]}),
    ], &[]);
    let base = temp.path().join("answers.sqlite");
    build_answers(&source,None,None,None,&base).unwrap();
    let c = Connection::open(&base).unwrap();
    let mut q = c.prepare("SELECT isbn13,answer FROM isbn_answer ORDER BY isbn13").unwrap();
    let before = q.query_map([], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    drop(q);
    let mut first: schema::StoredAnswer = serde_json::from_slice(&before[0].1).unwrap();
    first.result.matches[0].classifications[0].notation="A14".into();
    c.execute("UPDATE isbn_answer SET answer=?1 WHERE isbn13=?2",params![serde_json::to_vec(&first).unwrap(),before[0].0]).unwrap();
    c.execute("UPDATE isbn_answer SET answer=?1 WHERE isbn13=?2",params![b"invalid JSON".as_slice(),before[1].0]).unwrap();
    c.execute_batch("DROP TABLE answer_build; DROP TABLE answer_classification; DROP TABLE answer_work_lcc; DROP TABLE answer_provider; DROP TABLE answer_demand;").unwrap();
    let before_first: Vec<u8> = c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1",[before[0].0],|r|r.get(0)).unwrap();
    assert!(reclassify_answers(&source,None,None,None,None,&base).is_err());
    assert_eq!(c.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1",[before[0].0],|r|r.get::<_,Vec<u8>>(0)).unwrap(),before_first);
    assert!(!crate::build::has_table(&c,"answer_build").unwrap());
    assert_eq!(c.query_row("SELECT count(*) FROM edition_candidate",[],|r|r.get::<_,i64>(0)).unwrap(),2);
}

#[test]
fn legacy_in_place_update_applies_provider_and_demand_changes_and_tracks_later_deletions() {
    let temp = tempfile::tempdir().unwrap();
    let source = snapshot_from(temp.path(), &[
        serde_json::json!({"key":"/books/OL1M","title":"Shared","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"]}),
        serde_json::json!({"key":"/books/OL2M","title":"Shared","works":[{"key":"/works/OL10W"}],"isbn_13":["9780131103627"]}),
    ], &[serde_json::json!({"key":"/works/OL10W","description":"Preserved description"})], &[]);
    let base = temp.path().join("answers.sqlite");
    build_answers(&source,None,None,None,&base).unwrap();
    Connection::open(&base).unwrap().execute_batch("DROP TABLE answer_build; DROP TABLE answer_classification; DROP TABLE answer_work_lcc; DROP TABLE answer_provider; DROP TABLE answer_demand;").unwrap();
    let provider_path = temp.path().join("provider.sqlite");
    let provider = crate::crawl::open_provider(&provider_path).unwrap();
    let codes = serde_json::to_vec(&vec![Classification { scheme:ClassificationScheme::LibraryOfCongress,notation:"QA76".into(),source:ClassificationSource::ExactEdition,evidence:vec![] }]).unwrap();
    for isbn in [9780306406157_i64,9780000000019] {
        provider.execute("INSERT INTO provider_result VALUES('loc',?1,1,'matched',?2,?3)",params![isbn,codes,b"[]".as_slice()]).unwrap();
    }
    let demand_path = temp.path().join("demand.sqlite");
    Connection::open(&demand_path).unwrap().execute_batch("CREATE TABLE demand(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL); INSERT INTO demand VALUES(9780000000002,5);").unwrap();
    for iteration in 0..2 {
        let report = reclassify_answers(&source,Some(&provider_path),Some(&demand_path),None,None,&base).unwrap();
        assert_eq!(report.mode,"reclassified");
        assert!(report.resolved_rows>0);
        let fresh = temp.path().join(format!("fresh-{iteration}.sqlite"));
        build_answers(&source,Some(&provider_path),Some(&demand_path),None,&fresh).unwrap();
        assert_answer_tables_equal(&base,&fresh);
        provider.execute_batch("DELETE FROM provider_result").unwrap();
    }
}

#[test]
fn in_place_reclassification_rebuilds_candidates_across_work_page_boundaries() {
    let temp=tempfile::tempdir().unwrap();
    let mut editions=Vec::new();
    let mut works=Vec::new();
    for id in 1..=501_i64 {
        let body=978000000000+id;
        let sum=(0..12).map(|p|body/10_i64.pow(11-p)%10*if p%2==0 {1}else{3}).sum::<i64>();
        let isbn=body*10+(10-sum%10)%10;
        editions.push(serde_json::json!({"key":format!("/books/OL{id}M"),"title":format!("Book {id}"),"works":[{"key":format!("/works/OL{id}W")}],"isbn_13":[format!("{isbn}")]}));
        works.push(serde_json::json!({"key":format!("/works/OL{id}W"),"title":format!("Book {id}"),"lc_classifications":["QA76"]}));
    }
    let source=snapshot_from(temp.path(),&editions,&works,&[]);
    let answers=temp.path().join("answers.sqlite");
    build_answers(&source,None,None,None,&answers).unwrap();
    Connection::open(&answers).unwrap().execute_batch("DELETE FROM edition_title_key WHERE edition_id IN (1,501); DELETE FROM edition_candidate WHERE edition_id IN (1,501);").unwrap();
    let report=reclassify_answers(&source,None,None,None,None,&answers).unwrap();
    assert_eq!(report.reclassified_rows,0);
    assert_eq!(report.reclassified_editions,2);
    assert_eq!(Connection::open(&answers).unwrap().query_row("SELECT count(*) FROM edition_candidate",[],|r|r.get::<_,i64>(0)).unwrap(),501);
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

fn separate_work_fixture(path: &Path, candidate_title: &str, candidate_author: &str) -> PathBuf {
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
fn matching_title_and_author_on_another_work_do_not_supply_lcc() {
    let temp = tempfile::tempdir().unwrap();
    let database = separate_work_fixture(temp.path(), "An Empire of Wealth: The Epic History of American Economic Power", "Gordon, John Steele");
    let service = FixtureService::open(&database).unwrap();
    let response = service.lookup(vec!["9780061847646".into(), "9780060093624".into()]).unwrap();
    let matched = &response[0].matches[0];
    assert_eq!(response[0].status, LookupStatus::Matched);
    assert_eq!(response[0].canonical_isbn13.as_deref(), Some("9780061847646"));
    assert_eq!(matched.open_library_work_id.as_deref(), Some("OL10W"));
    assert_eq!(matched.exact_edition_ids, vec!["OL1M"]);
    assert!(matched.classifications.is_empty());
    assert_eq!(response[1].matches[0].classifications[0].notation, "HC103 .G673 2004");
    let enriched = service.enrich_rich(vec!["9780061847646".into()]).unwrap();
    assert!(enriched[0].matches[0].classifications.is_empty());
    assert_eq!(enriched[0].matches[0].authors[0].name.as_deref(), Some("John Steele Gordon"));
    let answers = Connection::open(&service.server.state.database).unwrap();
    assert_eq!(answers.query_row("SELECT count(*) FROM wanted WHERE isbn13=9780061847646", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn exact_edition_lcc_is_preserved_when_another_work_matches_title_and_author() {
    let temp = tempfile::tempdir().unwrap();
    let database = separate_work_fixture(temp.path(), "An Empire of Wealth", "John Steele Gordon");
    let c = Connection::open(&database).unwrap();
    c.execute("INSERT INTO edition_classification(edition_id,scheme,notation) VALUES(1,?1,'E1')", [LCC_SCHEME]).unwrap();
    drop(c);
    let response = FixtureService::open(database).unwrap().lookup(vec!["9780061847646".into()]).unwrap();
    let classifications = &response[0].matches[0].classifications;
    assert_eq!(classifications.len(), 1);
    assert_eq!(classifications[0].notation, "E1");
    assert_eq!(classifications[0].source, ClassificationSource::ExactEdition);
    assert!(classifications[0].evidence.is_empty());
}

#[test]
fn batched_lookup_preserves_invalid_repeated_ambiguous_and_authority_only_requests() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"],"lc_classifications":["QA76.73.C15"]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL20W"}],"isbn_13":["9780306406157"],"lc_classifications":["D1"]}),
            serde_json::json!({"key":"/books/OL3M","isbn_13":["9780131103627"],"lc_classifications":["QA76"]}),
        ],
        &[],
        &[],
    );
    let c = Connection::open(&database).unwrap();
    c.execute_batch("INSERT INTO edition VALUES(-100,NULL); INSERT INTO edition_isbn VALUES(9780131103627,-100); CREATE TABLE isbn_lcc(isbn13 INTEGER,notation TEXT,lc_record_id TEXT); INSERT INTO isbn_lcc VALUES(9780821338278,'HE2751','lc1'),(9780306406157,'E1','lc2'); INSERT INTO isbn_classification VALUES(9780821338278,1,'385');").unwrap();
    drop(c);
    let resolver = Resolver::open(&database, None).unwrap();
    let c = resolver.connection().unwrap();
    let requested = ["invalid", "9780306406157", "9780131103627", "0-306-40615-2", "9780821338278", "9780061847646", "9780306406157"].map(str::to_owned).to_vec();
    let expected = requested.iter().map(|isbn| lookup_one(&c, isbn.clone()).unwrap()).collect::<Vec<_>>();
    let actual = crate::build::batch::lookup_many(&c, requested).unwrap();
    assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap());
}

#[test]
fn zero_isbn_in_snapshot_is_built_before_edition_indexing() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[serde_json::json!({"key":"/books/OL1M","title":"Unclassified","isbn_13":["0000000000000"]}), serde_json::json!({"key":"/books/OL2M","title":"Classified","isbn_13":["9780306406157"],"lc_classifications":["QA76"]})],
        &[],
        &[],
    );
    let answers = temp.path().join("answers.sqlite");
    let report = build_answers(&database, None, None, None, &answers).unwrap();
    assert_eq!(report.isbn_rows, 2);
    let resolver = Resolver::open(&database, None).unwrap();
    let server = MetadataService::open(&answers, &temp.path().join("demand.sqlite")).unwrap();
    let requested = vec!["0000000000000".to_owned(), "9780306406157".to_owned()];
    assert_eq!(serde_json::to_value(server.enrich_rich(requested.clone()).unwrap()).unwrap(), serde_json::to_value(resolver.enrich_rich(requested).unwrap()).unwrap());
}

#[test]
fn answers_across_query_and_commit_boundaries_match_the_original_resolver() {
    let temp = tempfile::tempdir().unwrap();
    let mut isbns = Vec::new();
    let editions = (1..=4200)
        .map(|id| {
            let body = 978_000_000_000_i64 + id;
            let sum = (0..12).map(|position| body / 10_i64.pow(11 - position) % 10 * if position % 2 == 0 { 1 } else { 3 }).sum::<i64>();
            let isbn = format!("{}", body * 10 + (10 - sum % 10) % 10);
            isbns.push(isbn.clone());
            let mut edition = serde_json::json!({"key":format!("/books/OL{id}M"),"works":[{"key":format!("/works/OL{}W", (id+1)/2)}],"title":"Example","isbn_13":[isbn],"authors":[{"key":"/authors/OL1A"}]});
            if id % 3 == 0 {
                edition["lc_classifications"] = serde_json::json!(["QA76.73.C15", "QA76.73.C15 .A1"]);
            }
            edition
        })
        .collect::<Vec<_>>();
    let works = (1..=2100).map(|id| serde_json::json!({"key":format!("/works/OL{id}W"),"title":"Example","description":format!("Description {id}")})).collect::<Vec<_>>();
    let database = snapshot_from(temp.path(), &editions, &works, &[serde_json::json!({"key":"/authors/OL1A","name":"Example Author"})]);
    let answers = temp.path().join("answers.sqlite");
    let report = build_answers(&database, None, None, None, &answers).unwrap();
    assert_eq!(report.isbn_rows, 4200);
    let resolver = Resolver::open(&database, None).unwrap();
    let server = MetadataService::open(&answers, &temp.path().join("demand.sqlite")).unwrap();
    for requested in isbns.chunks(256) {
        let expected = resolver.enrich_rich(requested.to_vec()).unwrap();
        let actual = server.enrich_rich(requested.to_vec()).unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap());
    }
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

#[test]
fn edition_index_keeps_provider_fallbacks_and_title_author_authority_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Provider Title","isbn_13":["9780306406157"],"authors":[{"key":"/authors/OL1A"}]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL20W"}],"title":"Authority Title","isbn_13":["9780131103627"],"authors":[{"key":"/authors/OL1A"}]}),
            serde_json::json!({"key":"/books/OL3M","title":"No Evidence","isbn_13":["9780821338278"],"authors":[{"key":"/authors/OL1A"}]}),
        ],
        &[],
        &[serde_json::json!({"key":"/authors/OL1A","name":"Jane Smith"})],
    );
    let provider = temp.path().join("provider.sqlite");
    let c = crate::crawl::open_provider(&provider).unwrap();
    let codes = vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76".into(), source: ClassificationSource::ExactEdition, evidence: vec![] }];
    c.execute("INSERT INTO provider_result VALUES('loc',9780306406157,1,'matched',?1,?2)", params![serde_json::to_vec(&codes).unwrap(), b"[]".as_slice()]).unwrap();
    drop(c);
    for authority in [false, true] {
        if authority {
            Connection::open(&database).unwrap().execute("INSERT INTO title_author_classification VALUES(?1,'','Jane Smith',0,'',2,'E1')", [metadata_contract::matching::bibliographic_title_match_key("Authority Title")]).unwrap();
        }
        let answers = temp.path().join(format!("answers-{authority}.sqlite"));
        build_answers(&database, Some(&provider), None, None, &answers).unwrap();
        let c = Connection::open(&answers).unwrap();
        let ids = c.prepare("SELECT edition_id FROM edition_candidate ORDER BY edition_id").unwrap().query_map([], |r| r.get::<_, i64>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids, if authority { vec![1, 2] } else { vec![1] });
        let codes: Vec<u8> = c.query_row("SELECT classifications FROM edition_candidate WHERE edition_id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(serde_json::from_slice::<Vec<Classification>>(&codes).unwrap()[0].notation, "QA76");
        assert_eq!(c.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('wanted_by_work','wanted_by_rank')", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(c.query_row("SELECT count(*) FROM sqlite_master WHERE name LIKE 'build_%'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(c.query_row("SELECT count(*) FROM wanted WHERE work_id=10", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}

fn assert_answer_tables_equal(left: &Path, right: &Path) {
    use rusqlite::types::Value;
    let a = crate::snapshot::open(left).unwrap();
    let b = crate::snapshot::open(right).unwrap();
    for (table, order) in [
        ("isbn_answer", "isbn13"),
        ("work_description", "work_id"),
        ("edition_candidate", "edition_id"),
        ("edition_title_key", "key,edition_id"),
        ("authority_title", "key,source,control_number"),
        ("wanted", "isbn13"),
        ("answer_classification", "isbn13"),
        ("answer_work_lcc", "work_id"),
        ("answer_provider", "provider,isbn13"),
        ("answer_demand", "isbn13"),
    ] {
        let read = |c: &Connection| {
            let mut q = c.prepare(&format!("SELECT * FROM {table} ORDER BY {order}")).unwrap();
            let columns = q.column_count();
            q.query_map([], |r| (0..columns).map(|index| r.get::<_, Value>(index)).collect::<Result<Vec<_>, _>>()).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
        };
        assert_eq!(read(&a), read(&b), "table {table}");
    }
    let state = |c: &Connection| c.query_row("SELECT isbn_rows,json_bytes FROM answer_build", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap();
    assert_eq!(state(&a), state(&b));
}

#[test]
fn incremental_provider_and_demand_updates_match_full_builds_and_restore_work_queues() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(
        temp.path(),
        &[
            serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"title":"Shared Title","isbn_13":["9780306406157"]}),
            serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL10W"}],"title":"Shared Title","isbn_13":["9780131103627"]}),
            serde_json::json!({"key":"/books/OL3M","works":[{"key":"/works/OL20W"}],"title":"Other Title","isbn_13":["9780821338278"],"lc_classifications":["E1"]}),
            serde_json::json!({"key":"/books/OL4M","title":"Multiple ISBNs","isbn_13":["9780000000002","9780000000019"]}),
        ],
        &[serde_json::json!({"key":"/works/OL10W","description":"Shared description"})],
        &[],
    );
    let provider_path = temp.path().join("provider.sqlite");
    let provider = crate::crawl::open_provider(&provider_path).unwrap();
    let demand_path = temp.path().join("demand.sqlite");
    let demand = Connection::open(&demand_path).unwrap();
    demand.execute_batch("CREATE TABLE demand(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL); INSERT INTO demand VALUES(9780306406157,9);").unwrap();
    let rating_path = temp.path().join("ratings.gz");
    let mut gzip = GzEncoder::new(File::create(&rating_path).unwrap(), Compression::fast());
    gzip.write_all(b"OL10W\t\t5\t2026-09-01\nOL10W\t\t4\t2026-09-02\nOL20W\t\t3\t2026-09-01\n").unwrap();
    gzip.finish().unwrap();
    let code = |notation: &str| serde_json::to_vec(&vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: notation.into(), source: ClassificationSource::ExactEdition, evidence: vec![] }]).unwrap();
    let mut previous = temp.path().join("base.sqlite");
    build_answers_with_ratings(&database, Some(&provider_path), Some(&demand_path), None, Some(&rating_path), &previous).unwrap();
    let original = fs::read(&previous).unwrap();
    for iteration in 0..5 {
        match iteration {
            0 => {
                provider.execute("INSERT INTO provider_result VALUES('loc',9780306406157,1,'matched',?1,?2)", params![code("QA76"), b"[]".as_slice()]).unwrap();
            }
            1 => {
                // Same timestamp: comparing only fetched_at_ms would miss this.
                provider.execute("UPDATE provider_result SET classifications=?1", [code("HE2751")]).unwrap();
                demand.execute_batch("UPDATE demand SET requests=18; INSERT INTO demand VALUES(9780000000026,3);").unwrap();
                provider.execute("INSERT INTO provider_result VALUES('librarything',9780000000019,1,'matched',?1,?2)", params![code("E1"), b"[]".as_slice()]).unwrap();
            }
            2 => {
                provider.execute_batch("DELETE FROM provider_result WHERE provider='loc'").unwrap();
                demand.execute_batch("DELETE FROM demand").unwrap();
            }
            3 => {
                provider.execute("INSERT INTO provider_result VALUES('loc',9780000000033,2,'matched',?1,?2)", params![code("E1"), b"[]".as_slice()]).unwrap();
            }
            _ => {
                provider.execute_batch("DELETE FROM provider_result").unwrap();
            }
        }
        let updated = temp.path().join(format!("incremental-{iteration}.sqlite"));
        let fresh = temp.path().join(format!("full-{iteration}.sqlite"));
        let report = build_answers_incremental(&database, Some(&provider_path), Some(&demand_path), None, Some(&rating_path), &previous, &updated).unwrap();
        assert_eq!(report.mode, "incremental");
        build_answers_with_ratings(&database, Some(&provider_path), Some(&demand_path), None, Some(&rating_path), &fresh).unwrap();
        assert_answer_tables_equal(&fresh, &updated);
        let server = MetadataService::open(&updated, &temp.path().join(format!("serve-demand-{iteration}.sqlite"))).unwrap();
        let full = MetadataService::open(&fresh, &temp.path().join(format!("serve-full-demand-{iteration}.sqlite"))).unwrap();
        let isbns = vec!["9780306406157", "9780131103627", "9780821338278", "9780000000026", "9780000000033"].into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(server.enrich_rich(isbns.clone()).unwrap()).unwrap(), serde_json::to_value(full.enrich_rich(isbns).unwrap()).unwrap());
        let queries =
            ["Shared Title", "Other Title"].into_iter().map(|title| EditionIdentityQuery { query_id: title.into(), title: title.into(), authors: vec![], author_surname: None, publishers: vec![], book_year: None }).collect::<Vec<_>>();
        assert_eq!(serde_json::to_value(server.resolve_editions(queries.clone()).unwrap()).unwrap(), serde_json::to_value(full.resolve_editions(queries).unwrap()).unwrap());
        let c = crate::snapshot::open(&updated).unwrap();
        let first_work_wanted: i64 = c.query_row("SELECT count(*) FROM wanted WHERE work_id=10", [], |r| r.get(0)).unwrap();
        assert_eq!(first_work_wanted, if iteration < 2 { 0 } else { 2 });
        // A code on the second ISBN must not classify the edition's first-ISBN fallback.
        assert_eq!(c.query_row("SELECT count(*) FROM edition_candidate WHERE edition_id=4", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        previous = updated;
    }
    assert_eq!(fs::read(temp.path().join("base.sqlite")).unwrap(), original);
}

#[test]
fn unchanged_incremental_inputs_reuse_the_same_snapshot_without_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(temp.path(), &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]})], &[], &[]);
    let base = temp.path().join("base.sqlite");
    let next = temp.path().join("next.sqlite");
    build_answers(&database, None, None, None, &base).unwrap();
    let report = build_answers_incremental(&database, None, None, None, None, &base, &next).unwrap();
    assert_eq!(report.mode, "reused");
    assert_eq!(report.resolved_rows, 0);
    assert_eq!(report.reused_rows, 1);
    assert_eq!(fs::read(&base).unwrap(), fs::read(&next).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(base).unwrap().ino(), fs::metadata(next).unwrap().ino());
    }
}

#[test]
fn changed_source_and_legacy_base_fall_back_to_full_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(temp.path(), &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]})], &[], &[]);
    let base = temp.path().join("base.sqlite");
    build_answers(&database, None, None, None, &base).unwrap();
    Connection::open(&database).unwrap().execute("INSERT INTO edition_classification VALUES(1,2,'QA76')", []).unwrap();
    let changed = temp.path().join("changed.sqlite");
    assert_eq!(build_answers_incremental(&database, None, None, None, None, &base, &changed).unwrap().mode, "full");
    Connection::open(&changed).unwrap().execute("DROP TABLE answer_build", []).unwrap();
    let legacy = temp.path().join("legacy.sqlite");
    assert_eq!(build_answers_incremental(&database, None, None, None, None, &changed, &legacy).unwrap().mode, "full");
}

#[test]
fn failed_incremental_resolution_preserves_the_base_and_does_not_publish() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(temp.path(), &[], &[], &[]);
    let provider_path = temp.path().join("provider.sqlite");
    let provider = crate::crawl::open_provider(&provider_path).unwrap();
    let base = temp.path().join("base.sqlite");
    let next = temp.path().join("next.sqlite");
    build_answers(&database, Some(&provider_path), None, None, &base).unwrap();
    let original = fs::read(&base).unwrap();
    provider.execute("INSERT INTO provider_result VALUES('loc',9780306406157,1,'matched',?1,?2)", params![b"invalid JSON".as_slice(), b"[]".as_slice()]).unwrap();
    assert!(build_answers_incremental(&database, Some(&provider_path), None, None, None, &base, &next).is_err());
    assert!(!next.exists());
    assert_eq!(fs::read(&base).unwrap(), original);
    assert!(!next.with_extension(format!("building-{}", std::process::id())).exists());
}

#[test]
fn demand_priority_changes_do_not_resolve_existing_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let database = snapshot_from(temp.path(), &[serde_json::json!({"key":"/books/OL1M","isbn_13":["9780306406157"]})], &[], &[]);
    let demand_path = temp.path().join("demand.sqlite");
    let demand = Connection::open(&demand_path).unwrap();
    demand.execute_batch("CREATE TABLE demand(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL); INSERT INTO demand VALUES(9780306406157,1);").unwrap();
    let base = temp.path().join("base.sqlite");
    let next = temp.path().join("next.sqlite");
    build_answers(&database, None, Some(&demand_path), None, &base).unwrap();
    demand.execute("UPDATE demand SET requests=100", []).unwrap();
    let report = build_answers_incremental(&database, None, Some(&demand_path), None, None, &base, &next).unwrap();
    assert_eq!(report.mode, "incremental");
    assert_eq!(report.resolved_rows, 0);
    assert_eq!(report.reused_rows, 1);
    assert_eq!(crate::snapshot::open(&next).unwrap().query_row("SELECT rank FROM wanted WHERE isbn13=9780306406157", [], |r| r.get::<_, i64>(0)).unwrap(), -1_000_000_000_100);
}
