//! Browser entry points for transport-independent Rust domain behavior.
//!
//! Storage and worker messaging remain browser-specific. Identity and search
//! semantics are the same implementations used by the native applications.

use std::cell::RefCell;
use std::io::{self, Cursor};

use serde::{Deserialize, Serialize};

const MAX_COVER_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct PublicationMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub subject_paths: Vec<String>,
    pub subject_codes: Vec<PublicationTaxonomyCode>,
    pub isbns: Vec<String>,
    pub cover: Option<Vec<u8>>,
    pub toc: Vec<BookTocEntry>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PublicationTaxonomyCode {
    pub system: String,
    pub code: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BookTocEntry {
    pub title: String,
    pub target: String,
    pub children: Vec<BookTocEntry>,
}

fn toc_entry(entry: epub_provider::TocEntry) -> BookTocEntry {
    BookTocEntry { title: entry.title, target: entry.link, children: entry.children.into_iter().map(toc_entry).collect() }
}

thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn content_hash(input: &[u8]) -> String {
    blake3::hash(input).to_hex().to_string()
}

fn project_subject_codes(input: &[u8]) -> Result<Vec<u8>, String> {
    #[derive(Deserialize)]
    struct Subject {
        #[serde(default)]
        id: Option<String>,
        #[serde(alias = "value")]
        name: String,
        #[serde(default = "default_subject_source")]
        source: String,
        #[serde(default)]
        authority: Option<String>,
        #[serde(default)]
        code: Option<String>,
    }
    #[derive(Serialize)]
    struct Code<'a> {
        system: &'a str,
        code: &'a str,
    }
    fn default_subject_source() -> String {
        "dc:subject".to_owned()
    }

    let subjects = serde_json::from_slice::<Vec<Subject>>(input).map_err(|error| error.to_string())?;
    let subjects = subjects.into_iter().map(|subject| library_model::PublicationSubject::new(subject.id, subject.name, subject.source, subject.authority, subject.code)).collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    let codes = library_model::project_subject_codes(&subjects);
    serde_json::to_vec(&codes.iter().map(|code| Code { system: code.system_id(), code: code.code() }).collect::<Vec<_>>()).map_err(|error| error.to_string())
}

pub fn inspect_epub(input: &[u8]) -> io::Result<PublicationMetadata> {
    let provider = epub_provider::EpubProvider::try_from_reader(Cursor::new(input.to_vec()))?;
    let metadata = provider.metadata()?;
    let title = non_empty(metadata.title);
    let author =
        metadata.creators.iter().find(|creator| matches!(creator.role.as_str().trim().to_ascii_lowercase().as_str(), "aut" | "author" | "writer")).or_else(|| metadata.creators.first()).and_then(|creator| non_empty(creator.name.clone()));
    let description = non_empty(metadata.description);
    let subjects = metadata.subjects.iter().filter_map(|subject| non_empty(subject.value.clone())).collect::<Vec<_>>();
    let model_subjects = metadata.subjects.iter().filter_map(|subject| library_model::PublicationSubject::new(subject.id.clone(), &subject.value, &subject.source, subject.authority.clone(), subject.code.clone()).ok()).collect::<Vec<_>>();
    let subject_paths = library_model::project_unified_subjects(&model_subjects).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect();
    let subject_codes = library_model::project_subject_codes(&model_subjects).into_iter().map(|classification| PublicationTaxonomyCode { system: classification.system_id().to_owned(), code: classification.code().to_owned() }).collect();
    let mut isbns = metadata.identifiers.iter().filter(|identifier| identifier.scheme == epub_provider::IdentifierScheme::ISBN).filter_map(|identifier| library_model::canonical_isbn(&identifier.value)).collect::<Vec<_>>();
    isbns.sort();
    isbns.dedup();
    let cover = match provider.cover_image_path()? {
        Some(path) => provider.read_bytes(&path).ok().filter(|bytes| bytes.len() <= MAX_COVER_BYTES).and_then(|bytes| thumbnail::generate_thumbnail_from_image_bytes(&bytes).ok()),
        None => None,
    };
    let toc = provider.toc()?.unwrap_or_default().into_iter().map(toc_entry).collect();
    Ok(PublicationMetadata { title, author, description, subjects, subject_paths, subject_codes, isbns, cover, toc })
}

pub fn apply_reader_preference_patch(preferences: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    let mut preferences: app_contract::preferences::ReaderPreferences = serde_json::from_slice(preferences).map_err(|error| error.to_string())?;
    let patch: app_contract::preferences::ReaderPreferencePatch = serde_json::from_slice(patch).map_err(|error| error.to_string())?;
    patch.apply(&mut preferences).map_err(|error| error.to_string())?;
    serde_json::to_vec(&preferences).map_err(|error| error.to_string())
}

pub fn apply_browsing_preference_patch(preferences: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    let mut preferences: app_contract::preferences::BrowsingPreferences = serde_json::from_slice(preferences).map_err(|error| error.to_string())?;
    let patch: app_contract::preferences::BrowsingPreferencePatch = serde_json::from_slice(patch).map_err(|error| error.to_string())?;
    patch.apply(&mut preferences).map_err(|error| error.to_string())?;
    serde_json::to_vec(&preferences).map_err(|error| error.to_string())
}

pub fn encode_account_request(kind: u32, json: &[u8]) -> Result<Vec<u8>, String> {
    macro_rules! encode {
        ($type:ty) => {{
            let value: $type = serde_json::from_slice(json).map_err(|error| error.to_string())?;
            wire::encode(&value).map_err(|error| error.to_string())
        }};
    }
    match kind {
        1 => encode!(account_contract::LoginRequest),
        2 => encode!(account_contract::PublicRegistrationRequest),
        3 => encode!(account_contract::EmailRequest),
        4 => encode!(account_contract::EmailVerificationRequest),
        5 => encode!(account_contract::ResetPasswordRequest),
        6 => encode!(account_contract::RefreshRequest),
        7 => encode!(sync_common::api::libraries::UpsertLibraryRequest),
        8 => encode!(sync_common::api::assets::BlobManifestRequest),
        _ => Err("unknown account request kind".to_owned()),
    }
}

pub fn decode_account_response(kind: u32, bytes: &[u8]) -> Result<Vec<u8>, String> {
    macro_rules! decode {
        ($type:ty) => {{
            let value: $type = wire::decode(bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| error.to_string())?;
            serde_json::to_vec(&value).map_err(|error| error.to_string())
        }};
    }
    match kind {
        1 => decode!(account_contract::AuthResponse),
        2 => decode!(account_contract::MeResponse),
        3 => decode!(account_contract::StorageUsageResponse),
        4 => decode!(Vec<sync_common::api::libraries::LibrarySummary>),
        5 => decode!(sync_common::api::libraries::DeletedLibrariesResponse),
        6 => decode!(sync_common::api::libraries::LibrarySummary),
        8 => decode!(sync_common::api::assets::BlobManifestResponse),
        9 => decode!(account_contract::LibraryStorageUsageResponse),
        _ => Err("unknown account response kind".to_owned()),
    }
}

pub fn decode_blob_manifest_response(bytes: &[u8], request_json: &[u8]) -> Result<Vec<u8>, String> {
    use sync_common::api::assets::{validate_blob_manifest_response, BlobManifestRequest, BlobManifestResponse};
    let request: BlobManifestRequest = serde_json::from_slice(request_json).map_err(|error| error.to_string())?;
    let response: BlobManifestResponse = wire::decode(bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| error.to_string())?;
    validate_blob_manifest_response(&request, &response).map_err(|error| error.to_string())?;
    serde_json::to_vec(&response).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct BrowserSyncMutation {
    mutation_id: String,
    kind: String,
    entity_key: String,
    #[serde(default)]
    entity_subkey: String,
    value: serde_json::Value,
    changed_at: u64,
    replica_seq: u64,
}

#[derive(Deserialize)]
struct BrowserSyncExchangeRequest {
    library_id: String,
    replica_id: String,
    cursor: BrowserSyncCursor,
    mutations: Vec<BrowserSyncMutation>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
struct BrowserSyncCursor {
    state_revision: Option<u64>,
    reading_revision: Option<u64>,
}

impl BrowserSyncCursor {
    fn into_protocol(self) -> Result<sync_common::SyncCursor, String> {
        Ok(sync_common::SyncCursor {
            state_revision: self.state_revision.map(sync_common::LibraryRevision::new).transpose().map_err(|error| error.to_string())?,
            reading_revision: self.reading_revision.map(sync_common::LibraryRevision::new).transpose().map_err(|error| error.to_string())?,
        })
    }
}

#[derive(Serialize)]
struct BrowserServerMutation {
    mutation_id: String,
    kind: String,
    entity_key: String,
    entity_subkey: String,
    value: serde_json::Value,
    conflict_rank: u8,
    changed_at: u64,
    replica_seq: u64,
    replica_id: String,
    revision: u64,
}

#[derive(Serialize)]
struct BrowserSyncExchangeResponse {
    accepted: Vec<String>,
    rejected: Vec<(String, String, bool)>,
    mutations: Vec<BrowserServerMutation>,
    next_cursor: BrowserSyncCursor,
    has_more: bool,
}

#[derive(Deserialize)]
struct BrowserVersionedValue<T> {
    value: T,
    changed_at: u64,
    conflict_rank: u8,
    replica_id: String,
    replica_seq: u64,
    mutation_id: String,
}

#[derive(Deserialize)]
struct BrowserDirectoryIntent {
    id: String,
    name: BrowserVersionedValue<String>,
    parent: BrowserVersionedValue<String>,
    lifecycle: BrowserVersionedValue<library_model::DirectoryLifecycleState>,
}

#[derive(Serialize)]
struct BrowserDirectoryProjection {
    ordered_active: Vec<String>,
    suppressed: Vec<String>,
    names: Vec<(String, String)>,
}

fn browser_version<T>(value: &BrowserVersionedValue<T>) -> Result<library_replica::VersionKey, String> {
    Ok(library_replica::VersionKey::new(
        value.changed_at,
        value.conflict_rank,
        uuid::Uuid::parse_str(&value.replica_id).map_err(|error| error.to_string())?,
        value.replica_seq,
        sync_common::MutationId::parse(&value.mutation_id).map_err(|error| error.to_string())?,
    ))
}

pub fn project_browser_directories(json: &[u8]) -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    let input: Vec<BrowserDirectoryIntent> = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let intents = input
        .into_iter()
        .map(|intent| {
            let id = uuid::Uuid::parse_str(&intent.id).map_err(|error| error.to_string())?;
            let parent_id = uuid::Uuid::parse_str(&intent.parent.value).map_err(|error| error.to_string())?;
            let version = browser_version(&intent.name)?.max(browser_version(&intent.parent)?);
            Ok((id, library_replica::DirectoryIntent { id, parent_id, name: intent.name.value, lifecycle: intent.lifecycle.value, version }))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let projection = library_replica::project_directories(&intents);
    let mut suppressed = projection.suppressed.into_iter().map(|id| id.to_string()).collect::<Vec<_>>();
    suppressed.sort();
    let mut names = projection.names.into_iter().map(|(id, name)| (id.to_string(), name)).collect::<Vec<_>>();
    names.sort_by(|left, right| left.0.cmp(&right.0));
    let output = BrowserDirectoryProjection { ordered_active: projection.ordered_active.into_iter().map(|id| id.to_string()).collect(), suppressed, names };
    serde_json::to_vec(&output).map_err(|error| error.to_string())
}

#[derive(Deserialize, Serialize)]
struct BrowserFolderDestination {
    id: String,
    path: String,
}

#[derive(Deserialize)]
struct BrowserFolderDestinationRequest {
    destinations: Vec<BrowserFolderDestination>,
    policy: library_model::FolderDestinationPolicy,
}

pub fn select_browser_folder_destinations(json: &[u8]) -> Result<Vec<u8>, String> {
    let request: BrowserFolderDestinationRequest = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let destinations = request.destinations.into_iter().filter(|destination| request.policy.can_select(&destination.id, &destination.path)).collect::<Vec<_>>();
    serde_json::to_vec(&destinations).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct BrowserUniqueNameRequest {
    kind: String,
    requested: String,
    occupied: Vec<String>,
}

pub fn choose_browser_library_name(json: &[u8]) -> Result<Vec<u8>, String> {
    let request: BrowserUniqueNameRequest = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let occupied = request.occupied.iter().map(String::as_str);
    let chosen = match request.kind.as_str() {
        "folder" => library_model::unique_folder_name(&request.requested, occupied),
        "file" => library_model::unique_file_name(&request.requested, occupied),
        _ => return Err("unknown library name kind".to_owned()),
    };
    serde_json::to_vec(&chosen).map_err(|error| error.to_string())
}

fn browser_mutation_body(mutation: &BrowserSyncMutation) -> Result<library_replica::MutationBody, String> {
    use library_replica::MutationBody;
    let key = &mutation.entity_key;
    let value = mutation.value.clone();
    let parse_dir = |value: &str| uuid::Uuid::parse_str(value).map_err(|error| error.to_string());
    match mutation.kind.as_str() {
        sync_common::mutation_kind::DIRECTORY_NAME => Ok(MutationBody::DirectoryName { dir_id: parse_dir(key)?, name: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::DIRECTORY_PARENT => Ok(MutationBody::DirectoryParent { dir_id: parse_dir(key)?, parent_id: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::DIRECTORY_LIFECYCLE => Ok(MutationBody::DirectoryLifecycle { dir_id: parse_dir(key)?, value: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::BOOK_LIFECYCLE => Ok(MutationBody::BookLifecycle { content_hash: sync_common::ContentHash::new(key), value: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::PLACEMENT => {
            #[derive(Deserialize)]
            struct Placement {
                present: bool,
                origin_folder_id: Option<String>,
            }
            let value: Placement = serde_json::from_value(value).map_err(|error| error.to_string())?;
            Ok(MutationBody::Placement { dir_id: parse_dir(&mutation.entity_subkey)?, content_hash: sync_common::ContentHash::new(key), present: value.present, origin_folder_id: value.origin_folder_id })
        }
        sync_common::mutation_kind::READING_POSITION => Ok(MutationBody::ReadingPosition { content_hash: sync_common::ContentHash::new(key), value: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::METADATA => Ok(MutationBody::Metadata { content_hash: sync_common::ContentHash::new(key), value: serde_json::from_value(value).map_err(|error| error.to_string())? }),
        sync_common::mutation_kind::CATALOG_PUBLICATION => {
            #[derive(Deserialize)]
            struct CatalogPublicationValue {
                publication: catalog_contract::CatalogPublicationIdentity,
                content_hash: String,
            }
            let value: CatalogPublicationValue = serde_json::from_value(value).map_err(|error| error.to_string())?;
            Ok(MutationBody::CatalogPublication { source_id: catalog_contract::CatalogSourceId::parse(key)?, publication: value.publication, content_hash: sync_common::ContentHash::new(&value.content_hash) })
        }
        _ => Err(format!("unsupported browser sync kind {}", mutation.kind)),
    }
}

pub fn encode_sync_exchange(json: &[u8]) -> Result<Vec<u8>, String> {
    let input: BrowserSyncExchangeRequest = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let mutations = input
        .mutations
        .iter()
        .map(|mutation| {
            let body = browser_mutation_body(mutation)?;
            body.to_wire(sync_common::MutationId::parse(&mutation.mutation_id).map_err(|error| error.to_string())?, mutation.changed_at, sync_common::ReplicaSeq::new(mutation.replica_seq).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let request = sync_common::SyncExchangeRequest {
        library_id: uuid::Uuid::parse_str(&input.library_id).map_err(|error| error.to_string())?,
        replica_id: uuid::Uuid::parse_str(&input.replica_id).map_err(|error| error.to_string())?,
        mutations,
        cursor: input.cursor.into_protocol()?,
    };
    wire::encode(&request).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct BrowserExchangeValidation {
    requested: Vec<String>,
    cursor: BrowserSyncCursor,
}

pub fn decode_sync_exchange(bytes: &[u8], validation: &[u8]) -> Result<Vec<u8>, String> {
    let response: sync_common::SyncExchangeResponse = wire::decode(bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| error.to_string())?;
    let validation: BrowserExchangeValidation = serde_json::from_slice(validation).map_err(|error| error.to_string())?;
    let requested = validation.requested.iter().map(|id| sync_common::MutationId::parse(id).map_err(|error| error.to_string())).collect::<Result<Vec<_>, _>>()?;
    sync_common::validate_push_response(&requested, &response.push).map_err(|error| error.to_string())?;
    let cursor = validation.cursor.into_protocol()?;
    sync_common::validate_pull_batch(&response.pull, cursor).map_err(|error| error.to_string())?;
    let mutations = response
        .pull
        .mutations
        .into_iter()
        .map(|change| {
            let body = library_replica::MutationBody::from_wire(&change.mutation).map_err(|error| error.to_string())?;
            let value = body.map(browser_mutation_value).transpose()?.unwrap_or(serde_json::Value::Null);
            Ok(BrowserServerMutation {
                mutation_id: change.mutation.mutation_id.to_string(),
                kind: change.mutation.kind,
                entity_key: change.mutation.entity_key,
                entity_subkey: change.mutation.entity_subkey,
                value,
                conflict_rank: change.mutation.conflict_rank,
                changed_at: change.mutation.changed_at,
                replica_seq: change.mutation.replica_seq.get(),
                replica_id: change.replica_id.to_string(),
                revision: change.revision.get(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let output = BrowserSyncExchangeResponse {
        accepted: response.push.accepted.into_iter().map(|id| id.to_string()).collect(),
        rejected: response
            .push
            .rejected
            .into_iter()
            .map(|rejection| {
                let transient = rejection.reason.is_transient();
                (rejection.mutation_id.to_string(), format!("{:?}", rejection.reason), transient)
            })
            .collect(),
        mutations,
        next_cursor: BrowserSyncCursor { state_revision: response.pull.next_cursor.state_revision.map(|cursor| cursor.get()), reading_revision: response.pull.next_cursor.reading_revision.map(|cursor| cursor.get()) },
        has_more: response.pull.has_more,
    };
    serde_json::to_vec(&output).map_err(|error| error.to_string())
}

fn browser_mutation_value(body: library_replica::MutationBody) -> Result<serde_json::Value, String> {
    use library_replica::MutationBody;
    let value = match body {
        MutationBody::DirectoryName { name, .. } => serde_json::to_value(name),
        MutationBody::DirectoryParent { parent_id, .. } => serde_json::to_value(parent_id),
        MutationBody::DirectoryLifecycle { value, .. } => serde_json::to_value(value),
        MutationBody::BookLifecycle { value, .. } => {
            let subject_paths = match &value {
                library_model::BookLifecycleState::Present { metadata, .. } => {
                    Some(library_model::project_unified_subjects(&metadata.publication.subjects).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect::<Vec<_>>())
                }
                _ => None,
            };
            let mut encoded = serde_json::to_value(value).map_err(|error| error.to_string())?;
            if let Some(subject_paths) = subject_paths {
                insert_subject_paths(&mut encoded, "/present/metadata/publication", subject_paths).map_err(|error| error.to_string())?;
            }
            Ok(encoded)
        }
        MutationBody::Placement { present, origin_folder_id, .. } => serde_json::to_value(serde_json::json!({ "present": present, "origin_folder_id": origin_folder_id })),
        MutationBody::ReadingPosition { value, .. } => serde_json::to_value(value),
        MutationBody::Metadata { value, .. } => {
            let subject_paths = library_model::project_unified_subjects(&value.publication.subjects).into_iter().map(|assignment| assignment.subject_path().to_owned()).collect::<Vec<_>>();
            let mut encoded = serde_json::to_value(value).map_err(|error| error.to_string())?;
            insert_subject_paths(&mut encoded, "/publication", subject_paths).map_err(|error| error.to_string())?;
            Ok(encoded)
        }
        MutationBody::CatalogPublication { publication, content_hash, .. } => serde_json::to_value(serde_json::json!({
            "publication": publication,
            "content_hash": content_hash.to_string(),
        })),
        // These are outside the EPUB-only browser scope. Their opaque cells
        // still advance the durable cursor without entering the projection.
        MutationBody::Annotation { .. } | MutationBody::Bookmark { .. } | MutationBody::BookLink { .. } => Ok(serde_json::Value::Null),
    };
    value.map_err(|error| error.to_string())
}

fn insert_subject_paths(value: &mut serde_json::Value, pointer: &str, paths: Vec<String>) -> Result<(), serde_json::Error> {
    if let Some(serde_json::Value::Object(publication)) = value.pointer_mut(pointer) {
        publication.insert("subject_paths".to_owned(), serde_json::to_value(paths)?);
    }
    Ok(())
}

fn store_result(value: Result<Vec<u8>, String>) -> u32 {
    let (status, bytes) = match value {
        Ok(bytes) => (0, bytes),
        Err(error) => (1, error.into_bytes()),
    };
    RESULT.with(|result| *result.borrow_mut() = bytes);
    status
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn encode_metadata(metadata: PublicationMetadata) -> Vec<u8> {
    let title = metadata.title.as_deref().map(str::as_bytes);
    let author = metadata.author.as_deref().map(str::as_bytes);
    let description = metadata.description.as_deref().map(str::as_bytes);
    let subjects = serde_json::to_vec(&metadata.subjects).expect("EPUB subjects serialize");
    let subject_paths = serde_json::to_vec(&metadata.subject_paths).expect("EPUB subject paths serialize");
    let subject_codes = serde_json::to_vec(&metadata.subject_codes).expect("EPUB subject codes serialize");
    let isbns = serde_json::to_vec(&metadata.isbns).expect("EPUB ISBNs serialize");
    let cover = metadata.cover.as_deref();
    let toc = serde_json::to_vec(&metadata.toc).expect("EPUB TOC serializes");
    let mut output = Vec::with_capacity(
        36 + title.map_or(0, <[u8]>::len) + author.map_or(0, <[u8]>::len) + description.map_or(0, <[u8]>::len) + subjects.len() + subject_paths.len() + subject_codes.len() + isbns.len() + cover.map_or(0, <[u8]>::len) + toc.len(),
    );
    for value in [title, author, description, Some(subjects.as_slice()), Some(subject_paths.as_slice()), Some(subject_codes.as_slice()), Some(isbns.as_slice()), cover, Some(toc.as_slice())] {
        output.extend_from_slice(&value.map_or(u32::MAX, |bytes| bytes.len() as u32).to_le_bytes());
    }
    for value in [title, author, description, Some(subjects.as_slice()), Some(subject_paths.as_slice()), Some(subject_codes.as_slice()), Some(isbns.as_slice()), cover, Some(toc.as_slice())].into_iter().flatten() {
        output.extend_from_slice(value);
    }
    output
}

#[unsafe(no_mangle)]
pub extern "C" fn bokheim_alloc(length: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(length);
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

/// # Safety
/// `pointer` and `length` must describe a buffer returned by `bokheim_alloc`
/// which has not previously been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_dealloc(pointer: *mut u8, length: usize) {
    if length != 0 {
        // SAFETY: guaranteed by the caller as documented above.
        drop(unsafe { Vec::from_raw_parts(pointer, length, length) });
    }
}

/// # Safety
/// The input must reference `input_length` readable bytes and the output must
/// reference at least 64 writable bytes in this module's linear memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_blake3_hex(input: *const u8, input_length: usize, output: *mut u8) {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let digest = content_hash(input);
    // SAFETY: output is documented to hold at least 64 bytes and cannot overlap input.
    unsafe { std::ptr::copy_nonoverlapping(digest.as_ptr(), output, digest.len()) };
}

/// Returns the required output length. If it fits in `output_capacity`, the
/// normalized UTF-8 bytes are also written to `output`.
///
/// # Safety
/// Input must be valid UTF-8 and both pointers must describe readable/writable
/// regions of the stated sizes in this module's linear memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_normalize_search_text(input: *const u8, input_length: usize, output: *mut u8, output_capacity: usize) -> usize {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let value = std::str::from_utf8(input).expect("browser passed invalid UTF-8");
    let normalized = catalog_model::normalize_search_text(value);
    if normalized.len() <= output_capacity {
        // SAFETY: the capacity check and caller contract guarantee this write.
        unsafe { std::ptr::copy_nonoverlapping(normalized.as_ptr(), output, normalized.len()) };
    }
    normalized.len()
}

/// Parses EPUB package metadata with the shared EPUB provider and stores the
/// encoded result until the next inspection. Returns zero on success.
///
/// # Safety
/// Input must reference `input_length` readable bytes in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_inspect_epub(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    match inspect_epub(input) {
        Ok(metadata) => {
            RESULT.with(|result| *result.borrow_mut() = encode_metadata(metadata));
            0
        }
        Err(_) => {
            RESULT.with(|result| result.borrow_mut().clear());
            1
        }
    }
}

/// Applies the shared reader-preference validation and stores either the
/// updated preferences (success) or a UTF-8 error message (failure).
///
/// # Safety
/// Both inputs must reference readable UTF-8 JSON regions of their stated
/// sizes in this module's linear memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_apply_reader_preference_patch(preferences: *const u8, preferences_length: usize, patch: *const u8, patch_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let preferences = unsafe { std::slice::from_raw_parts(preferences, preferences_length) };
    // SAFETY: guaranteed by the caller as documented above.
    let patch = unsafe { std::slice::from_raw_parts(patch, patch_length) };
    store_result(apply_reader_preference_patch(preferences, patch))
}

/// Applies shared browsing-preference validation.
///
/// # Safety
/// Both inputs must reference readable UTF-8 JSON regions of their stated sizes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_apply_browsing_preference_patch(preferences: *const u8, preferences_length: usize, patch: *const u8, patch_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let preferences = unsafe { std::slice::from_raw_parts(preferences, preferences_length) };
    // SAFETY: guaranteed by the caller as documented above.
    let patch = unsafe { std::slice::from_raw_parts(patch, patch_length) };
    store_result(apply_browsing_preference_patch(preferences, patch))
}

/// Encodes JSON into the versioned account HTTP wire format.
///
/// # Safety
/// Input must reference a readable UTF-8 JSON region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_encode_account_request(kind: u32, input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(encode_account_request(kind, input))
}

/// Decodes a versioned account HTTP response into JSON.
///
/// # Safety
/// Input must reference a readable byte region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_decode_account_response(kind: u32, input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(decode_account_response(kind, input))
}

/// Decodes and validates a blob negotiation response against its request.
///
/// # Safety
/// Both inputs must reference readable regions of their stated sizes in this
/// module's memory; the request must be UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_decode_blob_manifest_response(input: *const u8, input_length: usize, request: *const u8, request_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    // SAFETY: guaranteed by the caller as documented above.
    let request = unsafe { std::slice::from_raw_parts(request, request_length) };
    store_result(decode_blob_manifest_response(input, request))
}

/// Encodes browser outbox JSON as the shared synchronization exchange.
///
/// # Safety
/// Input must reference a readable UTF-8 JSON region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_encode_sync_exchange(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(encode_sync_exchange(input))
}

/// Decodes the shared synchronization response into browser projection JSON.
///
/// # Safety
/// Input must reference a readable byte region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_decode_sync_exchange(input: *const u8, input_length: usize, validation: *const u8, validation_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    // SAFETY: guaranteed by the caller as documented above.
    let validation = unsafe { std::slice::from_raw_parts(validation, validation_length) };
    store_result(decode_sync_exchange(input, validation))
}

/// Projects directory state with the same collision and cycle rules as native.
///
/// # Safety
/// Input must reference a readable UTF-8 JSON region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_project_directories(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(project_browser_directories(input))
}

/// Resolves one DDC notation through the embedded DDC 23 summaries.
///
/// # Safety
/// Input must reference valid UTF-8 bytes for the stated length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_ddc_subject_path(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let result = std::str::from_utf8(input).map_err(|error| error.to_string()).and_then(|notation| library_model::ddc_subject_path(notation).ok_or_else(|| "invalid or unassigned DDC notation".to_owned())).map(String::into_bytes);
    store_result(result)
}

/// Resolves one LCC call number through the embedded official outline.
///
/// # Safety
/// Input must reference valid UTF-8 bytes for the stated length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_lcc_subject_path(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let result = std::str::from_utf8(input).map_err(|error| error.to_string()).and_then(|notation| library_model::lcc_subject_path(notation).ok_or_else(|| "invalid LCC call number".to_owned())).map(String::into_bytes);
    store_result(result)
}

/// Resolves a retained controlled-vocabulary code into unified browse paths.
///
/// # Safety
/// Input must reference UTF-8 JSON containing `system_id` and `code` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_unified_subject_paths(input: *const u8, input_length: usize) -> u32 {
    #[derive(serde::Deserialize)]
    struct Request {
        system_id: String,
        code: String,
    }

    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let result = serde_json::from_slice::<Request>(input).map(|request| library_model::unified_subject_paths(&request.system_id, &request.code)).and_then(|paths| serde_json::to_vec(&paths)).map_err(|error| error.to_string());
    store_result(result)
}

/// Projects raw publication subjects into every supported taxonomy code.
/// Inferred BISAC headings and explicit BISAC, DDC, and LCC classifications
/// are accumulated independently; finding one system never suppresses another.
///
/// # Safety
/// Input must reference UTF-8 JSON containing an array of publication subjects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_project_subject_codes(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(project_subject_codes(input))
}

/// Reports whether the embedded-plus-overlay taxonomy names an exact code.
///
/// # Safety
/// Input must reference UTF-8 JSON containing `system_id` and `code` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_unified_has_exact_source_code(input: *const u8, input_length: usize) -> u32 {
    #[derive(serde::Deserialize)]
    struct Request {
        system_id: String,
        code: String,
    }

    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let result = serde_json::from_slice::<Request>(input).map(|request| library_model::unified_has_exact_source_code(&request.system_id, &request.code)).and_then(|exact| serde_json::to_vec(&exact)).map_err(|error| error.to_string());
    store_result(result)
}

/// Replaces the browser's on-demand taxonomy overlay.
///
/// # Safety
/// Input must reference a JSON array of taxonomy concepts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_install_taxonomy_overlay(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    let result = serde_json::from_slice::<Vec<taxonomy_contract::TaxonomyConcept>>(input)
        .map_err(|error| error.to_string())
        .and_then(|concepts| {
            let definitions = concepts
                .into_iter()
                .map(|concept| {
                    let (parents, orders) = concept.parents.into_iter().map(|parent| (parent.concept_id, parent.display_order)).unzip();
                    library_model::UnifiedConceptDefinition::new_with_parent_sort_orders(concept.concept_id, concept.preferred_label, parents, orders, concept.source_selectors, concept.source_labels)
                })
                .collect();
            library_model::install_unified_taxonomy_overlay(definitions)
        })
        .map(|()| Vec::new());
    store_result(result)
}

/// Selects valid folder destinations using the shared native policy.
///
/// # Safety
/// Input must reference a readable UTF-8 JSON region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_select_folder_destinations(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(select_browser_folder_destinations(input))
}

/// Resolves folder and placement name collisions using the shared native rule.
///
/// # Safety
/// Input must reference a readable UTF-8 JSON region in this module's memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bokheim_choose_library_name(input: *const u8, input_length: usize) -> u32 {
    // SAFETY: guaranteed by the caller as documented above.
    let input = unsafe { std::slice::from_raw_parts(input, input_length) };
    store_result(choose_browser_library_name(input))
}

#[unsafe(no_mangle)]
pub extern "C" fn bokheim_result_pointer() -> *const u8 {
    RESULT.with(|result| result.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn bokheim_result_length() -> usize {
    RESULT.with(|result| result.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_shared_folder_destination_policy() {
        let request = serde_json::json!({
            "destinations": [
                { "id": "root", "path": "Library" },
                { "id": "parent", "path": "Shelf" },
                { "id": "source", "path": "Shelf / Child" },
                { "id": "nested", "path": "Shelf / Child / Nested" },
                { "id": "other", "path": "Other" }
            ],
            "policy": { "kind": "move_folder", "source_parent_id": "parent", "source_path": "Shelf / Child" }
        });
        let selected: serde_json::Value = serde_json::from_slice(&select_browser_folder_destinations(request.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            selected,
            serde_json::json!([
                { "id": "root", "path": "Library" },
                { "id": "other", "path": "Other" }
            ])
        );
    }

    #[test]
    fn uses_the_native_blake3_identity() {
        assert_eq!(content_hash(b"abc"), "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85");
    }

    #[test]
    fn browser_subject_projection_accumulates_inferred_bisac_ddc_and_lcc() {
        let input = br#"[
          {"name":"History"},
          {"name":"United States"},
          {"name":"20th Century"},
          {"name":"973","authority":"ddc","code":"973"},
          {"name":"E184","authority":"lcc","code":"E184"}
        ]"#;
        let codes = serde_json::from_slice::<Vec<serde_json::Value>>(&project_subject_codes(input).unwrap()).unwrap();
        assert!(codes.iter().any(|code| code["system"] == "bisac" && code["code"].as_str().is_some_and(|code| code.starts_with("HIS036"))));
        assert!(codes.iter().any(|code| code["system"] == "ddc" && code["code"] == "973"));
        assert!(codes.iter().any(|code| code["system"] == "lcc" && code["code"] == "E184"));
    }

    #[test]
    fn uses_shared_catalog_search_normalization() {
        assert_eq!(catalog_model::normalize_search_text("  García   Márquez "), "garcia marquez");
    }

    #[test]
    fn uses_shared_reader_preference_validation() {
        let defaults = serde_json::to_vec(&app_contract::preferences::ReaderPreferences::default()).unwrap();
        let updated = apply_reader_preference_patch(&defaults, br#"{"preference":"font_size_px","value":27}"#).unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&updated).unwrap();
        assert_eq!(updated["font_size_px"], 27.0);
        assert!(apply_reader_preference_patch(&defaults, br#"{"preference":"max_column_count","value":9}"#).is_err());
    }

    #[test]
    fn uses_shared_browsing_preference_validation() {
        let defaults = serde_json::to_vec(&app_contract::preferences::BrowsingPreferences::default()).unwrap();
        let updated = apply_browsing_preference_patch(&defaults, br#"{"preference":"library_view","value":"detailed"}"#).unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&updated).unwrap();
        assert_eq!(updated["library_view"], "detailed");
        assert!(apply_browsing_preference_patch(&defaults, br#"{"preference":"ui_font_size_px","value":255}"#).is_err());
    }

    #[test]
    fn account_requests_use_the_shared_wire_contract() {
        let encoded = encode_account_request(1, br#"{"email":"reader@example.com","password":"long-enough-password"}"#).unwrap();
        let request: account_contract::LoginRequest = wire::decode(&encoded, 1024).unwrap();
        assert_eq!(request.email, "reader@example.com");
        assert_eq!(request.password, "long-enough-password");
    }

    #[test]
    fn library_catalogue_uses_the_shared_sync_contract() {
        let library_id = sync_common::LibraryId::parse_str("c8e074f1-fd2d-447b-a645-ec94ee75b530").unwrap();
        let encoded = encode_account_request(7, br#"{"library_id":"c8e074f1-fd2d-447b-a645-ec94ee75b530","library_name":"Browser Library"}"#).unwrap();
        let request: sync_common::api::libraries::UpsertLibraryRequest = wire::decode(&encoded, 1024).unwrap();
        assert_eq!(request.library_id, library_id);

        let response = wire::encode(&vec![sync_common::api::libraries::LibrarySummary { library_id, library_name: "Browser Library".to_owned() }]).unwrap();
        let json = decode_account_response(4, &response).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(json[0]["library_name"], "Browser Library");
    }

    #[test]
    fn blob_negotiation_uses_shared_response_validation() {
        use sync_common::api::assets::{BlobManifestResponse, RejectedBlobManifestEntry};
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let response = BlobManifestResponse { owned: vec![hash.clone()], claimed: Vec::new(), upload: Vec::new(), rejected: Vec::<RejectedBlobManifestEntry>::new() };
        let bytes = wire::encode(&response).unwrap();
        let request = format!(r#"{{"blobs":[{{"content_hash":"{hash}","size_bytes":42}}]}}"#);
        assert!(decode_blob_manifest_response(&bytes, request.as_bytes()).is_ok());

        let missing = br#"{"blobs":[{"content_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size_bytes":42}]}"#;
        assert!(decode_blob_manifest_response(&bytes, missing).unwrap_err().contains("unknown or repeated"));
    }

    #[test]
    fn browser_outbox_is_encoded_by_the_shared_replica() {
        let json = br#"{
          "library_id":"c8e074f1-fd2d-447b-a645-ec94ee75b530",
          "replica_id":"12835433-f03d-49ef-bb1a-a3558cc18c5d",
          "cursor":{"state_revision":null,"reading_revision":null},
          "mutations":[{
            "mutation_id":"18d63cc8-aead-4a6f-b239-50ecc4a79777",
            "kind":"reading_position",
            "entity_key":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "entity_subkey":"",
            "value":{"location":"epubcfi(/6/4)","progress":0.25},
            "changed_at":100,"replica_seq":1
          }]
        }"#;
        let encoded = encode_sync_exchange(json).unwrap();
        let request: sync_common::SyncExchangeRequest = wire::decode(&encoded, 4096).unwrap();
        assert_eq!(request.mutations.len(), 1);
        let body = library_replica::MutationBody::from_wire(&request.mutations[0]).unwrap().unwrap();
        assert!(matches!(body, library_replica::MutationBody::ReadingPosition { .. }));
    }
}
