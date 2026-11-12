//! Decode the last published web storage layouts before writing current values.
//! Version 17's enrichment and state enum differ from version 18; changing only
//! the envelope version would misinterpret those values.
use crate::DatabaseError;
use book_model::{Annotation, AudiobookTrack, BookRecord, BookTocDocument, BookTocEntry, ChapterReadStatus};
use library_replica::{BookAddition, BookFacts, BookLifecycleState, DirectoryLifecycleState, EnrichmentResult, LibraryStateEntry, ReadingPositionState};
use metadata_contract::audible::LookupRequest;
use pdf_view_common::PdfReaderMetadata;
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sync_common::{BookId, DirId, FileName, MutationIssue, VersionKey, wire};

#[derive(Deserialize)]
struct RetiredScore {
    _work: String,
    _count: u64,
    _date: String,
}

#[derive(Deserialize)]
enum LegacyEnrichment {
    Found { record: BookRecord, toc: Option<Vec<BookTocEntry>>, cover_id: Option<String> },
    NoMatch,
    FoundWithPopularity { record: BookRecord, toc: Option<Vec<BookTocEntry>>, cover_id: Option<String>, _popularity: RetiredScore },
    Applied { toc: Option<Vec<BookTocEntry>>, cover_id: Option<String>, _popularity: Option<RetiredScore> },
}

#[derive(Deserialize)]
struct LegacyAddition {
    facts: BookFacts,
    #[serde(with = "book_model::record_transport")]
    record: BookRecord,
    navigation: Option<BookTocDocument>,
    pdf: Option<PdfReaderMetadata>,
    recording: Option<LookupRequest>,
    enrichment: Option<LegacyEnrichment>,
}

impl LegacyAddition {
    fn upgrade(self) -> BookAddition {
        let mut addition = BookAddition { facts: self.facts, record: self.record, navigation: self.navigation, pdf: self.pdf, recording: self.recording, enrichment: None };
        let Some(enrichment) = self.enrichment else { return addition };
        match enrichment {
            LegacyEnrichment::Found { record, toc, cover_id } => addition.apply_enrichment(record, EnrichmentResult::Found { toc, cover_id }),
            LegacyEnrichment::FoundWithPopularity { record, toc, cover_id, .. } => addition.apply_enrichment(record, EnrichmentResult::Found { toc, cover_id }),
            LegacyEnrichment::Applied { toc, cover_id, .. } => addition.enrichment = Some(EnrichmentResult::Found { toc, cover_id }),
            LegacyEnrichment::NoMatch => addition.enrichment = Some(EnrichmentResult::NoMatch),
        }
        addition
    }
}

// Preserve the original variant order. The removed external-provider variant
// (9) must never be decoded as the current BookContent variant (also 9).
#[derive(Deserialize)]
enum LegacyState {
    DirectoryName { dir_id: DirId, name: FileName },
    DirectoryParent { dir_id: DirId, parent_id: DirId },
    DirectoryLifecycle { dir_id: DirId, value: DirectoryLifecycleState },
    BookLifecycle { content_hash: BookId, value: BookLifecycleState },
    Placement { dir_id: DirId, content_hash: BookId, present: bool, origin_folder_id: Option<String> },
    ReadingPosition { content_hash: BookId, value: ReadingPositionState },
    Annotation { value: Annotation },
    BookAdd { content_hash: BookId, value: LegacyAddition },
    ChapterReadStatus { content_hash: BookId, value: ChapterReadStatus },
}

impl LegacyState {
    fn upgrade(self) -> LibraryStateEntry {
        match self {
            Self::DirectoryName { dir_id, name } => LibraryStateEntry::DirectoryName { dir_id, name },
            Self::DirectoryParent { dir_id, parent_id } => LibraryStateEntry::DirectoryParent { dir_id, parent_id },
            Self::DirectoryLifecycle { dir_id, value } => LibraryStateEntry::DirectoryLifecycle { dir_id, value },
            Self::BookLifecycle { content_hash, value } => LibraryStateEntry::BookLifecycle { book_id: content_hash, value },
            Self::Placement { dir_id, content_hash, present, origin_folder_id } => LibraryStateEntry::Placement { dir_id, book_id: content_hash, present, origin_folder_id },
            Self::ReadingPosition { content_hash, value } => LibraryStateEntry::ReadingPosition { book_id: content_hash, value },
            Self::Annotation { value } => LibraryStateEntry::Annotation { value },
            Self::BookAdd { content_hash, value } => LibraryStateEntry::BookAdd { book_id: content_hash, value: value.upgrade() },
            Self::ChapterReadStatus { content_hash, value } => LibraryStateEntry::ChapterReadStatus { book_id: content_hash, value },
        }
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DatabaseError> {
    if !bytes.starts_with(b"BKHM\x11") {
        return Err(DatabaseError::message("expected version 17 storage during upgrade"));
    }
    // Reuse the bounded, full-consumption decoder with the explicitly selected
    // historical type, rather than permitting old versions in ordinary reads.
    let mut body = bytes.to_vec();
    body[4] = 18;
    wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).map_err(DatabaseError::operation)
}

fn rewrite(tx: &Transaction<'_>, table: &str, column: &str, convert: impl Fn(&[u8]) -> Result<Vec<u8>, DatabaseError>) -> Result<(), DatabaseError> {
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| row.get(0))?;
    if !exists {
        return Ok(());
    }
    let sql = format!("SELECT rowid,{column} FROM {table} WHERE substr({column},1,5)=X'424B484D11'");
    let mut read = tx.prepare(&sql)?;
    let mut rows = read.query([])?;
    let mut write = tx.prepare(&format!("UPDATE {table} SET {column}=?2 WHERE rowid=?1"))?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let body: Vec<u8> = row.get(1)?;
        write.execute(params![id, convert(&body)?])?;
    }
    Ok(())
}

fn unchanged<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<Vec<u8>, DatabaseError> {
    wire::encode(&decode::<T>(bytes)?).map_err(DatabaseError::operation)
}

pub(crate) fn upgrade(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    for table in ["sync_state_version", "sync_book_record", "sync_outbox", "local_book_draft"] {
        rewrite(tx, table, "body", |bytes| wire::encode(&decode::<LegacyState>(bytes)?.upgrade()).map_err(DatabaseError::operation))?;
    }
    rewrite(tx, "sync_book_record", "publication", unchanged::<VersionKey>)?;
    rewrite(tx, "sync_outbox", "winning_version", unchanged::<VersionKey>)?;
    rewrite(tx, "sync_mutation_issue", "issue", unchanged::<MutationIssue>)?;
    rewrite(tx, "annotation", "body", unchanged::<Annotation>)?;
    rewrite(tx, "notebook_annotation", "body", unchanged::<Annotation>)?;
    rewrite(tx, "book", "toc_json", unchanged::<Vec<BookTocEntry>>)?;
    rewrite(tx, "book", "tracks_json", unchanged::<Vec<AudiobookTrack>>)?;
    rewrite(tx, "pdf_reader_metadata", "metadata", unchanged::<PdfReaderMetadata>)?;
    Ok(())
}
