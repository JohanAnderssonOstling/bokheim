use super::authors::{prepare_contributors, prepare_publication_agents, replace_book_contributors, AuthorIdPolicy};
use super::*;
use sync_common::Contributor;
#[cfg(test)]
use sync_common::MarcRelatorCode;
/// Inserts a newly scanned book with its final initial metadata in one write.
/// The book lifecycle trigger can therefore publish title and ordered contributors without a
/// second metadata mutation. A later rescan remains a normal metadata update.
#[cfg(test)]
pub fn insert_scanned_book(transaction: &Transaction, book_id: BookId, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let contributors = if author.trim().is_empty() { Vec::new() } else { vec![Contributor::new(author, None, MarcRelatorCode(*b"aut")).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?] };
    insert_scanned_book_contributors(transaction, book_id, title, &contributors)
}
#[cfg(test)]
pub fn insert_scanned_book_contributors(transaction: &Transaction, book_id: BookId, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    insert_scanned_book_metadata(transaction, book_id, &BookMetadataState { title: title.to_owned(), contributors: contributors.to_vec(), description: String::new(), publication: PublicationMetadata::default() }, BookFormat::Epub)
}
pub fn insert_scanned_book_metadata(transaction: &Transaction, book_id: BookId, metadata: &BookMetadataState, format: BookFormat) -> Result<(), rusqlite::Error> {
    let title = &metadata.title;
    let stored_contributors = prepare_contributors(transaction, &metadata.contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate, Some(book_id))?;
    let publication = prepare_publication_agents(transaction, &metadata.publication, AuthorIdPolicy::ReuseProvisionalNameCandidate)?;
    let encoded_publication = sync_common::wire::encode(&publication).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let description = normalized_description(&metadata.description);
    let added_at = unix_millis()?;
    // Write book_contributor rows before the book row itself (deferring FK
    // enforcement to commit) so that trig_book_added's contributors_for_book(..)
    // call already sees the full contributor list on a brand-new book. Doing
    // it in the other order would make the initial book_lifecycle mutation
    // carry an empty contributor list, and the book_contributor insert
    // triggers firing afterward would then enqueue a spurious extra
    // `metadata` mutation alongside `book_lifecycle` for a fresh scan.
    transaction.pragma_update(None, "defer_foreign_keys", "1")?;
    replace_book_contributors(transaction, book_id, &stored_contributors)?;
    transaction.upsert_scanned_book_metadata(&book_id.canonical(), title, &description, &encoded_publication, added_at, format.canonical_extension())?;
    replace_publication_metadata_projections(transaction, book_id, &publication)?;
    Ok(())
}
fn identifier_scheme_label(scheme: &PublicationIdentifierScheme) -> String {
    match scheme {
        PublicationIdentifierScheme::Unspecified => "unspecified".to_owned(),
        PublicationIdentifierScheme::Isbn => "isbn".to_owned(),
        PublicationIdentifierScheme::Asin => "asin".to_owned(),
        PublicationIdentifierScheme::Doi => "doi".to_owned(),
        PublicationIdentifierScheme::Issn => "issn".to_owned(),
        PublicationIdentifierScheme::Oclc => "oclc".to_owned(),
        PublicationIdentifierScheme::Lccn => "lccn".to_owned(),
        PublicationIdentifierScheme::Uuid => "uuid".to_owned(),
        PublicationIdentifierScheme::Calibre => "calibre".to_owned(),
        PublicationIdentifierScheme::Google => "google".to_owned(),
        PublicationIdentifierScheme::Goodreads => "goodreads".to_owned(),
        PublicationIdentifierScheme::Kobo => "kobo".to_owned(),
        PublicationIdentifierScheme::Other(value) => format!("other:{value}"),
    }
}
fn replace_publication_metadata_projections(transaction: &Transaction, book_id: BookId, metadata: &PublicationMetadata) -> Result<(), rusqlite::Error> {
    let id = book_id.canonical();
    transaction.clear_book_publishers(&id)?;
    transaction.clear_book_identifiers(&id)?;
    transaction.clear_book_languages(&id)?;
    transaction.clear_book_subjects(&id)?;
    transaction.clear_book_collections(&id)?;
    transaction.clear_book_publication_dates(&id)?;
    transaction.clear_book_publication_values(&id)?;
    for (position, publisher) in metadata.publishers.iter().enumerate() {
        let key = publisher.name().match_key();
        transaction.insert_publisher(publisher.name().as_str(), key.as_str())?;
        let mut publisher_id = 0;
        transaction.publisher_id_by_name(key.as_str(), |row| {
            publisher_id = row.get(0)?;
            Ok(())
        })?;
        let mut agent_identity_id = 0;
        transaction.author_identity_local_id(&publisher.agent_id().to_string(), |row| {
            agent_identity_id = row.get(0)?;
            Ok(())
        })?;
        transaction.insert_book_publisher(&id, publisher_id, agent_identity_id, position as i64, publisher.name().as_str())?;
    }
    for (position, identifier) in metadata.identifiers.iter().enumerate() {
        let evidence = sync_common::wire::encode(identifier.identifier_types()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.insert_book_identifier(&id, position as i64, &identifier_scheme_label(identifier.scheme()), identifier.value(), identifier.canonical_value().as_deref(), &evidence)?;
    }
    for (position, language) in metadata.languages.iter().enumerate() {
        transaction.insert_book_language(&id, position as i64, language.as_str())?;
    }
    for (position, subject) in metadata.subjects.iter().enumerate() {
        transaction.insert_book_subject(&id, position as i64, subject.name(), subject.authority(), subject.code())?;
    }
    for (position, collection) in metadata.collections.iter().enumerate() {
        transaction.insert_book_collection(&id, position as i64, collection.name(), collection.collection_type(), collection.position())?;
    }
    for (position, date) in metadata.dates.iter().enumerate() {
        transaction.insert_book_publication_date(&id, position as i64, date.value(), None, None, None, date.event())?;
    }
    let insert_values = |kind: &str, values: &[(String, Option<String>, Option<String>)]| -> Result<(), rusqlite::Error> {
        for (position, (value, qualifier, secondary)) in values.iter().enumerate() {
            transaction.insert_book_publication_value(&id, kind, position as i64, value, qualifier.as_deref(), secondary.as_deref())?;
        }
        Ok(())
    };
    let alternate_titles = metadata.alternate_titles.iter().map(|value| (value.value().to_owned(), value.title_type().map(ToOwned::to_owned), value.sort_as().map(ToOwned::to_owned))).collect::<Vec<_>>();
    insert_values("alternate_title", &alternate_titles)?;
    for (kind, values) in [("rights", &metadata.rights), ("source", &metadata.sources), ("type", &metadata.types), ("format", &metadata.formats), ("relation", &metadata.relations), ("coverage", &metadata.coverage)] {
        let values = values.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>();
        insert_values(kind, &values)?;
    }
    let properties = metadata.properties.iter().map(|property| (property.value().to_owned(), property.refines().map(ToOwned::to_owned), Some(property.property().to_owned()))).collect::<Vec<_>>();
    insert_values("property", &properties)?;
    Ok(())
}
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorCreditIdentity {
    pub position: usize,
    pub credited_name: String,
    pub author_id: AuthorId,
    pub preferred_name: Option<String>,
    pub external_ids: Vec<ExternalAuthorId>,
}
/// Reidentifies one author-role (`role = "aut"`) book_contributor row. Kept
/// scoped to authors, matching the existing product behavior: the reidentify
/// UI does not yet operate on translators/editors/etc.
#[cfg(test)]
fn reidentify_book_credit(transaction: &Transaction, book_id: BookId, position: usize, identity_local_id: i64, _author_id: AuthorId) -> Result<(), rusqlite::Error> {
    transaction.execute("UPDATE book_contributor SET author_identity_id=?1 WHERE book_id=?2 AND position=?3 AND role=X'617574'", rusqlite::params![identity_local_id, book_id.canonical(), i64::try_from(position).unwrap_or(i64::MAX)])?;
    Ok(())
}
/// Splits one credited occurrence away from an author that turned out to be a
/// namesake. The credited spelling and normalized grouping key remain intact;
/// only the stable identity referenced by this book position changes. Scoped
/// to author-role (`"aut"`) rows.
#[cfg(test)]
pub fn split_book_author(transaction: &Transaction, book_id: BookId, position: usize) -> Result<AuthorId, rusqlite::Error> {
    let credited_name: String =
        transaction.query_row("SELECT name FROM book_contributor WHERE book_id=?1 AND position=?2 AND role=X'617574'", rusqlite::params![book_id.canonical(), i64::try_from(position).unwrap_or(i64::MAX)], |row| row.get(0))?;
    let preferred_name = AuthorName::parse(&credited_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let author_id = AuthorId::new();
    transaction.execute(
        "INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at) VALUES(?1, ?2, ?3, 1, ?4)",
        rusqlite::params![author_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?],
    )?;
    let identity_local_id = transaction.last_insert_rowid();
    reidentify_book_credit(transaction, book_id, position, identity_local_id, author_id)?;
    Ok(author_id)
}
/// Resolves one exact book credit using an authority identifier. A normalized
/// name remains only an alias candidate: two credits with the same normalized
/// name can resolve to different identities, while spelling variants carrying
/// the same authority id converge on one identity. Scoped to author-role
/// (`"aut"`) rows.
#[cfg(test)]
pub fn resolve_book_author_identity(transaction: &Transaction, book_id: BookId, position: usize, preferred_name: &str, external_id: &ExternalAuthorId) -> Result<AuthorId, rusqlite::Error> {
    let preferred_name = AuthorName::parse(preferred_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let position = i64::try_from(position).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let (current_local_id, current_stable_id, current_is_provisional): (i64, String, bool) = transaction.query_row(
        "SELECT author_identity.id, author_identity.stable_id, author_identity.is_provisional
         FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         WHERE book_contributor.book_id=?1 AND book_contributor.position=?2 AND book_contributor.role=X'617574'",
        rusqlite::params![book_id.canonical(), position],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let existing: Option<(i64, String)> = transaction
        .query_row(
            "SELECT author_identity.id, author_identity.stable_id
             FROM author_external_identifier JOIN author_identity ON author_identity.id=author_external_identifier.author_identity_id
             WHERE author_external_identifier.authority=?1 AND author_external_identifier.external_id=?2",
            rusqlite::params![external_id.authority().as_str(), external_id.value()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (target_local_id, target_stable_id) = match existing {
        Some(identity) => identity,
        None if current_is_provisional => (current_local_id, current_stable_id),
        None => {
            let split_id = AuthorId::new();
            transaction.execute(
                "INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at) VALUES(?1, ?2, ?3, 0, ?4)",
                rusqlite::params![split_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?],
            )?;
            (transaction.last_insert_rowid(), split_id.to_string())
        }
    };
    let target_author_id = AuthorId::parse(&target_stable_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
    transaction.execute("UPDATE author_identity SET preferred_name=?1, normalized_name=?2, is_provisional=0 WHERE id=?3", rusqlite::params![preferred_name.as_str(), preferred_name.match_key().as_str(), target_local_id])?;
    transaction.execute(
        "INSERT OR IGNORE INTO author_external_identifier(author_identity_id, authority, external_id, verified_at) VALUES(?1, ?2, ?3, ?4)",
        rusqlite::params![target_local_id, external_id.authority().as_str(), external_id.value(), unix_millis()?],
    )?;
    if target_local_id != current_local_id {
        reidentify_book_credit(transaction, book_id, usize::try_from(position).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?, target_local_id, target_author_id)?;
        transaction.execute(
            "DELETE FROM author_identity
             WHERE id=?1 AND is_provisional=1
               AND NOT EXISTS(SELECT 1 FROM book_contributor WHERE author_identity_id=?1)
               AND NOT EXISTS(SELECT 1 FROM author_external_identifier WHERE author_identity_id=?1)",
            [current_local_id],
        )?;
    }
    Ok(target_author_id)
}
#[cfg(test)]
pub fn attach_external_author_id(transaction: &Transaction, author_id: AuthorId, external_id: &ExternalAuthorId) -> Result<(), rusqlite::Error> {
    let identity_local_id: i64 = transaction.query_row("SELECT id FROM author_identity WHERE stable_id=?1", [author_id.to_string()], |row| row.get(0))?;
    transaction.execute(
        "INSERT OR IGNORE INTO author_external_identifier(author_identity_id, authority, external_id, verified_at) VALUES(?1, ?2, ?3, ?4)",
        rusqlite::params![identity_local_id, external_id.authority().as_str(), external_id.value(), unix_millis()?],
    )?;
    let owner: i64 = transaction.query_row("SELECT author_identity_id FROM author_external_identifier WHERE authority=?1 AND external_id=?2", rusqlite::params![external_id.authority().as_str(), external_id.value()], |row| row.get(0))?;
    if owner != identity_local_id {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "external author identifier is already attached to another identity"))));
    }
    transaction.execute("UPDATE author_identity SET is_provisional=0 WHERE id=?1", [identity_local_id])?;
    Ok(())
}
#[cfg(test)]
pub fn get_book_author_identities(conn: &Connection, book_id: BookId) -> Result<Vec<AuthorCreditIdentity>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT book_contributor.position, book_contributor.name, author_identity.stable_id, author_identity.preferred_name,
                author_external_identifier.authority, author_external_identifier.external_id
         FROM book_contributor
         LEFT JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         LEFT JOIN author_external_identifier ON author_external_identifier.author_identity_id=author_identity.id
         WHERE book_contributor.book_id=?1 AND book_contributor.role=X'617574'
         ORDER BY book_contributor.position, author_external_identifier.authority, author_external_identifier.external_id",
    )?;
    let mut rows = statement.query([book_id.canonical()])?;
    let mut credits = Vec::<AuthorCreditIdentity>::new();
    while let Some(row) = rows.next()? {
        let raw_position: i64 = row.get(0)?;
        let position = usize::try_from(raw_position).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?;
        if credits.last().is_none_or(|credit| credit.position != position) {
            let stable_id: String = row.get(2)?;
            let author_id = AuthorId::parse(&stable_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
            credits.push(AuthorCreditIdentity { position, credited_name: row.get(1)?, author_id, preferred_name: row.get(3)?, external_ids: Vec::new() });
        }
        let authority: Option<String> = row.get(4)?;
        let external_value: Option<String> = row.get(5)?;
        if let (Some(authority), Some(external_value)) = (authority, external_value) {
            let authority = AuthorAuthority::parse(&authority).map_err(|error| invalid_text_column(4, error.to_string()))?;
            let external_id = ExternalAuthorId::parse(authority, &external_value).map_err(|error| invalid_text_column(5, error.to_string()))?;
            credits.last_mut().expect("the current credit was inserted above").external_ids.push(external_id);
        }
    }
    Ok(credits)
}
#[cfg(test)]
pub fn update_book_metadata(transaction: &Transaction, book_id: BookId, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let contributors = if author.trim().is_empty() { Vec::new() } else { vec![Contributor::new(author, None, MarcRelatorCode(*b"aut")).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?] };
    update_book_metadata_contributors(transaction, book_id, title, &contributors)
}
#[cfg(test)]
pub fn update_book_metadata_contributors(transaction: &Transaction, book_id: BookId, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, book_id, title, contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate)
}
pub(crate) fn update_book_metadata_contributors_preserving_ids(transaction: &Transaction, book_id: BookId, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, book_id, title, contributors, AuthorIdPolicy::PreserveIncoming)
}
fn update_book_metadata_contributors_with_policy(transaction: &Transaction, book_id: BookId, title: &str, contributors: &[Contributor], policy: AuthorIdPolicy) -> Result<(), rusqlite::Error> {
    let stored_contributors = prepare_contributors(transaction, contributors, policy, Some(book_id))?;
    transaction.update_book_title(title, &book_id.canonical())?;
    replace_book_contributors(transaction, book_id, &stored_contributors)?;
    Ok(())
}
pub fn update_book_description(transaction: &Transaction, book_id: BookId, description: &str) -> Result<(), rusqlite::Error> {
    let normalized = normalized_description(description);
    transaction.update_book_description(&normalized, &book_id.canonical())?;
    Ok(())
}
fn normalized_description(description: &str) -> String {
    let mut normalized = String::with_capacity(description.len().min(65_536));
    for character in description.chars().filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t')) {
        if normalized.len() + character.len_utf8() > 65_536 {
            break;
        }
        normalized.push(character);
    }
    normalized
}
fn update_book_publication_metadata(transaction: &Transaction, book_id: BookId, metadata: &PublicationMetadata, policy: AuthorIdPolicy) -> Result<(), rusqlite::Error> {
    let metadata = prepare_publication_agents(transaction, metadata, policy)?;
    let encoded = sync_common::wire::encode(&metadata).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.update_book_publication_metadata(&encoded, &book_id.canonical())?;
    replace_publication_metadata_projections(transaction, book_id, &metadata)?;
    Ok(())
}
pub(super) fn update_book_metadata_state_preserving_ids(transaction: &Transaction, book_id: BookId, metadata: &BookMetadataState) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_preserving_ids(transaction, book_id, &metadata.title, &metadata.contributors)?;
    update_book_description(transaction, book_id, &metadata.description)?;
    update_book_publication_metadata(transaction, book_id, &metadata.publication, AuthorIdPolicy::PreserveIncoming)
}
