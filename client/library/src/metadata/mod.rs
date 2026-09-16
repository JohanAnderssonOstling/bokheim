use super::authors::{AuthorIdPolicy, prepare_contributors, prepare_publication_agents, replace_book_contributors};
use crate::metadata::queries::MetadataSql;
#[cfg(test)]
use crate::schema::init_db;
#[cfg(test)]
use crate::schema::library_write_transaction;
use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use crate::storage_values::unix_millis;
#[cfg(test)]
use crate::sync_state::queries::SyncSql;
use library_model::AgentId;
#[cfg(test)]
use library_model::AuthorAuthority;
use library_model::AuthorId;
use library_model::AuthorName;
use library_model::BookFormat;
use library_model::BookMetadataState;
use library_model::Contributor;
use library_model::ExternalAuthorId;
#[cfg(test)]
#[cfg(test)]
use library_model::MarcRelatorCode;
use library_model::PublicationMetadata;
#[cfg(test)]
use library_model::PublicationSubject;
#[cfg(test)]
use library_model::PublisherCredit;
use library_model::Scheme;
use library_model::Scope;
use library_model::SyncBookMetadata;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use sync_common::ContentHash;
#[path = "classification_delta.rs"]
mod classification_delta;
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
    let mut metadata = metadata.clone();
    publication_enrichment::title_identity::normalize(&mut metadata).map_err(|e| invalid_text_column(0, e))?;
    let previous = stored_publication(transaction, content_hash)?;
    let existed: bool = transaction.query_row(include_str!("sql/metadata/insert_scanned_book_metadata_select.sql"), [content_hash.as_str()], |row| row.get(0))?;
    let title = &metadata.title;
    let stored_contributors = prepare_contributors(transaction, &metadata.contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate, Some(content_hash.clone()))?;
    let publication = prepare_publication_agents(transaction, &metadata.publication, AuthorIdPolicy::ReuseProvisionalNameCandidate)?;
    let encoded_publication = sync_common::wire::encode(&publication).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let description = normalized_description(&metadata.description);
    let added_at = unix_millis()?;
    // The local relationship rows reference SQLite's integer key, so establish
    // the content-hash boundary row before projecting contributors into it.
    transaction.upsert_scanned_book_metadata(content_hash.as_str(), title, &encoded_publication, added_at, format.canonical_extension())?;
    super::descriptions::set(transaction, content_hash, &description)?;
    replace_book_contributors(transaction, content_hash.clone(), &stored_contributors)?;
    replace_publication_metadata_projections(transaction, content_hash, &publication, previous.as_ref())?;
    if !existed {
        // The insert trigger runs before contributor projections exist. Fold
        // those projections into the initial lifecycle body and remove the
        // redundant metadata cell produced while building the new row.
        transaction.execute(include_str!("sql/metadata/insert_scanned_book_metadata_delete.sql"), [content_hash.as_str()])?;
        transaction.execute(include_str!("sql/metadata/insert_scanned_book_metadata_update.sql"), [content_hash.as_str()])?;
    }
    Ok(())
}
fn identifier_scheme_label(scheme: &Scheme) -> String {
    match scheme {
        Scheme::Unspecified => "unspecified".to_owned(),
        Scheme::Isbn => "isbn".to_owned(),
        Scheme::Asin => "asin".to_owned(),
        Scheme::Doi => "doi".to_owned(),
        Scheme::Issn => "issn".to_owned(),
        Scheme::Oclc => "oclc".to_owned(),
        Scheme::Lccn => "lccn".to_owned(),
        Scheme::Uuid => "uuid".to_owned(),
        Scheme::Calibre => "calibre".to_owned(),
        Scheme::Google => "google".to_owned(),
        Scheme::Goodreads => "goodreads".to_owned(),
        Scheme::Kobo => "kobo".to_owned(),
        Scheme::Other(value) => format!("other:{value}"),
    }
}
fn identifier_scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::Publication => "publication",
        Scope::Edition => "edition",
        Scope::Work => "work",
    }
}
fn stored_publication(conn: &Connection, hash: ContentHash) -> rusqlite::Result<Option<PublicationMetadata>> {
    let raw = conn.query_row(include_str!("sql/metadata/stored_publication_select.sql"), [hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    // Missing/legacy-invalid values require a full projection, not a no-op.
    Ok(raw.and_then(|raw| super::external_metadata::decode_publication_metadata(&raw).ok()))
}

fn replace_publication_metadata_projections(transaction: &Transaction, content_hash: ContentHash, metadata: &PublicationMetadata, previous: Option<&PublicationMetadata>) -> Result<(), rusqlite::Error> {
    if previous == Some(metadata) {
        return Ok(());
    }
    // Existing write paths already encoded the canonical value.
    let prepared = PreparedPublication::project(metadata, Vec::new(), previous)?;
    replace_prepared_projections(transaction, content_hash, &prepared, &mut super::authors::IdentityCache::default())
}

/// CPU-only projection work; construct before taking the library writer.
pub(super) struct PreparedPublication {
    metadata: PublicationMetadata,
    encoded: Vec<u8>,
    subject_codes: Vec<subject_projection::ProjectedSubjectCode>,
    assignments: Vec<subject_projection::ProjectedSubjectAssignment>,
    concepts: Vec<(usize, i64, String)>,
    references: Vec<(String, Option<String>, Option<String>)>,
    previous: Option<PublicationMetadata>,
    classification_rows: Vec<classification_delta::PreparedRows>,
}

impl PreparedPublication {
    pub(super) fn new(metadata: &PublicationMetadata, previous: &PublicationMetadata) -> rusqlite::Result<Self> {
        let encoded = sync_common::wire::encode(metadata).map_err(|e| invalid_text_column(0, e.to_string()))?;
        Self::project(metadata, encoded, Some(previous))
    }

    fn project(metadata: &PublicationMetadata, encoded: Vec<u8>, previous: Option<&PublicationMetadata>) -> rusqlite::Result<Self> {
        let subjects_changed = previous.is_none_or(|old| old.subjects != metadata.subjects);
        let properties_changed = previous.is_none_or(|old| old.properties != metadata.properties);
        let mut metadata = metadata.clone();
        metadata.infer_embedded_subject_isbns().map_err(|e| invalid_text_column(0, e.to_string()))?;
        let subject_codes = if subjects_changed { subject_projection::project_subject_codes(&metadata.subjects) } else { Vec::new() };
        let assignments = if subjects_changed { subject_projection::project_subjects(&metadata.subjects) } else { Vec::new() };
        let mut concepts = Vec::new();
        for (index, code) in subject_codes.iter().enumerate() {
            for path in subject_projection::unified_subject_paths(code.system_id(), code.code()) {
                concepts.push((index, subject_projection::unified_concept_id(&path).expect("projected subject exists in the taxonomy"), path));
            }
        }
        let references = if properties_changed { publication_enrichment::related_isbn::lookup_candidates(&metadata.properties).into_iter().map(|isbn| (isbn, None, None)).collect() } else { Vec::new() };
        let previous = previous
            .cloned()
            .map(|mut old| {
                old.infer_embedded_subject_isbns().map_err(|e| invalid_text_column(0, e.to_string()))?;
                Ok::<_, rusqlite::Error>(old)
            })
            .transpose()?;
        let mut prepared = Self { metadata, encoded, subject_codes, assignments, concepts, references, previous, classification_rows: Vec::new() };
        if subjects_changed {
            prepared.classification_rows = classification_delta::prepare(&prepared);
        }
        Ok(prepared)
    }

    pub(super) fn apply(&self, transaction: &Transaction, hash: ContentHash, description: Option<&str>, identities: &mut super::authors::IdentityCache) -> rusqlite::Result<()> {
        // Resolve local publisher identity rows at write time; no classification,
        // metadata merging or serialization is repeated here.
        transaction.execute(include_str!("sql/metadata/apply_update.sql"), rusqlite::params![hash.as_str(), self.encoded, description])?;
        if let Some(description) = description {
            super::descriptions::set(transaction, hash, description)?;
        }
        replace_prepared_projections(transaction, hash, self, identities)
    }
}

fn replace_prepared_projections(transaction: &Transaction, content_hash: ContentHash, prepared: &PreparedPublication, identities: &mut super::authors::IdentityCache) -> Result<(), rusqlite::Error> {
    let metadata = &prepared.metadata;
    let previous = prepared.previous.as_ref();
    let id: i64 = transaction.query_row(include_str!("sql/metadata/replace_prepared_projections_select.sql"), [content_hash.as_str()], |r| r.get(0))?;
    if previous.as_ref().is_none_or(|old| old.publishers != metadata.publishers) {
        transaction.clear_book_publishers(id)?;
        for (position, publisher) in metadata.publishers.iter().enumerate() {
            let (publisher_id, agent_identity_id) = identities.publisher(transaction, publisher)?;
            transaction.insert_book_publisher(id, publisher_id, agent_identity_id, position as i64, publisher.name().as_str())?;
        }
    }
    if previous.as_ref().is_none_or(|old| old.identifiers != metadata.identifiers) {
        transaction.clear_book_identifiers(id)?;
        for (position, identifier) in metadata.identifiers.iter().enumerate() {
            transaction.insert_book_identifier(
                id,
                position as i64,
                &identifier_scheme_label(identifier.scheme()),
                identifier.value(),
                identifier.canonical_value().as_deref(),
                identifier.source(),
                identifier_scope_label(identifier.scope()),
            )?;
        }
    }
    if previous.as_ref().is_none_or(|old| old.languages != metadata.languages) {
        transaction.clear_book_languages(id)?;
        for (position, language) in metadata.languages.iter().enumerate() {
            transaction.insert_book_language(id, position as i64, language.as_str())?;
        }
    }
    if previous.as_ref().is_none_or(|old| old.subjects != metadata.subjects) {
        classification_delta::apply(transaction, id, prepared)?;
    }
    if previous.as_ref().is_none_or(|old| old.collections != metadata.collections) {
        transaction.clear_book_collections(id)?;
        for (position, collection) in metadata.collections.iter().enumerate() {
            transaction.insert_book_collection(id, position as i64, collection.name(), collection.collection_type(), collection.position())?;
        }
    }
    if previous.as_ref().is_none_or(|old| old.dates != metadata.dates) {
        transaction.clear_book_publication_dates(id)?;
        for (position, date) in metadata.dates.iter().enumerate() {
            transaction.insert_book_publication_date(id, position as i64, date.value(), None, None, None, date.event())?;
        }
    }
    let insert_values = |kind: &str, values: &[(String, Option<String>, Option<String>)]| -> Result<(), rusqlite::Error> {
        let changed = previous.as_ref().is_none_or(|old| match kind {
            "alternate_title" => old.alternate_titles != metadata.alternate_titles,
            "rights" => old.rights != metadata.rights,
            "source" => old.sources != metadata.sources,
            "type" => old.types != metadata.types,
            "format" => old.formats != metadata.formats,
            "relation" => old.relations != metadata.relations,
            "coverage" => old.coverage != metadata.coverage,
            "property" | "enrichment_isbn" => old.properties != metadata.properties,
            _ => true,
        });
        if !changed {
            return Ok(());
        }
        transaction.execute(include_str!("sql/metadata/replace_prepared_projections_delete.sql"), rusqlite::params![id, kind])?;
        for (position, (value, qualifier, secondary)) in values.iter().enumerate() {
            transaction.insert_book_publication_value(id, kind, position as i64, value, qualifier.as_deref(), secondary.as_deref())?;
        }
        Ok(())
    };
    let mut ordered_titles = metadata.alternate_titles.iter().collect::<Vec<_>>();
    ordered_titles.sort_by_key(|title| title.source() == "title:colon-fallback" && title.title_type() == Some("subtitle"));
    let alternate_titles = ordered_titles.into_iter().map(|value| (value.value().to_owned(), value.title_type().map(ToOwned::to_owned), value.sort_as().map(ToOwned::to_owned))).collect::<Vec<_>>();
    insert_values("alternate_title", &alternate_titles)?;
    for (kind, values) in [("rights", &metadata.rights), ("source", &metadata.sources), ("type", &metadata.types), ("format", &metadata.formats), ("relation", &metadata.relations), ("coverage", &metadata.coverage)] {
        let values = values.iter().cloned().map(|value| (value, None, None)).collect::<Vec<_>>();
        insert_values(kind, &values)?;
    }
    let properties = metadata.properties.iter().map(|property| (property.value().to_owned(), property.refines().map(ToOwned::to_owned), Some(property.property().to_owned()))).collect::<Vec<_>>();
    insert_values("property", &properties)?;
    // Derived lookup references, not identifiers of the local edition.
    insert_values("enrichment_isbn", &prepared.references)?;
    Ok(())
}

fn ensure_external_subject_nodes(transaction: &Transaction, system_id: &str, subject_path: &str, matcher_version: i64) -> Result<(), rusqlite::Error> {
    let components = subject_path.split(" / ").map(str::trim).filter(|component| !component.is_empty()).collect::<Vec<_>>();
    transaction.execute(include_str!("sql/metadata/ensure_external_subject_nodes_insert.sql"), [system_id])?;
    let mut path = String::new();
    let mut parent_path = String::new();
    for component in components {
        if !path.is_empty() {
            path.push_str(" / ");
        }
        path.push_str(component);
        transaction.execute(include_str!("sql/metadata/ensure_external_subject_nodes_insert_2.sql"), rusqlite::params![system_id, path, parent_path, component])?;
        parent_path.clone_from(&path);
    }
    transaction.execute(include_str!("sql/metadata/ensure_external_subject_nodes_update.sql"), rusqlite::params![system_id, matcher_version])?;
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
    transaction.execute(include_str!("sql/metadata/reidentify_book_credit_update.sql"), rusqlite::params![identity_local_id, content_hash.as_str(), i64::try_from(position).unwrap_or(i64::MAX)])?;
    Ok(())
}
/// Splits one credited occurrence away from an author that turned out to be a
/// namesake. The credited spelling and normalized grouping key remain intact;
/// only the stable identity referenced by this book position changes. Scoped
/// to author-role (`"aut"`) rows.
#[cfg(test)]
pub fn split_book_author(transaction: &Transaction, content_hash: ContentHash, position: usize) -> Result<AuthorId, rusqlite::Error> {
    let credited_name: String = transaction.query_row(include_str!("sql/metadata/split_book_author_select.sql"), rusqlite::params![content_hash.as_str(), i64::try_from(position).unwrap_or(i64::MAX)], |row| row.get(0))?;
    let preferred_name = AuthorName::parse(&credited_name).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let author_id = AgentId::new_v4();
    transaction.execute(include_str!("sql/metadata/split_book_author_insert.sql"), rusqlite::params![author_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?])?;
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
    let (current_local_id, _current_stable_id, current_is_provisional): (i64, String, bool) =
        transaction.query_row(include_str!("sql/metadata/resolve_book_author_identity_select.sql"), rusqlite::params![content_hash.as_str(), position], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    let existing: Option<(i64, String)> =
        transaction.query_row(include_str!("sql/metadata/resolve_book_author_identity_select_2.sql"), rusqlite::params![external_id.authority().as_str(), external_id.value()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
    let authority_stable_id = library_model::agent_id_from_migration_seed(format!("external-author:{}:{}", external_id.authority().as_str(), external_id.value()).as_bytes());
    let stable_identity = transaction.query_row(include_str!("sql/metadata/resolve_book_author_identity_select_3.sql"), [authority_stable_id.to_string()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).optional()?;
    let current_credit_count: i64 = transaction.query_row(include_str!("sql/metadata/resolve_book_author_identity_select_4.sql"), [current_local_id], |row| row.get(0))?;
    let (target_local_id, target_stable_id) = match existing {
        Some(identity) => identity,
        None if stable_identity.is_some() => stable_identity.expect("checked above"),
        None if current_is_provisional && current_credit_count == 1 => {
            transaction.execute(include_str!("sql/metadata/resolve_book_author_identity_update.sql"), rusqlite::params![authority_stable_id.to_string(), current_local_id])?;
            (current_local_id, authority_stable_id.to_string())
        }
        None => {
            transaction.execute(include_str!("sql/metadata/resolve_book_author_identity_insert.sql"), rusqlite::params![authority_stable_id.to_string(), preferred_name.as_str(), preferred_name.match_key().as_str(), unix_millis()?])?;
            (transaction.last_insert_rowid(), authority_stable_id.to_string())
        }
    };
    let target_author_id = AgentId::parse_str(&target_stable_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
    transaction.execute(include_str!("sql/metadata/resolve_book_author_identity_update_2.sql"), rusqlite::params![preferred_name.as_str(), preferred_name.match_key().as_str(), target_local_id])?;
    transaction.execute(include_str!("sql/metadata/attach_external_author_id_insert.sql"), rusqlite::params![target_local_id, external_id.authority().as_str(), external_id.value(), unix_millis()?])?;
    // Update even when the local row is unchanged. The contributor trigger
    // refreshes the already-queued metadata mutation with the authority-derived
    // stable id before the first upload.
    reidentify_book_credit(transaction, content_hash, usize::try_from(position).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?, target_local_id, target_author_id)?;
    if target_local_id != current_local_id {
        transaction.execute(include_str!("sql/metadata/resolve_book_author_identity_delete.sql"), [current_local_id])?;
    }
    Ok(target_author_id)
}
#[cfg(test)]
pub fn attach_external_author_id(transaction: &Transaction, author_id: AuthorId, external_id: &ExternalAuthorId) -> Result<(), rusqlite::Error> {
    let identity_local_id: i64 = transaction.query_row(include_str!("sql/metadata/attach_external_author_id_select.sql"), [author_id.to_string()], |row| row.get(0))?;
    transaction.execute(include_str!("sql/metadata/attach_external_author_id_insert.sql"), rusqlite::params![identity_local_id, external_id.authority().as_str(), external_id.value(), unix_millis()?])?;
    let owner: i64 = transaction.query_row(include_str!("sql/metadata/attach_external_author_id_select_2.sql"), rusqlite::params![external_id.authority().as_str(), external_id.value()], |row| row.get(0))?;
    if owner != identity_local_id {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "external author identifier is already attached to another identity"))));
    }
    transaction.execute(include_str!("sql/metadata/attach_external_author_id_update.sql"), [identity_local_id])?;
    Ok(())
}
#[cfg(test)]
pub fn get_book_author_identities(conn: &Connection, content_hash: ContentHash) -> Result<Vec<AuthorCreditIdentity>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/metadata/get_book_author_identities_select.sql"))?;
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
pub fn update_book_metadata_contributors(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, content_hash, title, contributors, AuthorIdPolicy::ReuseProvisionalNameCandidate)
}
#[cfg(test)]
pub(crate) fn update_book_metadata_contributors_preserving_ids(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor]) -> Result<(), rusqlite::Error> {
    update_book_metadata_contributors_with_policy(transaction, content_hash, title, contributors, AuthorIdPolicy::PreserveIncoming)
}
fn update_book_metadata_contributors_with_policy(transaction: &Transaction, content_hash: ContentHash, title: &str, contributors: &[Contributor], policy: AuthorIdPolicy) -> Result<(), rusqlite::Error> {
    super::metadata_batch::run(transaction, || {
        let stored_contributors = prepare_contributors(transaction, contributors, policy, Some(content_hash.clone()))?;
        transaction.update_book_title(title, content_hash.as_str())?;
        normalize_stored_title(transaction, content_hash)?;
        replace_book_contributors(transaction, content_hash, &stored_contributors)?;
        Ok(())
    })
}
pub fn update_book_description(transaction: &Transaction, content_hash: ContentHash, description: &str) -> Result<(), rusqlite::Error> {
    let normalized = normalized_description(description);
    super::descriptions::set(transaction, content_hash, &normalized)?;
    Ok(())
}
pub(crate) fn update_missing_description(transaction: &Transaction, content_hash: &ContentHash, description: &str) -> Result<bool, rusqlite::Error> {
    let normalized = normalized_description(description);
    if normalized.trim().is_empty() {
        return Ok(false);
    }
    let missing: bool = transaction.query_row(include_str!("sql/metadata/update_missing_description_select.sql"), [content_hash.as_str()], |r| r.get(0))?;
    if missing {
        super::descriptions::set(transaction, *content_hash, &normalized)?;
    }
    Ok(missing)
}
pub(super) fn normalized_description(description: &str) -> String {
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
    let previous = stored_publication(transaction, content_hash)?;
    if previous.as_ref() == Some(metadata) {
        return Ok(());
    }
    let metadata = prepare_publication_agents(transaction, metadata, policy)?;
    let encoded = sync_common::wire::encode(&metadata).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.update_book_publication_metadata(&encoded, content_hash.as_str())?;
    replace_publication_metadata_projections(transaction, content_hash, &metadata, previous.as_ref())?;
    Ok(())
}
pub(super) fn update_book_publication_metadata_preserving_ids(transaction: &Transaction, content_hash: ContentHash, metadata: &PublicationMetadata) -> Result<(), rusqlite::Error> {
    update_book_publication_metadata(transaction, content_hash, metadata, AuthorIdPolicy::PreserveIncoming)
}
pub(super) fn update_book_metadata_state_preserving_ids(transaction: &Transaction, content_hash: ContentHash, metadata: &SyncBookMetadata) -> Result<(), rusqlite::Error> {
    update_book_metadata_with_snapshot(transaction, content_hash, metadata, None)
}

fn update_book_metadata_with_snapshot(transaction: &Transaction, content_hash: ContentHash, metadata: &SyncBookMetadata, snapshot: Option<(Option<PublicationMetadata>, bool)>) -> rusqlite::Result<()> {
    super::metadata_batch::run(transaction, || {
        let (previous, contributors_match) = match snapshot {
            Some(snapshot) => snapshot,
            None => (stored_publication(transaction, content_hash)?, super::authors::contributors_match(transaction, content_hash, &metadata.contributors)?),
        };
        let contributors = if contributors_match { None } else { Some(prepare_contributors(transaction, &metadata.contributors, AuthorIdPolicy::PreserveIncoming, Some(content_hash))?) };
        if previous.as_ref() == Some(&metadata.publication) {
            transaction.prepare_cached(include_str!("sql/metadata_delta_tests/before_contributor_fast_path_update.sql"))?.execute(rusqlite::params![content_hash.as_str(), metadata.title])?;
            if let Some(contributors) = contributors {
                replace_book_contributors(transaction, content_hash, &contributors)?;
            }
            return Ok(());
        }
        let publication = prepare_publication_agents(transaction, &metadata.publication, AuthorIdPolicy::PreserveIncoming)?;
        let encoded = sync_common::wire::encode(&publication).map_err(|e| invalid_text_column(0, e.to_string()))?;
        // One book-row update, not three successive trigger/outbox refreshes.
        transaction.execute(include_str!("sql/metadata/update_book_metadata_state_preserving_ids_update_2.sql"), rusqlite::params![content_hash.as_str(), metadata.title, encoded])?;
        if let Some(contributors) = contributors {
            replace_book_contributors(transaction, content_hash, &contributors)?;
        }
        replace_publication_metadata_projections(transaction, content_hash, &publication, previous.as_ref())
    })
}

#[cfg(test)]
#[path = "delta_tests.rs"]
mod delta_tests;

/// Normalize stored metadata without reinspecting book files. Normal mutation
/// triggers publish the title and subtitle together to other replicas.
pub(super) fn normalize_stored_title(transaction: &Transaction, hash: ContentHash) -> rusqlite::Result<bool> {
    let (original, raw): (String, Vec<u8>) = transaction.query_row(include_str!("sql/metadata/normalize_stored_title_select.sql"), [hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut title = original.clone();
    let mut publication = super::external_metadata::decode_publication_metadata(&raw)?;
    let previous = publication.clone();
    publication.normalize_title(&mut title);
    if title == original && publication == previous {
        return Ok(false);
    }
    super::metadata_batch::run(transaction, || {
        update_book_publication_metadata_preserving_ids(transaction, hash, &publication)?;
        transaction.update_book_title(&title, hash.as_str())?;
        Ok(())
    })?;
    Ok(true)
}

mod taxonomy;
pub use taxonomy::*;

pub(crate) mod queries;

mod batch_reads;
#[cfg(test)]
use crate::metadata_batch;
pub(crate) use batch_reads::update_independent_metadata;
