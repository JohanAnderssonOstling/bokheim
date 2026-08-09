//! Stable author identities and the ordered, role-tagged contributor list
//! attached to book metadata. `book_contributor` is one flat per-book
//! position sequence shared by every role (author, translator, editor, ...).

use super::*;
use sync_common::Contributor;

pub(super) struct StoredContributor {
    pub(super) identity_local_id: i64,
    pub(super) credit: Contributor,
}

#[derive(Clone, Copy)]
pub(super) enum AuthorIdPolicy {
    ReuseProvisionalNameCandidate,
    PreserveIncoming,
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
                Some((local_id, stable_id)) => (local_id, AuthorId::parse(&stable_id).map_err(|error| invalid_text_column(1, error.to_string()))?),
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

pub(super) fn prepare_publication_agents(transaction: &Transaction, metadata: &PublicationMetadata, policy: AuthorIdPolicy) -> Result<PublicationMetadata, rusqlite::Error> {
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
/// identity resolution mirrors the identity policy already used for publication
/// agents; the merged list shares one flat `(content_hash, position)` sequence across
/// every role.
pub(super) fn replace_book_contributors(transaction: &Transaction, content_hash: ContentHash, contributors: &[StoredContributor]) -> Result<(), rusqlite::Error> {
    for (position, contributor) in contributors.iter().enumerate() {
        let role = contributor.credit.role_code().0;
        transaction.upsert_book_contributor(content_hash.as_str(), contributor.identity_local_id, i64::try_from(position).unwrap_or(i64::MAX), contributor.credit.name(), contributor.credit.sort_name(), &role)?;
    }
    transaction.trim_book_contributors(content_hash.as_str(), i64::try_from(contributors.len()).unwrap_or(i64::MAX))?;
    Ok(())
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
            let stable_id = AuthorId::parse(&stable_id).map_err(|error| invalid_text_column(1, error.to_string()))?;
            (local_id, stable_id)
        }
        None => {
            transaction.insert_author_identity(&incoming_id.to_string(), name.as_str(), name.match_key().as_str(), unix_millis()?)?;
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
