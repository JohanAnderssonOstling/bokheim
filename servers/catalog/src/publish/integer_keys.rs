//! One-time conversion of the previous snapshot to binary keys and shared subject labels.
use super::{finish_snapshot, FORMAT_VERSION, PAGE_SIZE, SCHEMA};
use rusqlite::{params, Connection};
use std::path::Path;

pub(super) fn copy_normalized(input: &Path, output: &Path, release: i64) -> Result<(), String> {
    let mut target = Connection::open(output).map_err(|error| error.to_string())?;
    target.pragma_update(None, "page_size", PAGE_SIZE).map_err(|error| error.to_string())?;
    target.execute("ATTACH DATABASE ?1 AS previous", [input.to_string_lossy().as_ref()]).map_err(|error| error.to_string())?;
    let tx = target.transaction().map_err(|error| error.to_string())?;
    tx.execute_batch(SCHEMA).map_err(|error| error.to_string())?;
    tx.execute("INSERT INTO snapshot VALUES(1,?1,?2)", params![FORMAT_VERSION, release]).map_err(|error| error.to_string())?;
    tx.execute_batch("INSERT INTO source SELECT * FROM previous.source;").map_err(|error| error.to_string())?;
    let copy = "INSERT INTO description SELECT * FROM previous.description;
        INSERT INTO book(row_id,key,external,title,subtitle,description_id,year,file_bytes,source_id,entry_url,download_url,format)
            SELECT row_id,unhex(key),external,title,subtitle,description_id,year,file_bytes,source_id,entry_url,download_url,format
            FROM previous.book ORDER BY row_id;
        INSERT INTO author SELECT * FROM previous.author;
        INSERT INTO subject_source(name) SELECT DISTINCT source FROM previous.subject ORDER BY source;
        INSERT INTO subject_authority(name) SELECT DISTINCT authority FROM previous.subject WHERE authority IS NOT NULL ORDER BY authority;
        INSERT INTO subject SELECT subject_id,subject.id,subject.name,sort_name,source.id,authority.id,code
            FROM previous.subject subject JOIN subject_source source ON source.name=subject.source
            LEFT JOIN subject_authority authority ON authority.name=subject.authority ORDER BY subject_id;
        INSERT INTO book_author SELECT * FROM previous.book_author;
        INSERT INTO book_language SELECT * FROM previous.book_language;
        INSERT INTO book_identifier SELECT * FROM previous.book_identifier;
        INSERT INTO book_subject SELECT * FROM previous.book_subject;
        INSERT INTO book_source SELECT * FROM previous.book_source;
        INSERT INTO source_entry SELECT * FROM previous.source_entry;
        INSERT INTO source_entry_source SELECT * FROM previous.source_entry_source;";
    tx.execute_batch(copy).map_err(|error| error.to_string())?;
    let cached = tx
        .prepare("SELECT name,sql FROM previous.sqlite_schema WHERE type='table' AND name LIKE 'catalog_browse_%'")
        .map_err(|error| error.to_string())?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for (name, sql) in &cached {
        tx.execute_batch(sql).map_err(|error| error.to_string())?;
        tx.execute_batch(&format!("INSERT INTO {name} SELECT * FROM previous.{name}")).map_err(|error| error.to_string())?;
    }
    let compact = cached.iter().any(|(name, _)| name == "catalog_browse_search");
    if compact {
        // Integer book/author IDs remain stable; only the key representation changes.
        let views = tx
            .prepare("SELECT sql FROM previous.sqlite_schema WHERE type='view' AND name LIKE 'catalog_browse_%'")
            .map_err(|error| error.to_string())?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for sql in views {
            tx.execute_batch(&sql.replace("unhex(source.key)", "source.key")).map_err(|error| error.to_string())?;
        }
    } else if cached.iter().any(|(name, _)| name == "catalog_browse_book") {
        tx.execute_batch(
            "CREATE VIEW catalog_browse_author AS SELECT author_id,name FROM author;
            CREATE VIEW catalog_browse_author_book AS SELECT browse.row_id AS book_row_id,author.author_id FROM book_author credit
                JOIN author ON author.row_id=credit.author_row_id JOIN book source ON source.row_id=credit.book_row_id JOIN catalog_browse_book browse ON browse.key=source.key;
            CREATE VIEW catalog_browse_language AS SELECT DISTINCT browse.row_id AS book_row_id,
                lower(CASE WHEN instr(language,'-')>0 THEN substr(language,1,instr(language,'-')-1) ELSE language END) AS language
                FROM book_language credit JOIN book source ON source.row_id=credit.book_row_id JOIN catalog_browse_book browse ON browse.key=source.key;",
        )
        .map_err(|error| error.to_string())?;
    }
    if compact || cached.iter().any(|(name, _)| name == "catalog_browse_book") {
        tx.execute_batch(
            "CREATE VIEW catalog_hosted AS SELECT key AS identity,title,subtitle,description.text AS description,year
                FROM book JOIN description ON description.row_id=book.description_id WHERE external=0;
            CREATE VIEW catalog_external AS SELECT key AS identity,title,subtitle,description.text AS description,year,entry_url AS external_url,download_url,format
                FROM book JOIN description ON description.row_id=book.description_id WHERE external=1;",
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())?;
    target.execute_batch("DETACH DATABASE previous").map_err(|error| error.to_string())?;
    finish_snapshot(&target)?;
    eprintln!("{}: integer book keys, release {release}, format {FORMAT_VERSION}, {} bytes", output.display(), std::fs::metadata(output).map_err(|error| error.to_string())?.len());
    Ok(())
}
