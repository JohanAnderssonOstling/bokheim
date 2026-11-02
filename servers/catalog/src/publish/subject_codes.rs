//! Convert an existing snapshot without touching its curation or asset files.
use super::{finish_snapshot, Descriptions, FORMAT_VERSION, PAGE_SIZE, SCHEMA};
use reqwest::Url;
use rusqlite::{params, types::Value, Connection};
use std::fs::{remove_file, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(super) fn copy_assignments(input: &Path, output: &Path, release: i64) -> Result<(), String> {
    let target = Connection::open_in_memory().map_err(|error| error.to_string())?;
    target.pragma_update(None, "page_size", PAGE_SIZE).map_err(|error| error.to_string())?;
    target.pragma_update(None, "temp_store", "MEMORY").map_err(|error| error.to_string())?;
    let input = input.canonicalize().map_err(|error| error.to_string())?;
    let mut uri = Url::from_file_path(&input).map_err(|_| "invalid snapshot path")?;
    uri.set_query(Some("mode=ro"));
    target.execute("ATTACH DATABASE ?1 AS previous", [uri.as_str()]).map_err(|error| error.to_string())?;
    target.execute_batch(SCHEMA).map_err(|error| error.to_string())?;
    target.execute("INSERT INTO snapshot VALUES(1,?1,?2)", params![FORMAT_VERSION, release]).map_err(|error| error.to_string())?;
    let mut descriptions = Descriptions::default();
    let mut rows = target.prepare("SELECT row_id,text FROM previous.description WHERE row_id IN(SELECT description_id FROM previous.book) ORDER BY row_id").map_err(|error| error.to_string())?;
    for row in rows.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Value>(1)?))).map_err(|error| error.to_string())? {
        let (id, text) = row.map_err(|error| error.to_string())?;
        let text = description_storage::decode((&text).into()).map_err(|error| error.to_string())?;
        descriptions.store_at(&target, id, &text)?;
    }
    drop(rows);
    for table in ["source", "book", "author", "book_author", "book_language", "book_identifier", "book_concept", "book_source", "subject_projection"] {
        let filter = if table == "author" { " WHERE row_id IN(SELECT author_row_id FROM previous.book_author)" } else { "" };
        target.execute_batch(&format!("INSERT INTO {table} SELECT * FROM previous.{table}{filter}")).map_err(|error| error.to_string())?;
    }
    let integer_entries: bool = target.query_row("SELECT format_version>=11 FROM previous.snapshot WHERE singleton=1", [], |row| row.get(0)).map_err(|error| error.to_string())?;
    if integer_entries {
        target.execute_batch("INSERT INTO source_entry SELECT * FROM previous.source_entry; INSERT INTO source_entry_source SELECT * FROM previous.source_entry_source;").map_err(|error| error.to_string())?;
    } else {
        target
            .execute_batch(
                "CREATE TEMP TABLE entry_identity AS
         SELECT source_id,entry_id AS original_entry_id,row_number() OVER(ORDER BY source_id,entry_id) AS entry_id FROM previous.source_entry;
         CREATE UNIQUE INDEX temp.entry_identity_original ON entry_identity(source_id,original_entry_id);
         INSERT INTO source_entry
         SELECT entry.source_id,identity.entry_id,entry.hosted_row_id,entry.external_row_id,entry.ranking_source_id,
                entry.score,entry.popularity,entry.epub_popularity,entry.reading_percentile,entry.epub_percentile
         FROM previous.source_entry entry JOIN entry_identity identity ON identity.source_id=entry.source_id AND identity.original_entry_id=entry.entry_id;
         INSERT INTO source_entry_source
         SELECT credit.source_id,identity.entry_id,credit.original_source_id
         FROM previous.source_entry_source credit JOIN entry_identity identity ON identity.source_id=credit.source_id AND identity.original_entry_id=credit.entry_id;
         DROP TABLE temp.entry_identity;",
            )
            .map_err(|error| error.to_string())?;
    }
    let audio: bool = target.query_row("SELECT EXISTS(SELECT 1 FROM previous.sqlite_schema WHERE name='book_audiobook_duration')", [], |row| row.get(0)).map_err(|error| error.to_string())?;
    if audio {
        target.execute_batch("INSERT INTO book_audiobook_duration SELECT * FROM previous.book_audiobook_duration").map_err(|error| error.to_string())?;
    }
    target.execute_batch("DETACH DATABASE previous").map_err(|error| error.to_string())?;
    finish_snapshot(&target)?;
    let bytes = target.serialize("main").map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(output).map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = remove_file(output);
        return Err(error.to_string());
    }
    eprintln!("{}: concept assignments, release {release}, format {FORMAT_VERSION}, {} bytes", output.display(), bytes.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::compact_snapshot;

    #[test]
    fn conversion_replaces_shared_lines_and_preserves_existing_integer_entry_ids() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("shared-lines.sqlite3");
        let output = dir.path().join("complete.sqlite3");
        let source = Connection::open(&input).unwrap();
        source.execute_batch(include_str!("../../../../client/library-backend/crates/library-database/tests/sql/catalog_shared_lines.sql")).unwrap();
        let text = "雪\r\nShared line\n\nEmbedded\0NUL\n";
        source.execute("INSERT INTO description VALUES(73,?1)", [text]).unwrap();
        source
            .execute_batch(
                "INSERT INTO snapshot VALUES(1,11,297); INSERT INTO source VALUES(1,'example.org');
             INSERT INTO book(row_id,key,external,title,description_id,format) VALUES(1,zeroblob(32),0,'Title',73,1);
             INSERT INTO book_concept VALUES(1,3915);
             INSERT INTO source_entry VALUES(1,1001,1,NULL,1,0.25,42,NULL,NULL,NULL),(1,9001,1,NULL,1,0.5,64,NULL,NULL,NULL);
             INSERT INTO source_entry_source VALUES(1,1001,1),(1,9001,1);",
            )
            .unwrap();
        let before = std::fs::read(&input).unwrap();
        compact_snapshot(&input, &output).unwrap();
        assert_eq!(std::fs::read(&input).unwrap(), before);
        let complete = Connection::open(&output).unwrap();
        assert_eq!(complete.query_row("SELECT text FROM description WHERE row_id=73", [], |row| row.get::<_, String>(0)).unwrap().as_bytes(), text.as_bytes());
        assert_eq!(complete.query_row("SELECT type FROM sqlite_schema WHERE name='description'", [], |row| row.get::<_, String>(0)).unwrap(), "table");
        assert_eq!(complete.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('description_record','description_line','description_line_order')", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        let ids = |db: &Connection| db.prepare("SELECT entry_id FROM source_entry ORDER BY entry_id").unwrap().query_map([], |row| row.get::<_, i64>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids(&complete), ids(&source));
        let credits = |db: &Connection| {
            db.prepare("SELECT source_id,entry_id,original_source_id FROM source_entry_source ORDER BY entry_id")
                .unwrap()
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(credits(&complete), credits(&source));
        let second = dir.path().join("complete-again.sqlite3");
        compact_snapshot(&output, &second).unwrap();
        let second = Connection::open(second).unwrap();
        assert_eq!(ids(&second), ids(&source));
        assert_eq!(second.query_row("SELECT text FROM description WHERE row_id=73", [], |row| row.get::<_, String>(0)).unwrap().as_bytes(), text.as_bytes());
        assert_eq!(second.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)).unwrap(), "ok");
    }

    #[test]
    fn conversion_preserves_records_codes_on_server_and_every_assignment() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("previous.sqlite3");
        let output = dir.path().join("compact.sqlite3");
        let source = Connection::open(&input).unwrap();
        source.execute_batch(include_str!("../../../../client/library-backend/crates/library-database/tests/sql/catalog_legacy.sql")).unwrap();
        source
            .execute_batch(
                "INSERT INTO snapshot VALUES(1,8,291); INSERT INTO source VALUES(1,'example.org');
             INSERT INTO description VALUES(1,'Description');
             INSERT INTO book VALUES(1,zeroblob(32),0,'Title',NULL,1,1900,1234,NULL,NULL,NULL,1);
             INSERT INTO book_concept VALUES(1,3915),(1,4034);
             INSERT INTO subject_projection VALUES(1,'original:149');
             CREATE TABLE book_audiobook_duration(book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id),duration_ms INTEGER NOT NULL);
             INSERT INTO book_audiobook_duration VALUES(1,12345);
             CREATE TABLE subject(id INTEGER PRIMARY KEY,name TEXT,code TEXT);
             INSERT INTO subject VALUES(1,'Original heading','PR4550');
             INSERT INTO source_entry VALUES(1,'01','https://example.org/a',1,NULL,1,0.25,42,NULL,NULL,NULL),
                                            (1,'1','https://example.org/b',1,NULL,1,0.5,64,NULL,NULL,NULL);
             INSERT INTO source_entry_source VALUES(1,'01',1),(1,'1',1);",
            )
            .unwrap();
        source.execute("INSERT INTO description VALUES(7,?1),(42,?2)", ["First\nShared line\n\n", "雪\nShared line\r\n"]).unwrap();
        let before = std::fs::read(&input).unwrap();
        compact_snapshot(&input, &output).unwrap();
        assert_eq!(std::fs::read(&input).unwrap(), before);
        assert_eq!(source.query_row("SELECT code FROM subject", [], |row| row.get::<_, String>(0)).unwrap(), "PR4550");
        let compact = Connection::open(&output).unwrap();
        assert_eq!(compact.query_row("SELECT format_version,release_id FROM snapshot", [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).unwrap(), (11, 291));
        assert_eq!(compact.prepare("SELECT concept_id FROM book_concept ORDER BY concept_id").unwrap().query_map([], |row| row.get::<_, i64>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap(), [3915, 4034]);
        assert_eq!(compact.query_row("SELECT title FROM book", [], |row| row.get::<_, String>(0)).unwrap(), "Title");
        assert_eq!(compact.query_row("SELECT duration_ms FROM book_audiobook_duration", [], |row| row.get::<_, i64>(0)).unwrap(), 12345);
        let read_descriptions =
            |db: &Connection| db.prepare("SELECT row_id,text FROM description ORDER BY row_id").unwrap().query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(read_descriptions(&compact), [(1, "Description".to_owned())]);
        let observations = compact
            .prepare("SELECT entry_id,score,popularity FROM source_entry ORDER BY entry_id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(observations, [(1, 0.25, 42), (2, 0.5, 64)]);
        assert_eq!(compact.query_row("SELECT count(*) FROM source_entry_source", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(source.query_row("SELECT entry_url FROM source_entry WHERE entry_id='01'", [], |row| row.get::<_, String>(0)).unwrap(), "https://example.org/a");
        assert_eq!(compact.query_row("SELECT count(*) FROM pragma_table_info('source_entry') WHERE name='entry_url'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        crate::publish::tests::assert_no_subject_evidence(&compact);
        assert_eq!(compact.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)).unwrap(), "ok");
        assert!(compact_snapshot(&input, &output).unwrap_err().contains("already exists"));
        for format in [9, 10] {
            source.execute("UPDATE snapshot SET format_version=?1", [format]).unwrap();
            let second = dir.path().join(format!("from-{format}.sqlite3"));
            compact_snapshot(&input, &second).unwrap();
            assert_eq!(read_descriptions(&Connection::open(second).unwrap()), read_descriptions(&compact));
        }
    }
}
