use rusqlite::Connection;
use subject_projection::BISAC_MATCHER_VERSION;
use subject_projection::LCC_MATCHER_VERSION;
use subject_projection::LCC_SYSTEM_ID;
use subject_projection::LCC_VERSION;
use subject_projection::UNIFIED_MATCHER_VERSION;
use subject_projection::UNIFIED_SYSTEM_ID;
use subject_projection::bisac_subject_system;
use subject_projection::lcc_subject_path;
use sync_common::ContentHash;
use taxonomy_contract::TaxonomyCode;
use taxonomy_contract::TaxonomySystem;

pub(crate) fn seed_subject_systems(conn: &Connection) -> Result<(), rusqlite::Error> {
    let system = bisac_subject_system();
    let transaction = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    let stored_version = conn.query_row(include_str!("sql/seed_subject_systems_select.sql"), [system.id()], |row| row.get::<_, String>(0)).ok();
    let taxonomy_changed = stored_version.as_deref() != Some(system.version());
    if taxonomy_changed {
        conn.execute(include_str!("sql/seed_subject_systems_delete.sql"), [system.id()])?;
        conn.execute(include_str!("sql/seed_subject_systems_insert.sql"), rusqlite::params![system.id(), system.version(), BISAC_MATCHER_VERSION])?;
    } else {
        conn.execute(include_str!("sql/seed_subject_systems_update.sql"), rusqlite::params![system.id(), BISAC_MATCHER_VERSION])?;
    }
    if taxonomy_changed {
        let mut insert = conn.prepare_cached(include_str!("sql/seed_subject_systems_insert_2.sql"))?;
        for node in system.nodes() {
            insert.execute(rusqlite::params![system.id(), node.path(), node.parent_path().unwrap_or(""), node.name(), node.code(), i32::from(node.code().is_some())])?;
        }
    }
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    seed_lcc_subject_assignments(conn)?;
    seed_unified_subject_assignments(conn)
}

pub(crate) fn seed_lcc_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    let stored = conn.query_row(include_str!("sql/seed_lcc_subject_assignments_select.sql"), [LCC_SYSTEM_ID], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).ok();
    if stored.as_ref().is_some_and(|(version, matcher)| version == LCC_VERSION && *matcher == LCC_MATCHER_VERSION) {
        return Ok(());
    }

    let declarations = {
        let mut statement = conn.prepare(include_str!("sql/seed_lcc_subject_assignments_select_2.sql"))?;
        let rows = statement.query_map([LCC_SYSTEM_ID], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let transaction = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    conn.execute(include_str!("sql/seed_lcc_subject_assignments_delete.sql"), [LCC_SYSTEM_ID])?;
    conn.execute(include_str!("sql/seed_lcc_subject_assignments_insert.sql"), rusqlite::params![LCC_SYSTEM_ID, LCC_VERSION, LCC_MATCHER_VERSION])?;
    for (book_row_id, position, name, code) in declarations {
        let notation = code.as_deref().unwrap_or(&name);
        let subject_path = lcc_subject_path(notation).unwrap_or_else(|| format!("LCC / Unclassified / {}", notation.split_whitespace().collect::<Vec<_>>().join(" ")));
        let mut path = String::new();
        let mut parent_path = String::new();
        for component in subject_path.split(" / ") {
            if !path.is_empty() {
                path.push_str(" / ");
            }
            path.push_str(component);
            conn.execute(include_str!("sql/seed_lcc_subject_assignments_insert_2.sql"), rusqlite::params![LCC_SYSTEM_ID, path, parent_path, component])?;
            parent_path.clone_from(&path);
        }
        conn.execute(include_str!("sql/seed_lcc_subject_assignments_insert_3.sql"), rusqlite::params![book_row_id, LCC_SYSTEM_ID, subject_path, LCC_MATCHER_VERSION])?;
        conn.execute(include_str!("sql/seed_lcc_subject_assignments_insert_4.sql"), rusqlite::params![book_row_id, LCC_SYSTEM_ID, subject_path, position])?;
    }
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    Ok(())
}

pub(crate) fn seed_unified_subject_assignments(conn: &Connection) -> Result<(), rusqlite::Error> {
    // The bundled taxonomy is final. Existing book assignments are never
    // invalidated or replayed as a side effect of opening a library.
    conn.execute(include_str!("sql/seed_unified_subject_assignments_insert.sql"), rusqlite::params![UNIFIED_SYSTEM_ID, subject_projection::BUNDLED_UNIFIED_TAXONOMY_REVISION_ID, UNIFIED_MATCHER_VERSION])?;
    Ok(())
}

pub fn recorded_taxonomy_codes(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<TaxonomyCode>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/recorded_taxonomy_codes_select.sql"))?;
    let rows = statement.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut codes = Vec::new();
    for row in rows {
        let (system_id, code) = row?;
        let system = match system_id.as_str() {
            subject_projection::BISAC_SYSTEM_ID => TaxonomySystem::Bisac,
            subject_projection::LCC_SYSTEM_ID => TaxonomySystem::Lcc,
            _ => continue,
        };
        codes.push(TaxonomyCode { system, code });
    }
    Ok(codes)
}

pub fn recorded_taxonomy_codes_for_library(conn: &Connection) -> Result<Vec<TaxonomyCode>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/recorded_taxonomy_codes_for_library_select.sql"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut codes = Vec::new();
    for row in rows {
        let (system_id, code) = row?;
        let system = match system_id.as_str() {
            subject_projection::BISAC_SYSTEM_ID => TaxonomySystem::Bisac,
            subject_projection::LCC_SYSTEM_ID => TaxonomySystem::Lcc,
            _ => continue,
        };
        codes.push(TaxonomyCode { system, code });
    }
    Ok(codes)
}

pub(crate) fn source_subject_version(system_id: &str) -> &'static str {
    match system_id {
        subject_projection::BISAC_SYSTEM_ID => subject_projection::BISAC_VERSION,
        subject_projection::LCC_SYSTEM_ID => subject_projection::LCC_VERSION,
        _ => "external",
    }
}

pub(crate) fn source_mapping_type(system_id: &str) -> &'static str {
    match system_id {
        subject_projection::BISAC_SYSTEM_ID => "exact",
        subject_projection::LCC_SYSTEM_ID => "broader",
        _ => "related",
    }
}
