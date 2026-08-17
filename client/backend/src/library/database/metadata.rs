use super::authors::{prepare_contributors, prepare_publication_agents, replace_book_contributors, AuthorIdPolicy};
use super::*;
use library_model::Contributor;
#[cfg(test)]
use library_model::MarcRelatorCode;
use rusqlite::OptionalExtension;
/// Inserts a newly scanned book with its final initial metadata in one write.
/// The book lifecycle trigger can therefore publish title and ordered contributors without a
/// second metadata mutation. A later rescan remains a normal metadata update.
#[cfg(test)]
pub fn insert_scanned_book(transaction: &Transaction, content_hash: ContentHash, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let contributors = if author.trim().is_empty() { Vec::new() } else { vec![Contributor::new(author, None, MarcRelatorCode(*b"aut")).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?] };
    insert_scanned_book_contributors(transaction, content_hash, title, &contributors)
}
#[cfg(test)]
pub fn insert_scanned_book_contributors(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    insert_scanned_book_metadata(transaction, content_hash, &BookMetadataState { title: title.to_owned(), contributors: contributors.to_vec(), description: String::new(), publication: PublicationMetadata::default() }, BookFormat::Epub)
}
pub fn insert_scanned_book_metadata(transaction: &Transaction, content_hash: ContentHash, metadata: &BookMetadataState, format: BookFormat) -> Result<(), rusqlite::Error> {
    let existed: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1)", [content_hash.as_str()], |row| row.get(0))?;
    let title = &metadata.title;
    let stored_contributors = prepare_contributors(transaction, &metadata.contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate, Some(content_hash.clone()))?;
    let publication = prepare_publication_agents(transaction, &metadata.publication, AuthorIdPolicy::ReuseProvisionalNameCandidate)?;
    let encoded_publication = sync_common::wire::encode(&publication).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let description = normalized_description(&metadata.description);
    let added_at = unix_millis()?;
    // The local relationship rows reference SQLite's integer key, so establish
    // the content-hash boundary row before projecting contributors into it.
    transaction.upsert_scanned_book_metadata(content_hash.as_str(), title, &description, &encoded_publication, added_at, format.canonical_extension())?;
    replace_book_contributors(transaction, content_hash.clone(), &stored_contributors)?;
    replace_publication_metadata_projections(transaction, content_hash, &publication)?;
    if !existed {
        // The insert trigger runs before contributor projections exist. Fold
        // those projections into the initial lifecycle body and remove the
        // redundant metadata cell produced while building the new row.
        transaction.execute("DELETE FROM sync_outbox WHERE state_kind='metadata' AND state_key=?1", [content_hash.as_str()])?;
        transaction.execute(
            "UPDATE sync_outbox
             SET body=(SELECT bokheim_mutation('book_lifecycle', book.content_hash, 0,
                         COALESCE(book.title, book.content_hash), contributors_for_book(book.row_id),
                         COALESCE(book.description, ''), book.publication_metadata,
                         COALESCE(book.added_at, 0), book.format, NULL)
                       FROM book WHERE book.content_hash=?1)
             WHERE state_kind='book_lifecycle' AND state_key=?1",
            [content_hash.as_str()],
        )?;
    }
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
fn replace_publication_metadata_projections(transaction: &Transaction, content_hash: ContentHash, metadata: &PublicationMetadata) -> Result<(), rusqlite::Error> {
    let id = content_hash.to_string();
    transaction.clear_book_publishers(&id)?;
    transaction.clear_book_identifiers(&id)?;
    transaction.clear_book_languages(&id)?;
    transaction.clear_book_subject_assignments(&id)?;
    transaction.clear_book_subjects(&id)?;
    transaction.execute("DELETE FROM book_unified_concept WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [&id])?;
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
        transaction.insert_book_subject(&id, position as i64, subject.name(), subject.source(), subject.authority(), subject.code())?;
    }
    let subject_codes = library_model::project_subject_codes(&metadata.subjects);
    for classification in &subject_codes {
        for position in classification.evidence_positions() {
            transaction.execute(
                "INSERT OR IGNORE INTO book_subject_code(book_row_id,subject_position,system_id,code)
                 VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4)",
                rusqlite::params![id, *position as i64, classification.system_id(), classification.code()],
            )?;
        }
    }
    for assignment in library_model::project_subjects(&metadata.subjects) {
        if assignment.system_id() != library_model::BISAC_SYSTEM_ID {
            ensure_external_subject_nodes(transaction, assignment.system_id(), assignment.subject_path(), assignment.matcher_version())?;
        }
        transaction.insert_book_subject_assignment(&id, assignment.system_id(), assignment.subject_path(), assignment.matcher_version())?;
        for position in assignment.evidence_positions() {
            transaction.insert_book_subject_assignment_evidence(&id, assignment.system_id(), assignment.subject_path(), *position as i64)?;
        }
    }
    let unified_assignments = library_model::project_unified_subjects(&metadata.subjects);
    for assignment in &unified_assignments {
        ensure_external_subject_nodes(transaction, assignment.system_id(), assignment.subject_path(), assignment.matcher_version())?;
        transaction.insert_book_subject_assignment(&id, assignment.system_id(), assignment.subject_path(), assignment.matcher_version())?;
        for position in assignment.evidence_positions() {
            transaction.insert_book_subject_assignment_evidence(&id, assignment.system_id(), assignment.subject_path(), *position as i64)?;
        }
    }
    for classification in &subject_codes {
        for unified_path in library_model::unified_subject_paths(classification.system_id(), classification.code()) {
            let source_version = source_subject_version(classification.system_id());
            let concept_id = library_model::unified_concept_id(&unified_path);
            transaction.execute("INSERT OR IGNORE INTO source_subject(system_id,source_version,code,official_label) VALUES(?1,?2,?3,NULL)", rusqlite::params![classification.system_id(), source_version, classification.code()])?;
            transaction.execute(
                "INSERT OR REPLACE INTO source_concept_mapping(source_system_id,source_version,source_code,concept_id,mapping_type,mapper_version) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![classification.system_id(), source_version, classification.code(), concept_id, source_mapping_type(classification.system_id()), library_model::UNIFIED_MATCHER_VERSION],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3)",
                rusqlite::params![id, concept_id, library_model::UNIFIED_MATCHER_VERSION],
            )?;
            for position in classification.evidence_positions() {
                transaction.execute(
                    "INSERT OR IGNORE INTO book_unified_concept_evidence(book_row_id,concept_id,subject_position,source_system_id,source_code) VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5)",
                    rusqlite::params![id, concept_id, *position as i64, classification.system_id(), classification.code()],
                )?;
            }
            transaction.execute(
                "INSERT OR REPLACE INTO subject_code_mapping(source_system_id,source_code,unified_path,mapper_version) VALUES(?1,?2,?3,?4)",
                rusqlite::params![classification.system_id(), classification.code(), unified_path, library_model::UNIFIED_MATCHER_VERSION],
            )?;
        }
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

fn ensure_external_subject_nodes(transaction: &Transaction, system_id: &str, subject_path: &str, matcher_version: i64) -> Result<(), rusqlite::Error> {
    let components = subject_path.split(" / ").map(str::trim).filter(|component| !component.is_empty()).collect::<Vec<_>>();
    transaction.execute("INSERT OR IGNORE INTO subject_system(id,version,matcher_version) VALUES(?1,'external',1)", [system_id])?;
    let mut path = String::new();
    let mut parent_path = String::new();
    for component in components {
        if !path.is_empty() {
            path.push_str(" / ");
        }
        path.push_str(component);
        transaction.execute("INSERT OR IGNORE INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(?1,?2,?3,?4,NULL,1)", rusqlite::params![system_id, path, parent_path, component])?;
        parent_path.clone_from(&path);
    }
    transaction.execute("UPDATE subject_system SET matcher_version=MAX(matcher_version,?2) WHERE id=?1", rusqlite::params![system_id, matcher_version])?;
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
fn reidentify_book_credit(transaction: &Transaction, content_hash: ContentHash, position: usize, identity_local_id: i64, _author_id: AuthorId) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "UPDATE book_contributor SET author_identity_id=?1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND position=?3 AND role=X'617574'",
        rusqlite::params![identity_local_id, content_hash.as_str(), i64::try_from(position).unwrap_or(i64::MAX)],
    )?;
    Ok(())
}
/// Splits one credited occurrence away from an author that turned out to be a
/// namesake. The credited spelling and normalized grouping key remain intact;
/// only the stable identity referenced by this book position changes. Scoped
/// to author-role (`"aut"`) rows.
#[cfg(test)]
pub fn split_book_author(transaction: &Transaction, content_hash: ContentHash, position: usize) -> Result<AuthorId, rusqlite::Error> {
    let credited_name: String = transaction.query_row(
        "SELECT name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND position=?2 AND role=X'617574'",
        rusqlite::params![content_hash.as_str(), i64::try_from(position).unwrap_or(i64::MAX)],
        |row| row.get(0),
    )?;
    let preferred_name = AuthorName::parse(&credited_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let author_id = AgentId::new_v4();
    transaction.execute(
        "INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at) VALUES(?1, ?2, ?3, 1, ?4)",
        rusqlite::params![author_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?],
    )?;
    let identity_local_id = transaction.last_insert_rowid();
    reidentify_book_credit(transaction, content_hash, position, identity_local_id, author_id)?;
    Ok(author_id)
}
/// Resolves one exact book credit using an authority identifier. A normalized
/// name remains only an alias candidate: two credits with the same normalized
/// name can resolve to different identities, while spelling variants carrying
/// the same authority id converge on one identity. Scoped to author-role
/// (`"aut"`) rows.
pub fn resolve_book_author_identity(transaction: &Transaction, content_hash: ContentHash, position: usize, preferred_name: &str, external_id: &ExternalAuthorId) -> Result<AuthorId, rusqlite::Error> {
    let preferred_name = AuthorName::parse(preferred_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let position = i64::try_from(position).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let (current_local_id, _current_stable_id, current_is_provisional): (i64, String, bool) = transaction.query_row(
        "SELECT author_identity.id, author_identity.stable_id, author_identity.is_provisional
         FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND book_contributor.position=?2 AND book_contributor.role=X'617574'",
        rusqlite::params![content_hash.as_str(), position],
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
    let authority_stable_id = library_model::agent_id_from_migration_seed(format!("external-author:{}:{}", external_id.authority().as_str(), external_id.value()).as_bytes());
    let stable_identity = transaction.query_row("SELECT id,stable_id FROM author_identity WHERE stable_id=?1", [authority_stable_id.to_string()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).optional()?;
    let current_credit_count: i64 = transaction.query_row("SELECT COUNT(*) FROM book_contributor WHERE author_identity_id=?1", [current_local_id], |row| row.get(0))?;
    let (target_local_id, target_stable_id) = match existing {
        Some(identity) => identity,
        None if stable_identity.is_some() => stable_identity.expect("checked above"),
        None if current_is_provisional && current_credit_count == 1 => {
            transaction.execute("UPDATE author_identity SET stable_id=?1 WHERE id=?2", rusqlite::params![authority_stable_id.to_string(), current_local_id])?;
            (current_local_id, authority_stable_id.to_string())
        }
        None => {
            transaction.execute(
                "INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at) VALUES(?1, ?2, ?3, 0, ?4)",
                rusqlite::params![authority_stable_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?],
            )?;
            (transaction.last_insert_rowid(), authority_stable_id.to_string())
        }
    };
    let target_author_id = AgentId::parse_str(&target_stable_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
    transaction.execute("UPDATE author_identity SET preferred_name=?1, normalized_name=?2, is_provisional=0 WHERE id=?3", rusqlite::params![preferred_name.as_str(), preferred_name.match_key().as_str(), target_local_id])?;
    transaction.execute(
        "INSERT OR IGNORE INTO author_external_identifier(author_identity_id, authority, external_id, verified_at) VALUES(?1, ?2, ?3, ?4)",
        rusqlite::params![target_local_id, external_id.authority().as_str(), external_id.value(), unix_millis()?],
    )?;
    // Update even when the local row is unchanged. The contributor trigger
    // refreshes the already-queued metadata mutation with the authority-derived
    // stable id before the first upload.
    reidentify_book_credit(transaction, content_hash, usize::try_from(position).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?, target_local_id, target_author_id)?;
    if target_local_id != current_local_id {
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
pub fn get_book_author_identities(conn: &Connection, content_hash: ContentHash) -> Result<Vec<AuthorCreditIdentity>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT book_contributor.position, book_contributor.name, author_identity.stable_id, author_identity.preferred_name,
                author_external_identifier.authority, author_external_identifier.external_id
         FROM book_contributor
         LEFT JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         LEFT JOIN author_external_identifier ON author_external_identifier.author_identity_id=author_identity.id
         WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND book_contributor.role=X'617574'
         ORDER BY book_contributor.position, author_external_identifier.authority, author_external_identifier.external_id",
    )?;
    let mut rows = statement.query([content_hash.as_str()])?;
    let mut credits = Vec::<AuthorCreditIdentity>::new();
    while let Some(row) = rows.next()? {
        let raw_position: i64 = row.get(0)?;
        let position = usize::try_from(raw_position).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?;
        if credits.last().is_none_or(|credit| credit.position != position) {
            let stable_id: String = row.get(2)?;
            let author_id = AgentId::parse_str(&stable_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
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
pub fn update_book_metadata(transaction: &Transaction, content_hash: ContentHash, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let contributors = if author.trim().is_empty() { Vec::new() } else { vec![Contributor::new(author, None, MarcRelatorCode(*b"aut")).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?] };
    update_book_metadata_contributors(transaction, content_hash, title, &contributors)
}
#[cfg(test)]
pub fn update_book_metadata_contributors(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, content_hash, title, contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate)
}
pub(crate) fn update_book_metadata_contributors_preserving_ids(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, content_hash, title, contributors, AuthorIdPolicy::PreserveIncoming)
}
fn update_book_metadata_contributors_with_policy(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor], policy: AuthorIdPolicy) -> Result<(), rusqlite::Error> {
    let stored_contributors = prepare_contributors(transaction, contributors, policy, Some(content_hash.clone()))?;
    transaction.update_book_title(title, content_hash.as_str())?;
    replace_book_contributors(transaction, content_hash, &stored_contributors)?;
    Ok(())
}
pub fn update_book_description(transaction: &Transaction, content_hash: ContentHash, description: &str) -> Result<(), rusqlite::Error> {
    let normalized = normalized_description(description);
    transaction.update_book_description(&normalized, content_hash.as_str())?;
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
fn update_book_publication_metadata(transaction: &Transaction, content_hash: ContentHash, metadata: &PublicationMetadata, policy: AuthorIdPolicy) -> Result<(), rusqlite::Error> {
    let metadata = prepare_publication_agents(transaction, metadata, policy)?;
    let encoded = sync_common::wire::encode(&metadata).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.update_book_publication_metadata(&encoded, content_hash.as_str())?;
    replace_publication_metadata_projections(transaction, content_hash, &metadata)?;
    Ok(())
}
pub(super) fn update_book_publication_metadata_preserving_ids(transaction: &Transaction, content_hash: ContentHash, metadata: &PublicationMetadata) -> Result<(), rusqlite::Error> {
    update_book_publication_metadata(transaction, content_hash, metadata, AuthorIdPolicy::PreserveIncoming)
}
pub(super) fn update_book_metadata_state_preserving_ids(transaction: &Transaction, content_hash: ContentHash, metadata: &BookMetadataState) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_preserving_ids(transaction, content_hash.clone(), &metadata.title, &metadata.contributors)?;
    update_book_description(transaction, content_hash.clone(), &metadata.description)?;
    update_book_publication_metadata(transaction, content_hash, &metadata.publication, AuthorIdPolicy::PreserveIncoming)
}
