use super::super::{
    building_path, canonical_isbn13, dump_date, error, fs, open_library_id, params, valid_open_library_description, AuthorRecord, BTreeMap, BTreeSet, BufRead, BufReader, Connection, EditionRecord, File, KeyReference, MetadataError,
    MultiGzDecoder, OptionalExtension, Path, SystemTime, UnicodeNormalization, WorkRecord, BUILDER_SCHEMA_VERSION, DDC_SCHEME, IMPORT_CACHE_KIB, LCC_SCHEME, LEGACY_QUERY_SCHEMA_VERSION, MAX_NOTATION_BYTES, MAX_TITLE_SUBSTRINGS_PER_QUERY,
    OL_PERSON_KIND, OL_PLACE_KIND, OPEN_LIBRARY_CHECKPOINT_RECORDS, SCHEMA_VERSION, UNIX_EPOCH, WIKIDATA_SCHEMA_VERSION,
};
use super::pipeline::read_open_library_resumable;
use super::wikidata::{checkpoint_records, import_wikidata};

pub fn import_snapshot(editions_path: &Path, works_path: &Path, authors_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, None, None, output_path, record_limit)
}

pub fn import_snapshot_with_popularity(editions_path: &Path, works_path: &Path, authors_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, None, Some((ratings_path, reading_log_path)), output_path, record_limit)
}

/// Creates a new schema-v8 snapshot from an immutable bibliography snapshot
/// while streaming only the ratings and reading-log dumps. The source is never
/// opened for writing, so this can safely run beside the serving process.
pub fn add_popularity_to_snapshot(source_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>) -> Result<(), MetadataError> {
    if output_path.exists() {
        return Err(MetadataError(format!("output already exists: {}", output_path.display())));
    }
    let building_path = building_path(output_path);
    if building_path.exists() {
        return Err(MetadataError(format!("unfinished popularity snapshot already exists: {}", building_path.display())));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    fs::copy(source_path, &building_path).map_err(error)?;
    let result = (|| {
        let mut connection = Connection::open(&building_path).map_err(error)?;
        configure_import(&connection)?;
        let source_schema = connection.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", (), |row| row.get::<_, i64>(0)).map_err(error)?;
        if !matches!(source_schema, LEGACY_QUERY_SCHEMA_VERSION | SCHEMA_VERSION | WIKIDATA_SCHEMA_VERSION) {
            return Err(MetadataError(format!("cannot add popularity to metadata schema {source_schema}; expected {LEGACY_QUERY_SCHEMA_VERSION}, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")));
        }
        let has_wikidata = connection.query_row("SELECT EXISTS(SELECT 1 FROM entity LIMIT 1)", (), |row| row.get::<_, bool>(0)).map_err(error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS work_popularity(
                    work_id INTEGER PRIMARY KEY,
                    want_to_read_count INTEGER NOT NULL DEFAULT 0,
                    currently_reading_count INTEGER NOT NULL DEFAULT 0,
                    already_read_count INTEGER NOT NULL DEFAULT 0,
                    rating_count INTEGER NOT NULL DEFAULT 0,
                    rating_sum INTEGER NOT NULL DEFAULT 0
                 );
                 DELETE FROM import_checkpoint WHERE phase IN ('ratings','reading_log','snapshot_finalize');",
            )
            .map_err(error)?;
        import_work_popularity(&mut connection, ratings_path, reading_log_path, record_limit)?;
        let imported_at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis()).map_err(error)?;
        let output_schema = if has_wikidata { WIKIDATA_SCHEMA_VERSION } else { SCHEMA_VERSION };
        connection.execute("UPDATE snapshot SET schema_version=?1,imported_at_ms=?2 WHERE singleton=1", params![output_schema, imported_at_ms]).map_err(error)?;
        connection.execute("INSERT OR REPLACE INTO metadata_metric(name,value) VALUES('core_open_library_popularity',1)", ()).map_err(error)?;
        connection.execute_batch("VACUUM; ANALYZE; PRAGMA optimize;").map_err(error)?;
        let integrity: String = connection.query_row("PRAGMA integrity_check", (), |row| row.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("metadata database integrity check failed: {integrity}")));
        }
        connection.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('snapshot_finalize',0,1) ON CONFLICT(phase) DO UPDATE SET records_read=0,completed=1", ()).map_err(error)?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(error)?;
        Ok(())
    })();
    if let Err(failure) = result {
        return Err(failure);
    }
    fs::rename(&building_path, output_path).map_err(error)?;
    Ok(())
}

pub fn import_rich_snapshot_with_popularity(
    editions_path: &Path, works_path: &Path, authors_path: &Path, wikidata_path: &Path, ratings_path: &Path, reading_log_path: &Path, output_path: &Path, record_limit: Option<usize>,
) -> Result<(), MetadataError> {
    import_snapshot_inner(editions_path, works_path, authors_path, Some(wikidata_path), Some((ratings_path, reading_log_path)), output_path, record_limit)
}

pub(crate) fn import_snapshot_inner(
    editions_path: &Path, works_path: &Path, authors_path: &Path, wikidata_path: Option<&Path>, popularity_paths: Option<(&Path, &Path)>, output_path: &Path, record_limit: Option<usize>,
) -> Result<(), MetadataError> {
    if output_path.exists() {
        return Err(MetadataError(format!("output already exists: {}", output_path.display())));
    }
    let building_path = building_path(output_path);
    let resuming = building_path.exists();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    let mut connection = Connection::open(&building_path).map_err(error)?;
    configure_import(&connection)?;
    if resuming {
        validate_import_inputs(&connection, &building_path, editions_path, works_path, authors_path, wikidata_path, popularity_paths, record_limit)?;
        println!("resuming metadata snapshot build {}", building_path.display());
    } else {
        let transaction = connection.transaction().map_err(error)?;
        create_schema(&transaction)?;
        register_import_inputs(&transaction, editions_path, works_path, authors_path, wikidata_path, popularity_paths, record_limit)?;
        transaction.commit().map_err(error)?;
    }
    let result = (|| {
        let edition_count = import_editions(&mut connection, editions_path, record_limit)?;
        import_edition_identities(&mut connection, editions_path, record_limit)?;
        connection.execute("CREATE INDEX IF NOT EXISTS edition_by_work ON edition(work_id)", ()).map_err(error)?;
        connection
            .execute(
                "DELETE FROM edition_work_classification
                 WHERE NOT EXISTS (
                     SELECT 1 FROM edition
                     WHERE edition.work_id=edition_work_classification.work_id
                 )",
                (),
            )
            .map_err(error)?;
        let work_count = import_works(&mut connection, works_path, record_limit)?;
        import_work_titles(&mut connection, works_path, record_limit)?;
        if let Some((ratings_path, reading_log_path)) = popularity_paths {
            import_work_popularity(&mut connection, ratings_path, reading_log_path, record_limit)?;
        }
        connection.execute_batch("CREATE INDEX IF NOT EXISTS edition_author_by_author ON edition_author(author_id); CREATE INDEX IF NOT EXISTS work_author_by_author ON work_author(author_id);").map_err(error)?;
        let author_count = import_authors(&mut connection, authors_path, record_limit)?;
        if let Some(wikidata_path) = wikidata_path {
            import_wikidata(&mut connection, wikidata_path, record_limit)?;
        }
        // `edition_by_work` is also required by the work-title branch of the
        // edition-identity lookup, so it is part of the shipped read schema.
        connection.execute_batch("DROP INDEX IF EXISTS edition_author_by_author; DROP INDEX IF EXISTS work_author_by_author;").map_err(error)?;
        if completed_phase(&connection, "snapshot_finalize")?.is_none() {
            let dump_date = dump_date(editions_path).or_else(|| dump_date(works_path)).or_else(|| dump_date(authors_path)).unwrap_or_else(|| "unknown".to_owned());
            let imported_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
            let imported_at_ms = i64::try_from(imported_at_ms).map_err(error)?;
            let snapshot_schema_version = if wikidata_path.is_some() { WIKIDATA_SCHEMA_VERSION } else { SCHEMA_VERSION };
            connection
                .execute(
                    "INSERT OR REPLACE INTO snapshot(singleton,schema_version,dump_date,imported_at_ms,edition_records,work_records,author_records) VALUES(1,?1,?2,?3,?4,?5,?6)",
                    params![snapshot_schema_version, dump_date, imported_at_ms, edition_count, work_count, author_count],
                )
                .map_err(error)?;
            connection.execute_batch("VACUUM; ANALYZE; PRAGMA optimize;").map_err(error)?;
            let integrity: String = connection.query_row("PRAGMA integrity_check", (), |row| row.get(0)).map_err(error)?;
            if integrity != "ok" {
                return Err(MetadataError(format!("metadata database integrity check failed: {integrity}")));
            }
            connection.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('snapshot_finalize',0,1) ON CONFLICT(phase) DO UPDATE SET completed=1", ()).map_err(error)?;
        }
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(error)?;
        Ok(())
    })();
    drop(connection);
    result?;
    fs::rename(&building_path, output_path).map_err(error)?;
    let size = fs::metadata(output_path).map_err(error)?.len();
    println!("created metadata snapshot {} ({size} bytes)", output_path.display());
    Ok(())
}

pub(crate) fn completed_phase(connection: &Connection, phase: &str) -> Result<Option<i64>, MetadataError> {
    connection.query_row("SELECT records_read FROM import_checkpoint WHERE phase=?1 AND completed=1", [phase], |row| row.get(0)).optional().map_err(error)
}

pub(crate) fn register_import_inputs(connection: &Connection, editions: &Path, works: &Path, authors: &Path, wikidata: Option<&Path>, popularity: Option<(&Path, &Path)>, record_limit: Option<usize>) -> Result<(), MetadataError> {
    connection.execute("INSERT INTO import_setting(singleton,record_limit,rich) VALUES(1,?1,?2)", params![record_limit.map(i64::try_from).transpose().map_err(error)?, i64::from(wikidata.is_some())]).map_err(error)?;
    for (name, path) in [("editions", Some(editions)), ("works", Some(works)), ("authors", Some(authors)), ("wikidata", wikidata), ("ratings", popularity.map(|value| value.0)), ("reading_log", popularity.map(|value| value.1))] {
        if let Some(path) = path {
            let (path, size_bytes, modified_ms) = import_input_identity(path)?;
            connection.execute("INSERT INTO import_input(name,path,size_bytes,modified_ms) VALUES(?1,?2,?3,?4)", params![name, path, size_bytes, modified_ms]).map_err(error)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_import_inputs(
    connection: &Connection, building: &Path, editions: &Path, works: &Path, authors: &Path, wikidata: Option<&Path>, popularity: Option<(&Path, &Path)>, record_limit: Option<usize>,
) -> Result<(), MetadataError> {
    let builder_version = connection.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", (), |row| row.get::<_, i64>(0)).optional().map_err(error)?;
    if builder_version != Some(BUILDER_SCHEMA_VERSION) {
        return Err(MetadataError(format!(
            "unfinished snapshot {} uses builder schema {}; current builder schema v{BUILDER_SCHEMA_VERSION} must be rebuilt from its pinned inputs after the stale building file is removed",
            building.display(),
            builder_version.map_or_else(|| "unknown".to_owned(), |version| format!("v{version}")),
        )));
    }
    let expected_limit = record_limit.map(i64::try_from).transpose().map_err(error)?;
    let settings = connection
        .query_row("SELECT record_limit,rich FROM import_setting WHERE singleton=1", (), |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, bool>(1)?)))
        .map_err(|failure| MetadataError(format!("unfinished snapshot cannot be resumed (missing compatible checkpoint schema): {failure}")))?;
    if settings != (expected_limit, wikidata.is_some()) {
        return Err(MetadataError("unfinished snapshot was created with a different record limit or import mode".to_owned()));
    }
    for (name, path) in [("editions", Some(editions)), ("works", Some(works)), ("authors", Some(authors)), ("wikidata", wikidata), ("ratings", popularity.map(|value| value.0)), ("reading_log", popularity.map(|value| value.1))] {
        let Some(path) = path else { continue };
        let identity = import_input_identity(path)?;
        let stored = connection.query_row("SELECT path,size_bytes,modified_ms FROM import_input WHERE name=?1", [name], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).optional().map_err(error)?;
        if stored.as_ref() != Some(&identity) {
            return Err(MetadataError(format!("cannot resume: {name} dump path, size, or modification time changed")));
        }
    }
    Ok(())
}

pub(crate) fn import_input_identity(path: &Path) -> Result<(String, i64, i64), MetadataError> {
    let canonical = fs::canonicalize(path).map_err(error)?;
    let metadata = fs::metadata(&canonical).map_err(error)?;
    let size_bytes = i64::try_from(metadata.len()).map_err(error)?;
    let modified_ms = metadata.modified().map_err(error)?.duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
    Ok((canonical.to_string_lossy().into_owned(), size_bytes, i64::try_from(modified_ms).map_err(error)?))
}

pub(crate) fn configure_import(connection: &Connection) -> Result<(), MetadataError> {
    connection.execute_batch(&format!("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=FILE; PRAGMA cache_size=-{IMPORT_CACHE_KIB}; PRAGMA locking_mode=EXCLUSIVE;")).map_err(error)
}

const CORE_SCHEMA: &str = include_str!("core_schema.sql");

pub(crate) fn create_schema(connection: &Connection) -> Result<(), MetadataError> {
    connection.execute_batch(CORE_SCHEMA).map_err(error)
}
pub(crate) fn import_editions(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<EditionRecord>(connection, path, limit, "editions", |transaction, records| {
        let mut edition_insert = transaction.prepare_cached("INSERT INTO edition_stage(edition_id,work_id) VALUES(?1,?2)").map_err(error)?;
        let mut isbn_insert = transaction.prepare_cached("INSERT INTO edition_isbn_stage(isbn13,edition_id) VALUES(?1,?2)").map_err(error)?;
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO edition_bibliography_stage(edition_id,title,normalized_title,publication_year) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut publisher_insert = transaction.prepare_cached("INSERT INTO edition_publisher_stage(edition_id,position,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut classification_insert = transaction.prepare_cached("INSERT INTO edition_classification_stage(edition_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut work_classification_insert = transaction.prepare_cached("INSERT INTO edition_work_classification_stage(work_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut author_insert = transaction.prepare_cached("INSERT INTO edition_author_stage(edition_id,position,author_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut entity_insert = transaction.prepare_cached("INSERT INTO edition_entity_evidence_stage(edition_id,kind,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        for record in records {
            let Some(edition_id) = open_library_id(&record.key, 'M') else { continue };
            let mut isbns = BTreeSet::new();
            isbns.extend(record.isbn_10.iter().filter_map(|isbn| canonical_isbn13(isbn)));
            isbns.extend(record.isbn_13.iter().filter_map(|isbn| canonical_isbn13(isbn)));
            let work_id = record.works.first().and_then(|work| open_library_id(&work.key, 'W'));
            if let Some(work_id) = work_id {
                insert_classifications(&mut work_classification_insert, work_id, DDC_SCHEME, &record.dewey_decimal_class)?;
                insert_classifications(&mut work_classification_insert, work_id, LCC_SCHEME, &record.lc_classifications)?;
            }
            if isbns.is_empty() {
                continue;
            }
            edition_insert.execute(params![edition_id, work_id]).map_err(error)?;
            for isbn in isbns {
                isbn_insert.execute(params![isbn, edition_id]).map_err(error)?;
            }
            if let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) {
                let normalized_title = bibliographic_match_key(&title);
                if !normalized_title.is_empty() {
                    bibliography_insert.execute(params![edition_id, title, normalized_title, record.publish_date.as_deref().and_then(publication_year)]).map_err(error)?;
                    for (position, publisher) in record.publishers.into_iter().enumerate() {
                        let publisher = compact_text(&publisher);
                        let normalized = bibliographic_match_key(&publisher);
                        if !publisher.is_empty() && publisher.len() <= 4_096 && !normalized.is_empty() {
                            publisher_insert.execute(params![edition_id, i64::try_from(position).map_err(error)?, publisher, normalized]).map_err(error)?;
                        }
                    }
                }
            }
            insert_classifications(&mut classification_insert, edition_id, DDC_SCHEME, &record.dewey_decimal_class)?;
            insert_classifications(&mut classification_insert, edition_id, LCC_SCHEME, &record.lc_classifications)?;
            insert_author_references(&mut author_insert, edition_id, record.authors)?;
            insert_entity_evidence(&mut entity_insert, edition_id, OL_PLACE_KIND, record.subject_places)?;
            insert_entity_evidence(&mut entity_insert, edition_id, OL_PERSON_KIND, record.subject_people)?;
        }
        Ok(())
    })?;
    materialize_editions(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM edition", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} edition records; retained {retained} with a valid ISBN");
    Ok(count)
}

fn import_edition_identities(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    if completed_phase(connection, "edition_identities_materialize")?.is_some() {
        return Ok(());
    }
    read_open_library_resumable::<EditionRecord>(connection, path, limit, "edition_identities", |transaction, records| {
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO edition_bibliography_stage(edition_id,title,normalized_title,publication_year) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut publisher_insert = transaction.prepare_cached("INSERT INTO edition_publisher_stage(edition_id,position,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE edition_id=?1)").map_err(error)?;
        for record in records {
            let Some(edition_id) = open_library_id(&record.key, 'M') else { continue };
            if !retained.query_row([edition_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) else { continue };
            let normalized_title = bibliographic_match_key(&title);
            if normalized_title.is_empty() {
                continue;
            }
            bibliography_insert.execute(params![edition_id, title, normalized_title, record.publish_date.as_deref().and_then(publication_year)]).map_err(error)?;
            for (position, publisher) in record.publishers.into_iter().enumerate() {
                let publisher = compact_text(&publisher);
                let normalized = bibliographic_match_key(&publisher);
                if !publisher.is_empty() && publisher.len() <= 4_096 && !normalized.is_empty() {
                    publisher_insert.execute(params![edition_id, i64::try_from(position).map_err(error)?, publisher, normalized]).map_err(error)?;
                }
            }
        }
        Ok(())
    })?;
    materialize_edition_identities(connection)
}

fn import_works(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<WorkRecord>(connection, path, limit, "works", |transaction, records| {
        let mut bibliography_insert = transaction.prepare_cached("INSERT INTO work_bibliography_stage(work_id,title,normalized_title) VALUES(?1,?2,?3)").map_err(error)?;
        let mut description_insert = transaction.prepare_cached("INSERT INTO work_description_stage(work_id,description) VALUES(?1,?2)").map_err(error)?;
        let mut classification_insert = transaction.prepare_cached("INSERT INTO work_classification_stage(work_id,scheme,notation) VALUES(?1,?2,?3)").map_err(error)?;
        let mut author_insert = transaction.prepare_cached("INSERT INTO work_author_stage(work_id,position,author_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut entity_insert = transaction.prepare_cached("INSERT INTO work_entity_evidence_stage(work_id,kind,name,normalized_name) VALUES(?1,?2,?3,?4)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        for record in records {
            let Some(work_id) = open_library_id(&record.key, 'W') else { continue };
            if !referenced.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            if let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) {
                let normalized_title = bibliographic_match_key(&title);
                if !normalized_title.is_empty() {
                    bibliography_insert.execute(params![work_id, title, normalized_title]).map_err(error)?;
                }
            }
            if let Some(description) = record.description.as_deref().and_then(valid_open_library_description) {
                description_insert.execute(params![work_id, description]).map_err(error)?;
            }
            insert_classifications(&mut classification_insert, work_id, DDC_SCHEME, &record.dewey_number)?;
            insert_classifications(&mut classification_insert, work_id, LCC_SCHEME, &record.lc_classifications)?;
            insert_author_references(&mut author_insert, work_id, record.authors)?;
            insert_entity_evidence(&mut entity_insert, work_id, OL_PLACE_KIND, record.subject_places)?;
            insert_entity_evidence(&mut entity_insert, work_id, OL_PERSON_KIND, record.subject_people)?;
        }
        Ok(())
    })?;
    materialize_works(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(DISTINCT work_id) FROM edition WHERE work_id IS NOT NULL", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} work records; retained {retained} referenced by an ISBN edition");
    Ok(count)
}

fn import_work_titles(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    if completed_phase(connection, "work_titles_materialize")?.is_some() {
        return Ok(());
    }
    read_open_library_resumable::<WorkRecord>(connection, path, limit, "work_titles", |transaction, records| {
        let mut insert = transaction.prepare_cached("INSERT INTO work_bibliography_stage(work_id,title,normalized_title) VALUES(?1,?2,?3)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        for record in records {
            let Some(work_id) = open_library_id(&record.key, 'W') else { continue };
            if !referenced.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let Some(title) = record.title.as_deref().map(compact_text).filter(|title| !title.is_empty() && title.len() <= 4_096) else { continue };
            let normalized_title = bibliographic_match_key(&title);
            if !normalized_title.is_empty() {
                insert.execute(params![work_id, title, normalized_title]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    materialize_work_titles(connection)
}

pub(crate) fn import_work_popularity(connection: &mut Connection, ratings_path: &Path, reading_log_path: &Path, limit: Option<usize>) -> Result<(), MetadataError> {
    read_tsv_resumable(connection, ratings_path, limit, "ratings", |transaction, lines| {
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        let mut update = transaction
            .prepare_cached(
                "INSERT INTO work_popularity(work_id,rating_count,rating_sum) VALUES(?1,1,?2)
                 ON CONFLICT(work_id) DO UPDATE SET rating_count=rating_count+1,rating_sum=rating_sum+excluded.rating_sum",
            )
            .map_err(error)?;
        for line in lines {
            let fields = line.trim_end().split('\t').collect::<Vec<_>>();
            let Some(work_id) = fields.first().and_then(|value| open_library_id(value, 'W')) else { continue };
            let Some(rating) = fields.get(2).and_then(|value| value.parse::<i64>().ok()).filter(|value| (1..=5).contains(value)) else { continue };
            if retained.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                update.execute(params![work_id, rating]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    read_tsv_resumable(connection, reading_log_path, limit, "reading_log", |transaction, lines| {
        let mut retained = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition WHERE work_id=?1)").map_err(error)?;
        let mut update = transaction
            .prepare_cached(
                "INSERT INTO work_popularity(work_id,want_to_read_count,currently_reading_count,already_read_count)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(work_id) DO UPDATE SET
                   want_to_read_count=want_to_read_count+excluded.want_to_read_count,
                   currently_reading_count=currently_reading_count+excluded.currently_reading_count,
                   already_read_count=already_read_count+excluded.already_read_count",
            )
            .map_err(error)?;
        for line in lines {
            let fields = line.trim_end().split('\t').collect::<Vec<_>>();
            let Some(work_id) = fields.first().and_then(|value| open_library_id(value, 'W')) else { continue };
            let counts = match fields.get(2).copied() {
                Some("Want to Read") => (1_i64, 0_i64, 0_i64),
                Some("Currently Reading") => (0, 1, 0),
                Some("Already Read") => (0, 0, 1),
                _ => continue,
            };
            if retained.query_row([work_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                update.execute(params![work_id, counts.0, counts.1, counts.2]).map_err(error)?;
            }
        }
        Ok(())
    })?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM work_popularity", (), |row| row.get(0)).map_err(error)?;
    println!("retained popularity aggregates for {retained} ISBN-linked works");
    Ok(())
}

pub(crate) fn read_tsv_resumable(
    connection: &mut Connection, path: &Path, limit: Option<usize>, phase: &str, mut import_batch: impl FnMut(&rusqlite::Transaction<'_>, Vec<String>) -> Result<(), MetadataError>,
) -> Result<usize, MetadataError> {
    if let Some(records) = completed_phase(connection, phase)? {
        return usize::try_from(records).map_err(error);
    }
    let resume_at = usize::try_from(checkpoint_records(connection, phase)?).map_err(error)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, MultiGzDecoder::new(BufReader::with_capacity(1024 * 1024, File::open(path).map_err(error)?)));
    let mut line = String::new();
    let mut count = 0_usize;
    while count < resume_at {
        line.clear();
        if reader.read_line(&mut line).map_err(error)? == 0 {
            return Err(MetadataError(format!("cannot resume {phase} at record {resume_at}: dump ended after {count} records")));
        }
        count += 1;
    }
    loop {
        let mut lines = Vec::with_capacity(OPEN_LIBRARY_CHECKPOINT_RECORDS);
        let mut finished = limit.is_some_and(|limit| count >= limit);
        while lines.len() < OPEN_LIBRARY_CHECKPOINT_RECORDS && !finished {
            line.clear();
            if reader.read_line(&mut line).map_err(error)? == 0 {
                finished = true;
                break;
            }
            count += 1;
            lines.push(line.clone());
            if limit.is_some_and(|limit| count >= limit) {
                finished = true;
            }
        }
        let transaction = connection.transaction().map_err(error)?;
        import_batch(&transaction, lines)?;
        transaction
            .execute(
                "INSERT INTO import_checkpoint(phase,records_read,completed) VALUES(?1,?2,?3)
                 ON CONFLICT(phase) DO UPDATE SET records_read=excluded.records_read,completed=excluded.completed",
                params![phase, i64::try_from(count).map_err(error)?, i64::from(finished)],
            )
            .map_err(error)?;
        transaction.commit().map_err(error)?;
        if finished {
            return Ok(count);
        }
    }
}

fn import_authors(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let count = read_open_library_resumable::<AuthorRecord>(connection, path, limit, "authors", |transaction, records| {
        let mut author_insert = transaction.prepare_cached("INSERT INTO author_stage(author_id,name) VALUES(?1,?2)").map_err(error)?;
        let mut identifier_insert = transaction.prepare_cached("INSERT INTO author_identifier_stage(author_id,authority,external_id) VALUES(?1,?2,?3)").map_err(error)?;
        let mut referenced = transaction.prepare_cached("SELECT EXISTS(SELECT 1 FROM edition_author WHERE author_id=?1 UNION ALL SELECT 1 FROM work_author WHERE author_id=?1)").map_err(error)?;
        for record in records {
            let Some(author_id) = open_library_id(&record.key, 'A') else { continue };
            if !referenced.query_row([author_id], |row| row.get::<_, bool>(0)).map_err(error)? {
                continue;
            }
            let name = record.name.map(|name| name.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|name| !name.is_empty() && name.len() <= 4_096);
            author_insert.execute(params![author_id, name]).map_err(error)?;
            for (authority, value) in record.remote_ids {
                let authority = authority.trim().to_ascii_lowercase();
                let value = match value {
                    serde_json::Value::String(value) => value,
                    serde_json::Value::Number(value) => value.to_string(),
                    _ => continue,
                };
                let value = value.trim();
                if !authority.is_empty() && authority.len() <= 64 && !value.is_empty() && value.len() <= 512 {
                    identifier_insert.execute(params![author_id, authority, value]).map_err(error)?;
                }
            }
        }
        Ok(())
    })?;
    materialize_authors(connection)?;
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM author", (), |row| row.get(0)).map_err(error)?;
    println!("read {count} author records; retained {retained} referenced by an ISBN edition or work");
    Ok(count)
}

fn materialize_editions(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "editions_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO edition(edition_id,work_id)
                 SELECT edition_id,work_id FROM edition_stage ORDER BY edition_id;
             INSERT OR IGNORE INTO edition_isbn(isbn13,edition_id)
                 SELECT isbn13,edition_id FROM edition_isbn_stage ORDER BY isbn13,edition_id;
             INSERT OR REPLACE INTO edition_bibliography(edition_id,title,normalized_title,publication_year)
                 SELECT edition_id,title,normalized_title,publication_year FROM edition_bibliography_stage ORDER BY normalized_title,edition_id;
             INSERT OR IGNORE INTO edition_publisher(edition_id,position,name,normalized_name)
                 SELECT edition_id,position,name,normalized_name FROM edition_publisher_stage ORDER BY edition_id,position;
             INSERT OR IGNORE INTO edition_classification(edition_id,scheme,notation)
                 SELECT edition_id,scheme,notation FROM edition_classification_stage ORDER BY edition_id,scheme,notation;
             INSERT OR IGNORE INTO edition_work_classification(work_id,scheme,notation)
                 SELECT work_id,scheme,notation FROM edition_work_classification_stage ORDER BY work_id,scheme,notation;
             INSERT OR IGNORE INTO edition_author(edition_id,position,author_id)
                 SELECT edition_id,position,author_id FROM edition_author_stage ORDER BY edition_id,position;
             INSERT OR IGNORE INTO edition_entity_evidence(edition_id,kind,name,normalized_name)
                 SELECT edition_id,kind,name,normalized_name FROM edition_entity_evidence_stage ORDER BY edition_id,kind,name;
             DROP TABLE edition_stage;
             DROP TABLE edition_isbn_stage;
             DROP TABLE edition_bibliography_stage;
             DROP TABLE edition_publisher_stage;
             DROP TABLE edition_classification_stage;
             DROP TABLE edition_work_classification_stage;
             DROP TABLE edition_author_stage;
             DROP TABLE edition_entity_evidence_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "editions_materialize")?;
    mark_materialized(&transaction, "edition_identities_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_edition_identities(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "edition_identities_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO edition_bibliography(edition_id,title,normalized_title,publication_year)
                 SELECT edition_id,title,normalized_title,publication_year FROM edition_bibliography_stage ORDER BY normalized_title,edition_id;
             INSERT OR IGNORE INTO edition_publisher(edition_id,position,name,normalized_name)
                 SELECT edition_id,position,name,normalized_name FROM edition_publisher_stage ORDER BY edition_id,position;
             DROP TABLE edition_bibliography_stage;
             DROP TABLE edition_publisher_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "edition_identities_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_works(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "works_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR IGNORE INTO work_classification(work_id,scheme,notation)
                 SELECT work_id,scheme,notation FROM work_classification_stage ORDER BY work_id,scheme,notation;
             INSERT OR IGNORE INTO work_author(work_id,position,author_id)
                 SELECT work_id,position,author_id FROM work_author_stage ORDER BY work_id,position;
             INSERT OR IGNORE INTO work_entity_evidence(work_id,kind,name,normalized_name)
                 SELECT work_id,kind,name,normalized_name FROM work_entity_evidence_stage ORDER BY work_id,kind,name;
             INSERT OR REPLACE INTO work_bibliography(work_id,title,normalized_title)
                 SELECT work_id,title,normalized_title FROM work_bibliography_stage ORDER BY normalized_title,work_id;
             INSERT OR REPLACE INTO work_description(work_id,description)
                 SELECT work_id,description FROM work_description_stage ORDER BY work_id;
             DROP TABLE work_classification_stage;
             DROP TABLE work_bibliography_stage;
             DROP TABLE work_description_stage;
             DROP TABLE work_author_stage;
             DROP TABLE work_entity_evidence_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "works_materialize")?;
    mark_materialized(&transaction, "work_titles_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_work_titles(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "work_titles_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO work_bibliography(work_id,title,normalized_title)
                 SELECT work_id,title,normalized_title FROM work_bibliography_stage ORDER BY normalized_title,work_id;
             DROP TABLE work_bibliography_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "work_titles_materialize")?;
    transaction.commit().map_err(error)
}

fn materialize_authors(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "authors_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO author(author_id,name)
                 SELECT author_id,name FROM author_stage ORDER BY author_id;
             INSERT OR IGNORE INTO author_identifier(author_id,authority,external_id)
                 SELECT author_id,authority,external_id FROM author_identifier_stage ORDER BY author_id,authority,external_id;
             DROP TABLE author_stage;
             DROP TABLE author_identifier_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "authors_materialize")?;
    transaction.commit().map_err(error)
}

pub(crate) fn mark_materialized(transaction: &rusqlite::Transaction<'_>, phase: &str) -> Result<(), MetadataError> {
    transaction.execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES(?1,0,1) ON CONFLICT(phase) DO UPDATE SET completed=1", [phase]).map(|_| ()).map_err(error)
}

pub(crate) fn insert_author_references(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, authors: Vec<KeyReference>) -> Result<(), MetadataError> {
    for (position, author) in authors.into_iter().enumerate() {
        let Some(author_id) = open_library_id(&author.key, 'A') else { continue };
        statement.execute(params![owner_id, i64::try_from(position).map_err(error)?, author_id]).map_err(error)?;
    }
    Ok(())
}

pub(crate) fn insert_classifications(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, scheme: i64, values: &[String]) -> Result<(), MetadataError> {
    for notation in values {
        let notation = notation.split_whitespace().collect::<Vec<_>>().join(" ");
        if notation.is_empty() || notation.len() > MAX_NOTATION_BYTES {
            continue;
        }
        statement.execute(params![owner_id, scheme, notation]).map_err(error)?;
    }
    Ok(())
}

pub(crate) fn insert_entity_evidence(statement: &mut rusqlite::CachedStatement<'_>, owner_id: i64, kind: i64, values: Vec<String>) -> Result<(), MetadataError> {
    for name in values {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.len() > 4_096 {
            continue;
        }
        let normalized_name = normalize_entity_heading(kind, &name);
        if normalized_name.is_empty() {
            continue;
        }
        statement.execute(params![owner_id, kind, name, normalized_name]).map_err(error)?;
    }
    Ok(())
}

pub(crate) fn normalize_entity_heading(kind: i64, value: &str) -> String {
    let value = if kind == OL_PERSON_KIND { strip_person_dates(value) } else { value };
    value.nfkc().flat_map(char::to_lowercase).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn strip_person_dates(value: &str) -> &str {
    let trimmed = value.trim();
    let Some(open) = trimmed.rfind(" (") else { return trimmed };
    let Some(dates) = trimmed.get(open + 2..trimmed.len().saturating_sub(1)) else { return trimmed };
    if trimmed.ends_with(')') && dates.chars().all(|character| character.is_ascii_digit() || matches!(character, '-' | '?' | ' ')) && dates.contains('-') {
        trimmed[..open].trim()
    } else {
        trimmed
    }
}

pub(crate) fn compact_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Locale-independent key for conservative equality matching. Punctuation and
/// accents are not edition identity; token order is retained.
pub(crate) fn bibliographic_match_key(value: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    let mut key = String::new();
    let mut separator = false;
    for character in value.nfkd().flat_map(char::to_lowercase) {
        if is_combining_mark(character) {
            continue;
        }
        if character.is_alphanumeric() {
            if separator && !key.is_empty() {
                key.push(' ');
            }
            key.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    key
}

pub(crate) fn title_substring_keys(value: &str) -> Vec<(String, usize)> {
    let normalized = bibliographic_match_key(value);
    let words = normalized.split_whitespace().take(MAX_TITLE_SUBSTRINGS_PER_QUERY).collect::<Vec<_>>();
    let mut substrings = BTreeMap::<String, usize>::new();
    for start in 0..words.len() {
        for end in start + 1..=words.len() {
            let substring = words[start..end].join(" ");
            let specificity = substring.chars().count();
            substrings.entry(substring).or_insert(specificity);
        }
    }
    let mut substrings = substrings.into_iter().collect::<Vec<_>>();
    substrings.sort_by(|(left_title, left_specificity), (right_title, right_specificity)| right_specificity.cmp(left_specificity).then_with(|| left_title.cmp(right_title)));
    substrings.truncate(MAX_TITLE_SUBSTRINGS_PER_QUERY);
    substrings
}

pub(crate) fn author_match_keys(value: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::from([bibliographic_match_key(value)]);
    if let Some((family, given)) = value.split_once(',') {
        keys.insert(bibliographic_match_key(&format!("{given} {family}")));
    }
    keys.remove("");
    keys
}

pub(crate) fn author_match_tokens(value: &str) -> BTreeSet<String> {
    author_match_keys(value).into_iter().flat_map(|key| key.split_whitespace().map(str::to_owned).collect::<Vec<_>>()).filter(|token| token.len() > 1 && !matches!(token.as_str(), "jr" | "sr" | "ii" | "iii" | "iv" | "v")).collect()
}

pub(crate) fn publication_year(value: &str) -> Option<i32> {
    let mut years = BTreeSet::new();
    for run in value.split(|character: char| !character.is_ascii_digit()) {
        if run.len() == 4 {
            if let Ok(year) = run.parse::<i32>() {
                if (1000..=2100).contains(&year) {
                    years.insert(year);
                }
            }
        }
    }
    (years.len() == 1).then(|| *years.first().expect("one year exists"))
}
