//! Stable author identities and the ordered, role-tagged contributor list
//! attached to book metadata. `book_contributor` is one flat per-book
//! position sequence shared by every role (author, translator, editor, ...).
use rusqlite::OptionalExtension;

use crate::metadata::MetadataSql;
#[cfg(test)]
use crate::schema::init_db;
use crate::storage_values::invalid_text_column;
use crate::storage_values::unix_millis;
use include_sqlite_sql::include_sql;
use book_model::AgentId;
use book_model::AuthorId;
use catalog_model::AuthorName;
use book_model::Contributor;
use book_model::ExternalAuthorId;
use book_model::BookMetadata;
use book_model::PublisherCredit;
use rusqlite::Transaction;
use std::collections::HashMap;
use sync_common::ContentHash;

pub(super) struct StoredContributor {
    pub(super) identity_local_id: i64,
    pub(super) credit: Contributor,
}

/// Owned by one write batch only. Legacy identity mutations explicitly evict
/// affected entries before another operation can reuse their integer IDs.
#[derive(Default)]
pub(super) struct IdentityCache {
    agents: HashMap<AgentId, i64>,
    publishers: HashMap<String, i64>,
    authorities: HashMap<(String, String), (AgentId, String)>,
    #[cfg(test)]
    pub(super) misses: usize,
}
impl IdentityCache {
    pub(super) fn invalidate(&mut self) {
        self.agents.clear();
        self.publishers.clear();
        self.authorities.clear();
    }
    #[cfg(test)]
    pub(super) fn forget(&mut self, id: AgentId) {
        self.agents.remove(&id);
    }
    fn agent(&mut self, tx: &Transaction, id: AgentId, name: &AuthorName) -> rusqlite::Result<i64> {
        if let Some(row) = self.agents.get(&id) {
            return Ok(*row);
        }
        #[cfg(test)]
        {
            self.misses += 1;
        }
        let (row, _) = get_or_create_author_identity(tx, name, id, AuthorIdPolicy::PreserveIncoming)?;
        self.agents.insert(id, row);
        Ok(row)
    }
    pub(super) fn publisher(&mut self, tx: &Transaction, publisher: &PublisherCredit) -> rusqlite::Result<(i64, i64)> {
        let name = AuthorName::parse(publisher.name().as_str()).map_err(|e| invalid_text_column(0, e.to_string()))?;
        let agent = self.agent(tx, publisher.agent_id(), &name)?;
        let key = publisher.name().match_key().as_str().to_owned();
        let row = if let Some(row) = self.publishers.get(&key) {
            *row
        } else {
            #[cfg(test)]
            {
                self.misses += 1;
            }
            tx.insert_publisher(publisher.name().as_str(), &key)?;
            let row = tx.query_row(include_str!("sql/authors/publisher_select.sql"), [&key], |r| r.get(0))?;
            self.publishers.insert(key, row);
            row
        };
        Ok((row, agent))
    }
    pub(super) fn contributors(&mut self, tx: &Transaction, credits: &[Contributor]) -> rusqlite::Result<Vec<StoredContributor>> {
        credits
            .iter()
            .map(|credit| {
                let name = AuthorName::parse(credit.name()).map_err(|e| invalid_text_column(0, e.to_string()))?;
                Ok(StoredContributor { identity_local_id: self.agent(tx, credit.agent_id(), &name)?, credit: credit.clone() })
            })
            .collect()
    }
    pub(super) fn authoritative(&mut self, tx: &Transaction, name: &str, external: &ExternalAuthorId) -> rusqlite::Result<Contributor> {
        let key = (external.authority().as_str().to_owned(), external.value().to_owned());
        let name = AuthorName::parse(name).map_err(|e| invalid_text_column(0, e.to_string()))?;
        let cached = self.authorities.get(&key).cloned();
        let id = if let Some((id, _)) = &cached {
            *id
        } else {
            let existing: Option<(i64, String)> = tx.query_row(include_str!("sql/authors/authoritative_select.sql"), rusqlite::params![key.0, key.1], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let (row, id) = if let Some((row, raw)) = existing {
                (row, AgentId::parse_str(&raw).map_err(|e| invalid_text_column(1, e.to_string()))?)
            } else {
                let id = book_model::agent_id_from_migration_seed(format!("external-author:{}:{}", key.0, key.1).as_bytes());
                (self.agent(tx, id, &name)?, id)
            };
            self.agents.insert(id, row);
            tx.execute(include_str!("sql/authors/authoritative_insert.sql"), rusqlite::params![row, key.0, key.1, unix_millis()?])?;
            id
        };
        if cached.as_ref().is_none_or(|(_, previous)| previous != name.as_str()) {
            let row = self.agent(tx, id, &name)?;
            tx.execute(include_str!("sql/authors/authoritative_update.sql"), rusqlite::params![name.as_str(), name.match_key().as_str(), row])?;
            self.authorities.insert(key, (id, name.as_str().into()));
        }
        Contributor::with_id(id, name.as_str(), None, book_model::MarcRelatorCode(*b"aut")).map_err(|e| invalid_text_column(0, e.to_string()))
    }
}

#[derive(Clone, Copy)]
pub(super) enum AuthorIdPolicy {
    ReuseProvisionalNameCandidate,
    PreserveIncoming,
}

/// Exact ordered credit equality, including stable identity, role and sort name.
/// Used only by the incoming-ID-preserving path; name-based identity reuse still
/// requires the normal preparation policy.
pub(super) fn contributors_match(tx: &Transaction, hash: ContentHash, incoming: &[Contributor]) -> rusqlite::Result<bool> {
    let mut statement = tx.prepare_cached(include_str!("sql/authors/contributors_match_select.sql"))?;
    let mut rows = statement.query([hash.as_str()])?;
    for (position, credit) in incoming.iter().enumerate() {
        if parse_author_name(credit.name())?.is_none() {
            return Ok(false);
        }
        let Some(row) = rows.next()? else {
            return Ok(false);
        };
        let stable_id: Option<String> = row.get(1)?;
        let name: String = row.get(2)?;
        let sort_name: Option<String> = row.get(3)?;
        let role: Vec<u8> = row.get(4)?;
        if row.get::<_, i64>(0)? != position as i64 || stable_id.as_deref() != Some(credit.agent_id().to_string().as_str()) || name != credit.name() || sort_name.as_deref() != credit.sort_name() || role.as_slice() != credit.role_code().0 {
            return Ok(false);
        }
    }
    Ok(rows.next()?.is_none())
}

/// Read ordered credits for independent books in one query.
pub(super) fn contributors_match_batch(tx: &Transaction, incoming: &[(ContentHash, &library_replica::SyncBookMetadata)]) -> Result<Vec<bool>, Box<dyn std::error::Error>> {
    let keys = serde_json::to_string(&incoming.iter().map(|(hash, _)| hash.as_str()).collect::<Vec<_>>())?;
    let mut matched = vec![true; incoming.len()];
    let mut counts = vec![0usize; incoming.len()];
    let mut statement = tx.prepare_cached(include_str!("sql/authors/contributors_match_batch.sql"))?;
    let mut rows = statement.query([keys])?;
    while let Some(row) = rows.next()? {
        let index = row.get::<_, i64>(0)? as usize;
        let position = counts[index];
        counts[index] += 1;
        let Some(credit) = incoming[index].1.contributors.get(position) else {
            matched[index] = false;
            continue;
        };
        let stable_id: Option<String> = row.get(2)?;
        let name: String = row.get(3)?;
        let sort_name: Option<String> = row.get(4)?;
        let role: Vec<u8> = row.get(5)?;
        matched[index] &= parse_author_name(credit.name())?.is_some()
            && row.get::<_, i64>(1)? == position as i64
            && stable_id.as_deref() == Some(credit.agent_id().to_string().as_str())
            && name == credit.name()
            && sort_name.as_deref() == credit.sort_name()
            && role.as_slice() == credit.role_code().0;
    }
    for (index, (_, metadata)) in incoming.iter().enumerate() {
        matched[index] &= counts[index] == metadata.contributors.len();
    }
    Ok(matched)
}

pub(super) fn prepare_contributors(transaction: &Transaction, contributors: &[Contributor], policy: AuthorIdPolicy, existing_content_hash: Option<ContentHash>) -> Result<Vec<StoredContributor>, rusqlite::Error> {
    contributors
        .iter()
        .enumerate()
        .map(|(position, contributor)| {
            let Some(name) = parse_author_name(contributor.name())? else { return Ok(None) };
            let existing = if matches!(policy, AuthorIdPolicy::ReuseProvisionalNameCandidate) {
                existing_content_hash
                    .as_ref()
                    .map(|content_hash| {
                        let mut value = None;
                        transaction.existing_book_contributor_credit(content_hash.as_str(), i64::try_from(position).unwrap_or(i64::MAX), |row| {
                            value = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?));
                            Ok(())
                        })?;
                        Ok::<_, rusqlite::Error>(value)
                    })
                    .transpose()?
                    .flatten()
                    .filter(|(_, _, credited_name)| AuthorName::parse(credited_name).is_ok_and(|existing_name| existing_name.match_key() == name.match_key()))
                    .map(|(local_id, stable_id, _)| (local_id, stable_id))
            } else {
                None
            };
            let (identity_local_id, contributor_id) = match existing {
                Some((local_id, stable_id)) => (local_id, AgentId::parse_str(&stable_id).map_err(|error| invalid_text_column(1, error.to_string()))?),
                None => get_or_create_author_identity(transaction, &name, contributor.agent_id(), policy)?,
            };
            let reidentified = Contributor::with_id(contributor_id, contributor.name(), contributor.sort_name().map(ToOwned::to_owned), contributor.role_code()).map_err(|error| invalid_text_column(0, error.to_string()))?;
            Ok(Some(StoredContributor { identity_local_id, credit: reidentified }))
        })
        .filter_map(|contributor| match contributor {
            Ok(Some(contributor)) => Some(Ok(contributor)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

pub(super) fn prepare_book_agents(transaction: &Transaction, metadata: &BookMetadata, policy: AuthorIdPolicy) -> Result<BookMetadata, rusqlite::Error> {
    let mut prepared = metadata.clone();
    prepared.publishers = metadata
        .publishers
        .iter()
        .map(|publisher| {
            let Some(name) = parse_author_name(publisher.name().as_str())? else {
                return Err(invalid_text_column(0, "publisher name did not produce an agent name"));
            };
            let (_, stable_id) = get_or_create_author_identity(transaction, &name, publisher.agent_id(), policy)?;
            PublisherCredit::with_id(stable_id, publisher.name().as_str()).map_err(|error| invalid_text_column(0, error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(prepared)
}

/// Clear+rewrite all `book_contributor` rows for a book in one pass. Contributor
/// identity resolution mirrors the identity policy already used for book
/// agents; the merged list shares one flat `(content_hash, position)` sequence across
/// every role.
pub(super) fn replace_book_contributors(transaction: &Transaction, content_hash: ContentHash, contributors: &[StoredContributor]) -> Result<(), rusqlite::Error> {
    super::metadata_batch::run(transaction, || {
        for (position, contributor) in contributors.iter().enumerate() {
            let role = contributor.credit.role_code().0;
            transaction.prepare_cached(include_str!("sql/authors/upsert_book_contributor.sql"))?.execute(rusqlite::params![
                content_hash.as_str(),
                contributor.identity_local_id,
                i64::try_from(position).unwrap_or(i64::MAX),
                contributor.credit.name(),
                contributor.credit.sort_name(),
                &role
            ])?;
        }
        transaction.prepare_cached(include_str!("sql/authors/trim_book_contributors.sql"))?.execute(rusqlite::params![content_hash.as_str(), i64::try_from(contributors.len()).unwrap_or(i64::MAX)])?;
        Ok(())
    })
}

fn get_or_create_author_identity(transaction: &Transaction, name: &AuthorName, incoming_id: AuthorId, policy: AuthorIdPolicy) -> Result<(i64, AuthorId), rusqlite::Error> {
    let mut incoming = None;
    transaction.author_identity_by_stable_id(&incoming_id.to_string(), |row| {
        incoming = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
        Ok(())
    })?;
    let selected = match incoming {
        Some(value) => Some(value),
        None if matches!(policy, AuthorIdPolicy::ReuseProvisionalNameCandidate) => {
            let mut provisional = None;
            transaction.provisional_author_identity_by_name(name.match_key().as_str(), |row| {
                provisional = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                Ok(())
            })?;
            provisional
        }
        None => None,
    };
    let (local_id, stable_id) = match selected {
        Some((local_id, stable_id)) => {
            let stable_id = AgentId::parse_str(&stable_id).map_err(|error| invalid_text_column(1, error.to_string()))?;
            (local_id, stable_id)
        }
        None => {
            transaction.prepare_cached(include_str!("sql/authors/insert_author_identity.sql"))?.execute(rusqlite::params![incoming_id.to_string(), name.as_str(), name.match_key().as_str(), unix_millis()?])?;
            (transaction.last_insert_rowid(), incoming_id)
        }
    };
    Ok((local_id, stable_id))
}

fn parse_author_name(author_name: &str) -> Result<Option<AuthorName>, rusqlite::Error> {
    if author_name.trim().is_empty() {
        return Ok(None);
    }
    let name = AuthorName::parse(author_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    if name.match_key().as_str().is_empty() {
        return Ok(None);
    }
    Ok(Some(name))
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use rusqlite::Connection;
    #[test]
    fn shared_author_and_publisher_ids_resolve_once_per_batch() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let tx = conn.transaction().unwrap();
        let mut cache = IdentityCache::default();
        let author = Contributor::new("Shared Author", None, book_model::MarcRelatorCode(*b"aut")).unwrap();
        let publisher = PublisherCredit::with_id(author.agent_id(), "Shared Author").unwrap();
        let first = cache.contributors(&tx, &[author.clone()]).unwrap()[0].identity_local_id;
        for _ in 0..32 {
            assert_eq!(cache.contributors(&tx, &[author.clone()]).unwrap()[0].identity_local_id, first);
            assert_eq!(cache.publisher(&tx, &publisher).unwrap().1, first);
        }
        assert_eq!(cache.misses, 2, "one agent lookup and one publisher lookup for the whole batch");
        cache.forget(author.agent_id());
        cache.contributors(&tx, &[author]).unwrap();
        assert_eq!(cache.misses, 3, "invalidated identities must be resolved again");
        tx.commit().unwrap();
    }
}

include_sql!("src/authors/sql/authors.sql");
