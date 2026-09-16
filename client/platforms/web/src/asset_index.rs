//! Indexed logical asset names.

pub(super) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(include_str!("sql/asset_index/initialize_pragma.sql"))
}

pub(super) fn publish(conn: &rusqlite::Connection, name: &str, physical: &str, length: u64) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/asset_index/publish_insert.sql"),
        rusqlite::params![name, physical, length as i64])?;
    Ok(())
}

pub(super) fn live_physical_files(conn: &rusqlite::Connection, prefix: &str) -> rusqlite::Result<std::collections::HashSet<String>> {
    let upper = format!("{}0", prefix.strip_suffix('/').ok_or(rusqlite::Error::InvalidQuery)?);
    let mut query = conn.prepare(include_str!("sql/asset_index/live_physical_files_select.sql"))?;
    let rows = query.query_map(rusqlite::params![prefix, upper], |row| row.get(0))?;
    rows.collect()
}

pub(super) fn page(conn: &rusqlite::Connection, prefix: &str, after: &str) -> rusqlite::Result<Vec<(String, String)>> {
    // Callers use slash-terminated ASCII asset namespaces; the next character
    // after '/' provides an indexed exclusive upper bound for the namespace.
    let upper = format!("{}0", prefix.strip_suffix('/').ok_or(rusqlite::Error::InvalidQuery)?);
    let mut query = conn.prepare(include_str!("sql/asset_index/page_select.sql"))?;
    let rows = query.query_map(rusqlite::params![prefix, upper, after], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

/// Immutable generations record their length when published; totals never open files.
pub(super) fn total_bytes(conn: &rusqlite::Connection, prefix: &str) -> rusqlite::Result<u64> {
    let upper = format!("{}0", prefix.strip_suffix('/').ok_or(rusqlite::Error::InvalidQuery)?);
    conn.query_row(include_str!("sql/asset_index/total_bytes_select.sql"), rusqlite::params![prefix, upper], |row| row.get::<_, i64>(0)).map(|bytes| bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishing_records_replacement_and_preserves_existing_files() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        publish(&conn, "assets/a/book/existing", "keep", 2).unwrap();
        publish(&conn, "assets/a/book/new", "first", 3).unwrap();
        publish(&conn, "assets/a/book/new", "replacement", 5).unwrap();
        initialize(&conn).unwrap();
        assert_eq!(conn.query_row(include_str!("sql/asset_index/publishing_records_replacement_and_preserves_existing_files_select.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "replacement");
        assert_eq!(conn.query_row(include_str!("sql/asset_index/publishing_records_replacement_and_preserves_existing_files_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "keep");
        assert_eq!(total_bytes(&conn, "assets/a/book/").unwrap(), 7);
    }

    #[test]
    fn recorded_totals_follow_replacement_and_retirement_without_file_access() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/asset_index/recorded_totals_follow_replacement_and_retirement_without_file_access_create.sql")).unwrap();
        conn.execute_batch(include_str!("sql/asset_index/recorded_totals_follow_replacement_and_retirement_without_file_access_insert.sql")).unwrap();
        assert_eq!(total_bytes(&conn, "assets/a/book/").unwrap(), 8);
        conn.execute(include_str!("sql/asset_index/recorded_totals_follow_replacement_and_retirement_without_file_access_update.sql"), []).unwrap();
        assert_eq!(total_bytes(&conn, "assets/a/book/").unwrap(), 12);
        conn.execute(include_str!("sql/asset_index/recorded_totals_follow_replacement_and_retirement_without_file_access_delete.sql"), []).unwrap();
        assert_eq!(total_bytes(&conn, "assets/a/book/").unwrap(), 0);
    }

    #[test]
    fn live_physical_files_are_scoped_to_the_exact_import_prefix() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/asset_index/live_physical_files_are_scoped_to_the_exact_import_prefix_create.sql")).unwrap();
        for (name, physical) in [("first", Some("__import_objects/job/0")), ("alias", Some("__import_objects/job/0")), ("second", Some("__import_objects/job/1")), ("other", Some("__import_objects/job-other/0")), ("wildcard", Some("XXimport_objects/job/0")), ("deleted", None)] {
            conn.execute(include_str!("sql/asset_index/live_physical_files_are_scoped_to_the_exact_import_prefix_insert.sql"), rusqlite::params![name, physical]).unwrap();
        }
        assert_eq!(live_physical_files(&conn, "__import_objects/job/").unwrap(), ["__import_objects/job/0".to_owned(), "__import_objects/job/1".to_owned()].into_iter().collect());
        assert!(live_physical_files(&conn, "__import_objects/absent/").unwrap().is_empty());
        assert!(live_physical_files(&conn, "__import_objects/job").is_err());
    }

    fn insert_asset(conn: &rusqlite::Connection, name: &str) -> rusqlite::Result<()> {
        conn.execute(include_str!("sql/asset_index/insert_asset_insert.sql"), rusqlite::params![name, format!("__asset_objects/{name}")])?;
        Ok(())
    }

    #[test]
    fn pages_preserve_tombstones_and_do_not_skip_after_deletion() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/asset_index/pages_preserve_tombstones_and_do_not_skip_after_deletion_create.sql")).unwrap();
        for index in 0..70 {
            insert_asset(&conn, &format!("assets/library/book/{index:03}")).unwrap();
        }
        insert_asset(&conn, "assets/library/thumbnail/cover").unwrap();
        insert_asset(&conn, "assets/other/book/000").unwrap();
        conn.execute(include_str!("sql/asset_index/pages_preserve_tombstones_and_do_not_skip_after_deletion_update.sql"), []).unwrap();
        conn.execute(include_str!("sql/asset_index/pages_preserve_tombstones_and_do_not_skip_after_deletion_update_2.sql"), []).unwrap();
        assert_eq!(conn.query_row(include_str!("sql/asset_index/pages_preserve_tombstones_and_do_not_skip_after_deletion_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "__asset_objects/new-generation");
        let first = page(&conn, "assets/library/book/", "").unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(first[0], ("assets/library/book/001".into(), "__asset_objects/new-generation".into()));
        assert_eq!(first[31].0, "assets/library/book/032");
        conn.execute(include_str!("sql/asset_index/pages_preserve_tombstones_and_do_not_skip_after_deletion_delete.sql"), [&first[31].0]).unwrap();
        let second = page(&conn, "assets/library/book/", &first[31].0).unwrap();
        assert_eq!(second.len(), 32);
        assert_eq!(second[0].0, "assets/library/book/033");
        let third = page(&conn, "assets/library/book/", &second[31].0).unwrap();
        assert_eq!(third.len(), 5);
        assert_eq!(third[4].0, "assets/library/book/069");
        assert!(page(&conn, "assets/library/book/", &third[4].0).unwrap().is_empty());
    }
}
