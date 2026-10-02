//! Pull-page commit: wire decoding plus one atomic apply transaction.

use std::collections::BTreeMap;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::sync::{PullCursorKey, SyncApplySql, invalid_integer_column, invalid_text_column, nonnegative_u64, parse_revision, parse_uuid_column, read_cursor_revision, storage_i64, write_cursor_revision};
pub(crate) use crate::{Database, DatabaseError};
pub(crate) use library_replica::{LibraryStateEntry, VersionKey};
pub(crate) use sync_common::{ReplicaSeq, PullStateResponse};

#[derive(Clone, Debug)]
pub(crate) struct StoredVersion(pub(crate) VersionKey);

impl StoredVersion {
    pub(crate) fn decode(changed_at: i64, conflict_rank: i64, actor: String, actor_seq: i64, columns: [usize; 4]) -> rusqlite::Result<Option<Self>> {
        if changed_at == 0 && actor.is_empty() && actor_seq == 0 {
            return Ok(None);
        }
        let changed_at = nonnegative_u64(changed_at, columns[0], "change timestamp")?;
        let conflict_rank = u8::try_from(conflict_rank).ok().and_then(|rank| sync_common::ConflictRank::try_from(rank).ok()).ok_or_else(|| invalid_integer_column(columns[1], "conflict rank is outside the protocol"))?;
        let actor_seq = ReplicaSeq::new(nonnegative_u64(actor_seq, columns[3], "actor sequence")?).map_err(|_| invalid_integer_column(columns[3], "actor sequence must be positive"))?;
        let actor = parse_uuid_column(actor, columns[2])?;
        Ok(Some(Self(VersionKey::new(changed_at, conflict_rank, actor, actor_seq))))
    }
}

pub(crate) fn read_version(conn: &rusqlite::Connection, kind: &str, key: &str, subkey: &str) -> Result<Option<VersionKey>, DatabaseError> {
    let mut stored = None;
    conn.read_sync_state_version_select(kind, key, subkey, |row| {
        stored = StoredVersion::decode(row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, [0, 1, 2, 3])?.map(|version| version.0);
        Ok(())
    })?;
    Ok(stored)
}

/// Decoded wire values and state identities only. Never snapshots database
/// state: winning versions and lifecycle checks still run under the writer.
pub(crate) struct PreparedRemoteChanges {
    pub(crate) changes: Vec<sync_common::Versioned<LibraryStateEntry>>,
}

pub(crate) fn sync_state_identity(body: &LibraryStateEntry) -> (&'static str, String, String) {
    let address = body.address();
    (body.kind(), address.entity_key, address.entity_subkey)
}

/// CPU-only book projection, built before the writer runs its
/// statements. Mirrors the enrichment write path: identical inputs project
/// identical rows.
#[derive(Clone)]
pub(crate) struct PreparedBook {
    pub(crate) metadata: book_model::BookMetadata,
    pub(crate) subject_codes: Vec<subject_projection::ProjectedSubjectCode>,
    pub(crate) concepts: Vec<(usize, i64)>,
    pub(crate) classification_rows: Vec<ClassificationRows>,
}

pub(crate) fn project_book(metadata: &book_model::BookMetadata) -> Result<PreparedBook, DatabaseError> {
    let mut timing = crate::timing::PhaseTimer::new("project_book");
    let invalid = |error: String| DatabaseError::message(format!("book projection failed: {error}"));
    let mut metadata = metadata.clone();
    metadata.infer_embedded_subject_isbns().map_err(|e| invalid(e.to_string()))?;
    timing.mark("clone_and_infer_isbns");
    let subject_codes = subject_projection::project_subject_codes(&metadata.subjects);
    timing.mark("subject_codes");
    let mut concepts = Vec::new();
    for (index, code) in subject_codes.iter().enumerate() {
        for path in subject_projection::unified_subject_paths(code.system_id(), code.code()) {
            concepts.push((index, subject_projection::unified_concept_id(&path).expect("projected subject exists in the taxonomy")));
        }
    }
    timing.mark("unified_concepts");
    let mut prepared = PreparedBook { metadata, subject_codes, concepts, classification_rows: Vec::new() };
    prepared.classification_rows = prepare_classification_rows(&prepared);
    timing.mark("classification_rows");
    Ok(prepared)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cell {
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

impl rusqlite::types::ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        use rusqlite::types::{ToSqlOutput, Value, ValueRef};
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(Value::Null),
            Self::Integer(v) => ToSqlOutput::Owned(Value::Integer(*v)),
            Self::Text(v) => ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes())),
        })
    }
}

#[derive(Clone)]
pub(crate) struct ClassificationRows {
    pub(crate) table: &'static str,
    pub(crate) keys: usize,
    pub(crate) width: usize,
    pub(crate) desired: BTreeMap<Vec<Cell>, Vec<Cell>>,
}

pub(crate) fn prepare_classification_rows(prepared: &PreparedBook) -> Vec<ClassificationRows> {
    let subjects = prepared.metadata.subjects.iter().enumerate().map(|(i, s)| vec![(i as i64).into(), s.name().into(), s.source().into(), s.authority().into(), s.code().into()]).collect();
    let mut concepts = Vec::new();
    let mut unified_evidence = Vec::new();
    for (index, concept) in &prepared.concepts {
        concepts.push(vec![(*concept).into(), subject_projection::UNIFIED_MATCHER_VERSION.into()]);
        let code = &prepared.subject_codes[*index];
        for pos in code.evidence_positions() {
            unified_evidence.push(vec![(*concept).into(), (*pos as i64).into(), code.system_id().into(), code.code().into()]);
        }
    }
    fn rows(table: &'static str, keys: usize, width: usize, rows: Vec<Vec<Cell>>) -> ClassificationRows {
        ClassificationRows { table, keys, width, desired: rows.into_iter().map(|v| (v[..keys].to_vec(), v)).collect() }
    }
    vec![rows("book_subject", 1, 5, subjects), rows("book_unified_concept", 1, 2, concepts), rows("book_unified_concept_evidence", 4, 4, unified_evidence)]
}

pub(crate) mod pull;

pub(crate) mod registers;

pub use crate::transactions::commit;

/// Reconcile derived subject rows without changing canonical metadata or other projections.
pub(crate) fn write_subject_projection(tx: &rusqlite::Transaction<'_>, book_id: i64, projected: &PreparedBook) -> Result<(), DatabaseError> {
    // Manual subject paths are legacy canonical evidence. Retain their stable
    // concept IDs across renames/reparenting without rewriting synchronized metadata.
    let mut adjusted;
    let projected = if projected.subject_codes.iter().any(|c| c.system_id() == subject_projection::UNIFIED_SYSTEM_ID) {
        adjusted = projected.clone();
        for (index, code) in adjusted.subject_codes.iter().enumerate() {
            if code.system_id() != subject_projection::UNIFIED_SYSTEM_ID {
                continue;
            }
            let mut retained = Vec::new();
            for position in code.evidence_positions() {
                let mut query = tx.prepare(include_str!("sql/manual_unified_concept_evidence.sql"))?;
                retained.extend(query.query_map(rusqlite::params![book_id, *position as i64, code.code()], |r| Ok((index, r.get::<_, i64>(0)?)))?.collect::<Result<Vec<_>, _>>()?);
            }
            if !retained.is_empty() {
                adjusted.concepts.retain(|(i, _)| *i != index);
                adjusted.concepts.extend(retained);
            }
        }
        adjusted.classification_rows = prepare_classification_rows(&adjusted);
        &adjusted
    } else {
        projected
    };

    {
        for rows in &projected.classification_rows {
            let (select_sql, delete_sql, insert_sql): (&str, &str, &str) = match rows.table {
                "book_subject" => (include_str!("sql/reconcile/book_subject_select.sql"), include_str!("sql/reconcile/book_subject_delete.sql"), include_str!("sql/reconcile/book_subject_insert.sql")),
                "book_unified_concept" => (include_str!("sql/reconcile/book_unified_concept_select.sql"), include_str!("sql/reconcile/book_unified_concept_delete.sql"), include_str!("sql/reconcile/book_unified_concept_insert.sql")),
                "book_unified_concept_evidence" => {
                    (include_str!("sql/reconcile/book_unified_concept_evidence_select.sql"), include_str!("sql/reconcile/book_unified_concept_evidence_delete.sql"), include_str!("sql/reconcile/book_unified_concept_evidence_insert.sql"))
                }
                _ => return Err(DatabaseError::message("unknown classification table")),
            };
            let existing: BTreeMap<Vec<Cell>, Vec<Cell>> = tx
                .prepare_cached(select_sql)
                .map_err(DatabaseError::operation)?
                .query_map([book_id], |r| {
                    (0..rows.width)
                        .map(|i| match r.get_ref(i)? {
                            rusqlite::types::ValueRef::Null => Ok(Cell::Null),
                            rusqlite::types::ValueRef::Integer(v) => Ok(Cell::Integer(v)),
                            rusqlite::types::ValueRef::Text(v) => Ok(Cell::Text(std::str::from_utf8(v).map_err(|error| invalid_text_column(i, error.to_string()))?.into())),
                            _ => Err(invalid_text_column(i, "unexpected classification value")),
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .map_err(DatabaseError::operation)?
                .collect::<Result<Vec<_>, rusqlite::Error>>()
                .map_err(DatabaseError::operation)?
                .into_iter()
                .map(|v| (v[..rows.keys].to_vec(), v))
                .collect();
            for key in existing.keys().filter(|key| !rows.desired.contains_key(*key)) {
                tx.prepare_cached(delete_sql).map_err(DatabaseError::operation)?.execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(key.iter().cloned()))).map_err(DatabaseError::operation)?;
            }
            for (key, values) in &rows.desired {
                if existing.get(key) == Some(values) {
                    continue;
                }
                tx.prepare_cached(insert_sql).map_err(DatabaseError::operation)?.execute(rusqlite::params_from_iter(std::iter::once(Cell::Integer(book_id)).chain(values.iter().cloned()))).map_err(DatabaseError::operation)?;
            }
        }
    }
    Ok(())
}
