//! Contributor identity lookups and writes. Import supplies name-matching rules.
use crate::SqlParam;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::{DatabaseError, import::ImportCommitSql, sync::SyncApplySql};
pub(crate) use book_model::{AUTHOR_MARC_RELATOR_CODE, AgentId, Contributor, normalize_search_text};
use content_address::ContentHash;

pub(crate) type Identity = (i64, AgentId);
pub(crate) type StoredCredit = (i64, Contributor);

/// Import supplies its own name equivalence rule; storage does not normalize names.
pub type ContributorNameMatcher = fn(&str, &str) -> bool;

pub(crate) fn resolve_identity(tx: &rusqlite::Transaction<'_>, id: AgentId, name: &str, matcher: Option<ContributorNameMatcher>) -> Result<Identity, DatabaseError> {
    let mut existing: Option<(i64, String)> = None;
    tx.author_identity_by_stable_id(&id.to_string(), |row| {
        existing = Some((row.get(0)?, row.get(1)?));
        Ok(())
    })?;
    if existing.is_none()
        && let Some(matches) = matcher
    {
        tx.provisional_author_identities(|row| {
            if existing.is_none() && matches(name, &row.get::<_, String>(2)?) {
                existing = Some((row.get(0)?, row.get(1)?));
            }
            Ok(())
        })?;
    }
    match existing {
        Some((local, stable)) => Ok((local, AgentId::parse_str(&stable).map_err(|e| DatabaseError::message(format!("stored author identity is not readable: {e}")))?)),
        None => {
            tx.shared_authors_insert_author_identity(&id.to_string(), name)?;
            Ok((tx.last_insert_rowid(), id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_methods::query_rows;
    use crate::{Database, transactions::WriteOutcome};

    #[test]
    fn credit_resolution_uses_only_the_supplied_matching_rule() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.write(|tx| {
            let first = Contributor::new("Same Name", AUTHOR_MARC_RELATOR_CODE).unwrap();
            let other = Contributor::new("Same Name", AUTHOR_MARC_RELATOR_CODE).unwrap();
            let original = resolve_credits(tx, &[first], None)?;
            let reused = resolve_credits(tx, &[other.clone()], Some(|left, right| left == right))?;
            let independent = resolve_credits(tx, &[other], None)?;
            assert_eq!(original[0].1.agent_id(), reused[0].1.agent_id());
            assert_ne!(original[0].1.agent_id(), independent[0].1.agent_id());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cached_credit_writes_rebind_books_roles_and_empty_lists() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let first = ContentHash::new(&"a".repeat(64));
        let second = ContentHash::new(&"b".repeat(64));
        let author = Contributor::new("First Author", book_model::MarcRelatorCode(*b"aut")).unwrap();
        let editor = Contributor::new("Second Author", book_model::MarcRelatorCode(*b"edt")).unwrap();
        db.with_write_transaction(|tx| {
            for hash in [&first, &second] {
                tx.fixtures_insert_book(&hash)?;
            }
            let credits = resolve_credits(tx, &[author, editor], None)?;
            write_credits(tx, &first, &credits)?;
            write_credits(tx, &second, &credits[..1])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        db.with_write_transaction(|tx| {
            write_credits(tx, &first, &[])?;
            let replacement = Contributor::new("Replacement", book_model::MarcRelatorCode(*b"trl")).unwrap();
            let credits = resolve_credits(tx, &[replacement], None)?;
            write_credits(tx, &second, &credits)?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        let rows = query_rows(|callback| db.connection.fixtures_read_book_contributor_b_content_hash_c(callback), |row| Ok((row.get::<_, ContentHash>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows, vec![(second, 0, "Replacement".to_owned(), b"trl".to_vec())]);
    }
}

pub(crate) fn resolve_credits(tx: &rusqlite::Transaction<'_>, credits: &[Contributor], matcher: Option<ContributorNameMatcher>) -> Result<Vec<StoredCredit>, DatabaseError> {
    let mut stored = Vec::new();
    for credit in credits {
        let identity = resolve_identity(tx, credit.agent_id(), credit.name(), matcher)?;
        let credit = Contributor::with_id(identity.1, credit.name(), credit.role_code()).map_err(|e| DatabaseError::message(format!("contributor credit is not writable: {e}")))?;
        stored.push((identity.0, credit));
    }
    Ok(stored)
}

pub(crate) fn write_credits(tx: &rusqlite::Transaction<'_>, hash: &ContentHash, credits: &[StoredCredit]) -> Result<(), DatabaseError> {
    if !credits.is_empty() {
        for (position, (local, credit)) in credits.iter().enumerate() {
            tx.contributors_upsert_credit(&hash, &local, &(i64::try_from(position).unwrap_or(i64::MAX)), &(credit.name()), &(&credit.role_code().0))?;
        }
    }
    tx.contributors_trim_credits(&hash, &(i64::try_from(credits.len()).unwrap_or(i64::MAX)))?;
    let author = credits.iter().filter(|(_, credit)| credit.role_code() == AUTHOR_MARC_RELATOR_CODE).map(|(_, credit)| credit.name()).collect::<Vec<_>>().join(", ");
    tx.contributors_set_author(&hash, &author, &(normalize_search_text(&author)))?;
    Ok(())
}

include_sqlite_sql::include_sql!("src/sql/contributors.sql");
#[cfg(test)]
use crate::sql_fixtures::FixturesSql;
