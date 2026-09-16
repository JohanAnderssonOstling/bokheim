//! Reconcile per-book rows in FK order, preserving unchanged keys and evidence.
use super::*;
use rusqlite::types::{ToSql, ToSqlOutput, Value, ValueRef};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Null,
    Integer(i64),
    Text(String),
}
impl From<i64> for Cell {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}
impl From<&str> for Cell {
    fn from(v: &str) -> Self {
        Self::Text(v.into())
    }
}
impl From<Option<&str>> for Cell {
    fn from(v: Option<&str>) -> Self {
        v.map_or(Self::Null, Self::from)
    }
}
impl ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(Value::Null),
            Self::Integer(v) => ToSqlOutput::Owned(Value::Integer(*v)),
            Self::Text(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
        })
    }
}

// Identifiers are static internal schema names, never publication input. Columns
// start with the primary-key suffix (book_row_id is supplied separately).
fn reconcile(tx: &Transaction, row: i64, table: &str, columns: &[&str], keys: usize, desired: &BTreeMap<Vec<Cell>, Vec<Cell>>) -> rusqlite::Result<()> {
    let (select_sql, delete_sql, insert_sql) = match table {
        "book_subject" => (include_str!("sql/classification_delta/book_subject_select.sql"), include_str!("sql/classification_delta/book_subject_delete.sql"), include_str!("sql/classification_delta/book_subject_insert.sql")),
        "book_subject_code" => {
            (include_str!("sql/classification_delta/book_subject_code_select.sql"), include_str!("sql/classification_delta/book_subject_code_delete.sql"), include_str!("sql/classification_delta/book_subject_code_insert.sql"))
        }
        "book_subject_assignment" => (
            include_str!("sql/classification_delta/book_subject_assignment_select.sql"),
            include_str!("sql/classification_delta/book_subject_assignment_delete.sql"),
            include_str!("sql/classification_delta/book_subject_assignment_insert.sql"),
        ),
        "book_subject_assignment_evidence" => (
            include_str!("sql/classification_delta/book_subject_assignment_evidence_select.sql"),
            include_str!("sql/classification_delta/book_subject_assignment_evidence_delete.sql"),
            include_str!("sql/classification_delta/book_subject_assignment_evidence_insert.sql"),
        ),
        "book_unified_concept" => {
            (include_str!("sql/classification_delta/book_unified_concept_select.sql"), include_str!("sql/classification_delta/book_unified_concept_delete.sql"), include_str!("sql/classification_delta/book_unified_concept_insert.sql"))
        }
        "book_unified_concept_evidence" => (
            include_str!("sql/classification_delta/book_unified_concept_evidence_select.sql"),
            include_str!("sql/classification_delta/book_unified_concept_evidence_delete.sql"),
            include_str!("sql/classification_delta/book_unified_concept_evidence_insert.sql"),
        ),
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let mut select = tx.prepare_cached(select_sql)?;
    let existing = select
        .query_map([row], |r| {
            (0..columns.len())
                .map(|i| match r.get_ref(i)? {
                    ValueRef::Null => Ok(Cell::Null),
                    ValueRef::Integer(v) => Ok(Cell::Integer(v)),
                    ValueRef::Text(v) => Ok(Cell::Text(std::str::from_utf8(v).map_err(|e| invalid_text_column(i, e.to_string()))?.into())),
                    _ => Err(invalid_text_column(i, "unexpected classification value")),
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let existing: BTreeMap<_, _> = existing.into_iter().map(|v| (v[..keys].to_vec(), v)).collect();
    let mut delete = tx.prepare_cached(delete_sql)?;
    for key in existing.keys().filter(|key| !desired.contains_key(*key)) {
        delete.execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(row)).chain(key.iter().cloned())))?;
    }
    let mut insert = tx.prepare_cached(insert_sql)?;
    for (key, values) in desired {
        if existing.get(key) == Some(values) {
            continue;
        }
        insert.execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(row)).chain(values.iter().cloned())))?;
    }
    Ok(())
}

pub(super) struct PreparedRows {
    table: &'static str,
    columns: &'static [&'static str],
    keys: usize,
    desired: BTreeMap<Vec<Cell>, Vec<Cell>>,
}
impl PreparedRows {
    fn new(table: &'static str, columns: &'static [&'static str], keys: usize, rows: Vec<Vec<Cell>>) -> Self {
        Self { table, columns, keys, desired: rows.into_iter().map(|v| (v[..keys].to_vec(), v)).collect() }
    }
}
pub(super) fn prepare(p: &PreparedPublication) -> Vec<PreparedRows> {
    let subjects = p.metadata.subjects.iter().enumerate().map(|(i, s)| vec![(i as i64).into(), s.name().into(), s.source().into(), s.authority().into(), s.code().into()]).collect();
    let mut codes = Vec::new();
    for c in &p.subject_codes {
        for pos in c.evidence_positions() {
            codes.push(vec![(*pos as i64).into(), c.system_id().into(), c.code().into()]);
        }
    }
    let mut assignments = Vec::new();
    let mut evidence = Vec::new();
    for a in &p.assignments {
        assignments.push(vec![a.system_id().into(), a.subject_path().into(), a.matcher_version().into()]);
        for pos in a.evidence_positions() {
            evidence.push(vec![a.system_id().into(), a.subject_path().into(), (*pos as i64).into()]);
        }
    }
    let mut concepts = Vec::new();
    let mut unified_evidence = Vec::new();
    for (index, concept, _) in &p.concepts {
        concepts.push(vec![(*concept).into(), subject_projection::UNIFIED_MATCHER_VERSION.into()]);
        let c = &p.subject_codes[*index];
        for pos in c.evidence_positions() {
            unified_evidence.push(vec![(*concept).into(), (*pos as i64).into(), c.system_id().into(), c.code().into()]);
        }
    }
    vec![
        PreparedRows::new("book_subject", &["position", "name", "source", "authority", "code"], 1, subjects),
        PreparedRows::new("book_subject_code", &["subject_position", "system_id", "code"], 3, codes),
        PreparedRows::new("book_subject_assignment", &["system_id", "subject_path", "matcher_version"], 2, assignments),
        PreparedRows::new("book_subject_assignment_evidence", &["system_id", "subject_path", "subject_position"], 3, evidence),
        PreparedRows::new("book_unified_concept", &["concept_id", "mapper_version"], 1, concepts),
        PreparedRows::new("book_unified_concept_evidence", &["concept_id", "subject_position", "source_system_id", "source_code"], 4, unified_evidence),
    ]
}
pub(super) fn apply(tx: &Transaction, row: i64, prepared: &PreparedPublication) -> rusqlite::Result<()> {
    for a in &prepared.assignments {
        if a.system_id() != subject_projection::BISAC_SYSTEM_ID {
            ensure_external_subject_nodes(tx, a.system_id(), a.subject_path(), a.matcher_version())?;
        }
    }
    for (index, concept, path) in &prepared.concepts {
        let code = &prepared.subject_codes[*index];
        let version = source_subject_version(code.system_id());
        tx.execute(include_str!("sql/classification_delta/apply_insert.sql"), rusqlite::params![code.system_id(), version, code.code()])?;
        tx.execute(include_str!("sql/classification_delta/apply_insert_2.sql"), rusqlite::params![code.system_id(), version, code.code(), concept, source_mapping_type(code.system_id()), subject_projection::UNIFIED_MATCHER_VERSION])?;
        tx.execute(include_str!("sql/classification_delta/apply_insert_3.sql"), rusqlite::params![code.system_id(), code.code(), path, subject_projection::UNIFIED_MATCHER_VERSION])?;
    }
    for rows in &prepared.classification_rows {
        reconcile(tx, row, rows.table, rows.columns, rows.keys, &rows.desired)?;
    }
    Ok(())
}
