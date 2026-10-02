//! Shared identity primitives; import may reuse provisional names, sync must not.
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::sync::parse_author_name_opt;
pub(crate) use crate::{DatabaseError, import::ImportCommitSql, sync::SyncApplySql};
pub(crate) use book_model::{AUTHOR_MARC_RELATOR_CODE, AgentId, AuthorName, Contributor, PublisherCredit, normalize_search_text};

pub(crate) type Identity = (i64, AgentId);
pub(crate) type StoredCredit = (i64, Contributor);

#[derive(Clone, Copy)]
pub(crate) enum IdentityPolicy {
    StableOnly,
    ReuseProvisional,
}

pub(crate) fn resolve_identity(tx: &rusqlite::Transaction<'_>, id: AgentId, name: &AuthorName, policy: IdentityPolicy) -> Result<Identity, DatabaseError> {
    let mut existing: Option<(i64, String)> = None;
    tx.author_identity_by_stable_id(&id.to_string(), |row| {
        existing = Some((row.get(0)?, row.get(1)?));
        Ok(())
    })?;
    if existing.is_none() && matches!(policy, IdentityPolicy::ReuseProvisional) {
        tx.provisional_author_identity_by_name(name.match_key().as_str(), |row| {
            existing = Some((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
    }
    match existing {
        Some((local, stable)) => Ok((local, AgentId::parse_str(&stable).map_err(|e| DatabaseError::message(format!("stored author identity is not readable: {e}")))?)),
        None => {
            tx.shared_authors_insert_author_identity(&id.to_string(), name.match_key().as_str())?;
            Ok((tx.last_insert_rowid(), id))
        }
    }
}

pub(crate) fn resolve_publishers(tx: &rusqlite::Transaction<'_>, publishers: &[PublisherCredit], policy: IdentityPolicy) -> Result<Vec<PublisherCredit>, DatabaseError> {
    publishers
        .iter()
        .map(|publisher| {
            let name = parse_author_name_opt(publisher.name().as_str())?.ok_or_else(|| DatabaseError::message("publisher name did not produce an agent name"))?;
            let (_, stable) = resolve_identity(tx, publisher.agent_id(), &name, policy)?;
            PublisherCredit::with_id(stable, publisher.name().as_str()).map_err(|e| DatabaseError::message(format!("publisher credit is not writable: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Database, transactions::WriteOutcome};

    #[test]
    fn cached_credit_writes_rebind_books_roles_and_empty_lists() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let first = "a".repeat(64);
        let second = "b".repeat(64);
        let author = Contributor::new("First Author", book_model::MarcRelatorCode(*b"aut")).unwrap();
        let editor = Contributor::new("Second Author", book_model::MarcRelatorCode(*b"edt")).unwrap();
        db.with_write_transaction(|tx| {
            for hash in [&first, &second] {
                tx.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES (?1,'Book',1,1)", [hash])?;
            }
            let credits = resolve_credits(tx, &[author, editor])?;
            write_credits(tx, &first, &credits)?;
            write_credits(tx, &second, &credits[..1])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        db.with_write_transaction(|tx| {
            write_credits(tx, &first, &[])?;
            let replacement = Contributor::new("Replacement", book_model::MarcRelatorCode(*b"trl")).unwrap();
            let credits = resolve_credits(tx, &[replacement])?;
            write_credits(tx, &second, &credits)?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        let rows = db
            .connection
            .prepare("SELECT b.content_hash,c.position,c.name,c.role FROM book_contributor c JOIN book b ON b.row_id=c.book_row_id ORDER BY b.content_hash,c.position")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows, vec![(second, 0, "Replacement".to_owned(), b"trl".to_vec())]);
    }

    #[test]
    fn import_reuses_names_but_sync_preserves_distinct_stable_identities() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.with_write_transaction(|tx| {
            let name = AuthorName::parse("Same Name").unwrap();
            let first = AgentId::new_v4();
            let second = AgentId::new_v4();
            let original = resolve_identity(tx, first, &name, IdentityPolicy::StableOnly)?;
            assert_eq!(resolve_identity(tx, second, &name, IdentityPolicy::ReuseProvisional)?, original);
            let separate = resolve_identity(tx, second, &name, IdentityPolicy::StableOnly)?;
            assert_ne!(separate.0, original.0);
            assert_eq!(separate.1, second);
            // Stable IDs take precedence even when another provisional name matches.
            assert_eq!(resolve_identity(tx, second, &name, IdentityPolicy::ReuseProvisional)?, separate);
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
    }
}

pub(crate) fn resolve_credits(tx: &rusqlite::Transaction<'_>, credits: &[Contributor]) -> Result<Vec<StoredCredit>, DatabaseError> {
    let mut stored = Vec::new();
    for credit in credits {
        let Some(name) = parse_author_name_opt(credit.name())? else { continue };
        let identity = resolve_identity(tx, credit.agent_id(), &name, IdentityPolicy::StableOnly)?;
        let credit = Contributor::with_id(identity.1, credit.name(), credit.role_code()).map_err(|e| DatabaseError::message(format!("contributor credit is not writable: {e}")))?;
        stored.push((identity.0, credit));
    }
    Ok(stored)
}

pub(crate) fn write_credits(tx: &rusqlite::Transaction<'_>, hash: &str, credits: &[StoredCredit]) -> Result<(), DatabaseError> {
    if !credits.is_empty() {
        let mut statement = tx.prepare_cached(include_str!("sql/upsert_contributor.sql"))?;
        for (position, (local, credit)) in credits.iter().enumerate() {
            statement.execute(rusqlite::named_params! {
                ":content_hash": hash, ":author_identity_id": local,
                ":position": i64::try_from(position).unwrap_or(i64::MAX),
                ":name": credit.name(), ":role": &credit.role_code().0,
            })?;
        }
    }
    tx.prepare_cached(include_str!("sql/trim_contributors.sql"))?.execute(rusqlite::named_params! {
        ":content_hash": hash, ":position": i64::try_from(credits.len()).unwrap_or(i64::MAX),
    })?;
    let author = credits.iter().filter(|(_, credit)| credit.role_code() == AUTHOR_MARC_RELATOR_CODE).map(|(_, credit)| credit.name()).collect::<Vec<_>>().join(", ");
    tx.execute("UPDATE book SET author=?2,search_author=?3 WHERE content_hash=?1", rusqlite::params![hash, author, normalize_search_text(&author)])?;
    Ok(())
}
