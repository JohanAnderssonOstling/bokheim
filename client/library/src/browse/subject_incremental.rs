//! Update changed books and their ancestors; unrelated branches stay materialized.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Contents {
    subjects: BTreeSet<String>,
    books: BTreeSet<i64>,
}

pub(super) fn update(conn: &Connection) -> rusqlite::Result<()> {
    let tx = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    let (revision, built, missing_root): (i64, i64, bool) = conn.query_row(include_str!("sql/subject_incremental/update_select.sql"), [ROOT_SUBJECT_PATH], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let pending: bool = conn.query_row(include_str!("sql/subject_incremental/update_select_2.sql"), [], |r| r.get(0))?;
    if revision == built && !pending && !missing_root {
        return Ok(());
    }
    conn.execute(include_str!("sql/subject_incremental/update_update.sql"), [])?;
    let full = missing_root || built < 0 || (revision != built && !pending);
    if full {
        conn.execute_batch(include_str!("sql/subject_incremental/update_insert.sql"))?;
    }
    let books = conn.prepare(include_str!("sql/subject_incremental/update_select_3.sql"))?.query_map([], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut affected = BTreeSet::new();
    for book in books {
        let old = conn.prepare(include_str!("sql/subject_incremental/update_select_4.sql"))?.query_map([book], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
        let mut routes = conn
            .prepare(include_str!("sql/subject_compaction/book_routes.sql"))?
            .query_map([book], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if routes.iter().any(|r| r.1 == ROOT_SUBJECT_PATH) && !routes.iter().any(|r| r.0 == ROOT_SUBJECT_PATH) {
            routes.push((ROOT_SUBJECT_PATH.to_owned(), String::new(), -1, ROOT_SUBJECT_PATH.to_owned(), 0));
        }
        if routes.iter().any(|r| r.1.is_empty()) && !routes.iter().any(|r| r.0.is_empty()) {
            routes.push((String::new(), String::new(), -1, String::new(), 0));
        }
        let new = routes.iter().map(|r| (r.0.clone(), r.4)).collect::<BTreeMap<_, _>>();
        if old != new {
            affected.extend(old.keys().chain(new.keys()).cloned());
            conn.execute(include_str!("sql/subject_incremental/update_delete.sql"), [book])?;
            for (path, parent, concept, label, direct) in routes {
                conn.execute(include_str!("sql/subject_incremental/update_insert_2.sql"), rusqlite::params![path, parent, concept, label])?;
                conn.execute(include_str!("sql/subject_incremental/update_insert_3.sql"), rusqlite::params![path, book, direct])?;
            }
        }
        conn.execute(include_str!("sql/subject_incremental/update_delete_2.sql"), [book])?;
        conn.execute(include_str!("sql/subject_compaction/book_ownership.sql"), [book])?;
        conn.execute(include_str!("sql/subject_incremental/update_insert_4.sql"), [book])?;
    }
    // Update descendant counts before exposing each actual parent-child edge.
    let mut affected = affected.into_iter().collect::<Vec<_>>();
    affected.sort_by_key(|path| std::cmp::Reverse(subject_path_depth(path)));
    for path in affected {
        let mut contents = Contents { subjects: BTreeSet::new(), books: conn.prepare(include_str!("sql/subject_incremental/update_select_6.sql"))?.query_map([&path], |r| r.get(0))?.collect::<rusqlite::Result<_>>()? };
        let children = conn.prepare(include_str!("sql/subject_incremental/update_select_7.sql"))?.query_map([&path], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for child in children {
            contents.subjects.insert(child);
        }
        conn.execute(include_str!("sql/subject_incremental/update_delete_3.sql"), [&path])?;
        conn.execute(include_str!("sql/subject_incremental/update_delete_4.sql"), [&path])?;
        for child in &contents.subjects {
            conn.execute(include_str!("sql/subject_incremental/update_insert_5.sql"), rusqlite::params![path, child])?;
        }
        for book in &contents.books {
            conn.execute(include_str!("sql/subject_incremental/update_insert_6.sql"), rusqlite::params![path, book])?;
        }
        conn.execute(include_str!("sql/subject_incremental/update_delete_5.sql"), [&path])?;
        conn.execute(include_str!("sql/subject_incremental/update_delete_6.sql"), [&path])?;
        for child in &contents.subjects {
            conn.execute(include_str!("sql/subject_incremental/update_insert_7.sql"), rusqlite::params![path, child])?;
        }
        for book in &contents.books {
            conn.execute(include_str!("sql/subject_incremental/update_insert_8.sql"), rusqlite::params![path, book])?;
        }
        conn.execute(include_str!("sql/subject_incremental/update_insert_9.sql"), [&path])?;
        conn.execute(include_str!("sql/subject_incremental/update_delete_7.sql"), [&path])?;
    }
    conn.execute_batch(include_str!("sql/subject_incremental/update_delete_8.sql"))?;
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(())
}
