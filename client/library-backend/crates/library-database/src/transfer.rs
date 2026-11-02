//! Snapshots and reversible source removal for transfers to another library.
use crate::sync::apply::registers::canonical_body;
use crate::{Database, DatabaseError};
use book_inspection::{CachedPdfReader, InspectedBook};
use book_model::{Annotation, BookFormat};
use content_address::ContentHash;
use library_model::LibraryFolderDestination;
use library_replica::{BookAddition, LibraryStateEntry};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use sync_common::{BookId, DirId, ROOT_DIR_ID, StateKey};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSelection {
    pub folders: Vec<DirId>,
    pub books: Vec<BookId>,
    pub source: Option<DirId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferBook {
    pub content_hash: BookId,
    pub directory: DirId,
    pub file_name: String,
    /// Direct selections go into the destination itself; tree placements keep their parent.
    pub direct: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferPlan {
    pub selection: TransferSelection,
    pub roots: Vec<DirId>,
    /// Parents precede their children, including empty folders.
    pub folders: Vec<LibraryFolderDestination>,
    pub books: Vec<TransferBook>,
}
/// Device-local reader adapter context, used while importing reader state.
/// This is never part of an annotation, book record, or synchronized state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReaderAnnotationSource {
    Foliate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTransferState {
    pub content_hash: BookId,
    #[serde(default)]
    pub byte_checksum: Option<ContentHash>,
    pub addition: BookAddition,
    pub reading: Option<LibraryStateEntry>,
    #[serde(default)]
    pub chapter_read: Option<LibraryStateEntry>,
    pub annotations: Vec<Annotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation_source: Option<ReaderAnnotationSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedBook {
    pub content_hash: BookId,
    pub created: bool,
}

impl BookTransferState {
    pub fn expected_checksum(&self) -> Option<ContentHash> {
        self.byte_checksum.or_else(|| self.content_hash.content_hash())
    }

    /// Only call after verifying complete bytes. Reading state follows those
    /// bytes when the destination assigns a different storage identity.
    pub fn retarget(mut self, destination: BookId) -> Result<Self, DatabaseError> {
        let original = self.content_hash;
        self.byte_checksum = self.expected_checksum();
        if destination.content_hash().zip(self.byte_checksum).is_some_and(|(target, original)| target != original) {
            return Err(DatabaseError::message("destination bytes differ from the transferred source"));
        }
        if let Some(pdf) = &mut self.addition.pdf {
            if !pdf.valid_for(&original.to_hex()) {
                return Err(DatabaseError::message("transferred PDF metadata does not match the source"));
            }
            pdf.content_hash = destination.to_hex();
        }
        if let Some(tracks) = self.addition.navigation.as_mut().and_then(|navigation| navigation.tracks.as_mut()) {
            for track in tracks {
                if let Some(id) = &mut track.archive_hash {
                    if *id != original {
                        return Err(DatabaseError::message("transferred audio index does not match the source"));
                    }
                    *id = destination;
                }
            }
        }
        for (entry, kind) in [(&mut self.reading, "reading_position"), (&mut self.chapter_read, "chapter_read_status")] {
            if let Some(entry) = entry {
                if entry.book() != Some(original) || entry.kind() != kind {
                    return Err(DatabaseError::message("transferred reading state does not match the source"));
                }
                match entry {
                    LibraryStateEntry::ReadingPosition { content_hash, .. } | LibraryStateEntry::ChapterReadStatus { content_hash, .. } => *content_hash = destination,
                    _ => return Err(DatabaseError::message("invalid transferred reading state")),
                }
            }
        }
        for annotation in &mut self.annotations {
            if annotation.content_hash != original {
                return Err(DatabaseError::message("transferred annotation does not match the source"));
            }
            annotation.content_hash = destination;
        }
        self.content_hash = destination;
        Ok(self)
    }
}
impl Database {
    /// Active contents only; copying storage never mutates this source.
    pub fn storage_copy_plan(&self) -> Result<TransferPlan, DatabaseError> {
        self.with_browse_snapshot(|| {
            let folders = self.folder_destinations()?.into_iter().filter(|folder| folder.parent_id == Some(ROOT_DIR_ID)).map(|folder| folder.id).collect();
            let mut query = self.connection.prepare("SELECT b.content_hash FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND bd.deleted_at IS NULL AND b.deleted_at IS NULL ORDER BY b.content_hash")?;
            let books = query.query_map([ROOT_DIR_ID.to_string()], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
            let selection = TransferSelection { folders, books, source: Some(ROOT_DIR_ID) };
            if selection.folders.is_empty() && selection.books.is_empty() {
                return Ok(TransferPlan { selection, roots: Vec::new(), folders: Vec::new(), books: Vec::new() });
            }
            self.transfer_plan_inner(selection)
        })
    }

    /// Retain an existing destination placement's name when the same hash is imported again.
    pub fn transfer_destination_name(&self, hash: &BookId, directory: &DirId) -> Result<Option<String>, DatabaseError> {
        Ok(self
            .connection
            .query_row(
                "SELECT bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND b.content_hash=?2 AND bd.deleted_at IS NULL AND b.deleted_at IS NULL",
                rusqlite::params![directory.to_string(), hash],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn transfer_plan(&self, selection: TransferSelection) -> Result<TransferPlan, DatabaseError> {
        self.with_browse_snapshot(|| self.transfer_plan_inner(selection))
    }
    fn transfer_plan_inner(&self, selection: TransferSelection) -> Result<TransferPlan, DatabaseError> {
        if selection.folders.contains(&ROOT_DIR_ID) || (selection.folders.is_empty() && selection.books.is_empty()) {
            return Err(DatabaseError::message("select books or folders to transfer"));
        }
        let all = self.folder_destinations()?;
        let by_id: HashMap<_, _> = all.iter().map(|folder| (folder.id, folder)).collect();
        let selected: HashSet<_> = selection.folders.iter().copied().collect();
        let mut roots = Vec::new();
        let mut scope = HashSet::new();
        for id in &selection.folders {
            let folder = by_id.get(id).ok_or_else(|| DatabaseError::message("source folder is unavailable"))?;
            let mut parent = folder.parent_id;
            let mut nested = false;
            let mut visited = HashSet::new();
            while let Some(id) = parent {
                if !visited.insert(id) {
                    return Err(DatabaseError::message("invalid source folder hierarchy"));
                }
                if selected.contains(&id) {
                    nested = true;
                    break;
                }
                parent = by_id.get(&id).and_then(|folder| folder.parent_id);
            }
            if !nested && scope.insert(*id) {
                roots.push(*id);
            }
        }
        let mut folders: Vec<_> = roots.iter().map(|id| (*by_id[id]).clone()).collect();
        if !folders.is_empty() {
            // Keep sibling order from the destination list without rescanning
            // the whole library for each folder reached by the breadth-first walk.
            let mut children = HashMap::<DirId, Vec<&LibraryFolderDestination>>::new();
            for folder in &all {
                if let Some(parent) = folder.parent_id {
                    children.entry(parent).or_default().push(folder);
                }
            }
            let mut index = 0;
            while index < folders.len() {
                for &folder in children.get(&folders[index].id).into_iter().flatten() {
                    if scope.insert(folder.id) {
                        folders.push(folder.clone());
                    }
                }
                index += 1;
            }
        }
        let mut statement = self.connection.prepare_cached("SELECT b.content_hash, bd.dir_id, bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir d ON d.id=bd.dir_id WHERE bd.deleted_at IS NULL AND b.deleted_at IS NULL AND d.deleted_at IS NULL AND (bd.dir_id IN (SELECT value FROM json_each(?1)) OR b.content_hash IN (SELECT unhex(value) FROM json_each(?2))) ORDER BY bd.dir_id,b.content_hash")?;
        let scope_json = serde_json::to_string(&scope).map_err(DatabaseError::operation)?;
        let books_json = serde_json::to_string(&selection.books).map_err(DatabaseError::operation)?;
        enum SelectedPlacement {
            Missing,
            Included,
            Direct(TransferBook),
        }
        let mut selected_books: HashMap<_, _> = selection.books.iter().map(|hash| (*hash, SelectedPlacement::Missing)).collect();
        let mut books = Vec::new();
        let mut rows = statement.query(rusqlite::params![scope_json, books_json])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(1)?;
            let directory = id.parse().map_err(|error| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error)))?;
            let mut placement = TransferBook { content_hash: row.get(0)?, directory, file_name: row.get(2)?, direct: false };
            let in_tree = scope.contains(&placement.directory);
            // SQL orders source placements. Keep the first matching one even
            // when it is already covered by a selected folder.
            if selection.source.is_none_or(|source| source == placement.directory)
                && let Some(selected) = selected_books.get_mut(&placement.content_hash)
                && matches!(selected, SelectedPlacement::Missing)
            {
                if !in_tree {
                    placement.direct = true;
                    *selected = SelectedPlacement::Direct(placement);
                    continue;
                }
                *selected = SelectedPlacement::Included;
            }
            if in_tree {
                books.push(placement);
            }
        }
        for hash in &selection.books {
            let selected = selected_books.get_mut(hash).expect("selected book");
            match std::mem::replace(selected, SelectedPlacement::Included) {
                SelectedPlacement::Missing => return Err(DatabaseError::message("source book placement is unavailable")),
                SelectedPlacement::Included => {}
                SelectedPlacement::Direct(placement) => books.push(placement),
            }
        }
        Ok(TransferPlan { selection, roots, folders, books })
    }
    /// Revalidate and remove the entire source selection in one transaction.
    /// Copying a stale snapshot never discards changes made during the transfer.
    pub fn remove_transferred_selection(&self, plan: &TransferPlan) -> Result<(), DatabaseError> {
        self.write(|tx| {
            if self.transfer_plan_inner(plan.selection.clone())? != *plan {
                return Err(DatabaseError::message("source changed while copying; copied items were kept and the source was left unchanged"));
            }
            for root in &plan.roots {
                crate::placements::trash_directory_in(tx, root)?;
                crate::projection::project_dirty(tx)?;
            }
            for book in plan.books.iter().filter(|book| book.direct) {
                crate::placements::remove_book_placement_in(tx, &book.content_hash, &book.directory)?;
                crate::projection::project_dirty(tx)?;
            }
            Ok(())
        })
    }
    pub fn book_transfer_state(&self, hash: BookId) -> Result<BookTransferState, DatabaseError> {
        self.with_browse_snapshot(|| self.book_transfer_state_in_snapshot(hash))
    }

    /// Reuse reader geometry/navigation only for the exact previously inspected bytes.
    pub fn cached_pdf_reader(&self, hash: BookId) -> Result<Option<CachedPdfReader>, DatabaseError> {
        let Some(checksum) = hash.content_hash() else { return Ok(None) };
        let identity = self.book_for_local_content(checksum)?.unwrap_or(hash);
        if let Some(canonical) = self.canonical_content_hash(identity)? {
            if canonical != checksum {
                return Ok(None);
            }
        } else if identity != hash {
            let revisions: i64 = self.connection.query_row("SELECT count(*) FROM local_content_alias WHERE book_id=?1", [identity], |row| row.get(0))?;
            if revisions != 1 {
                return Ok(None);
            }
        }
        let Some(mut metadata) = self.pdf_metadata(&identity)? else { return Ok(None) };
        metadata.content_hash = hash.to_hex();
        metadata.skippable_pages = None;
        metadata.annotations.clear();
        metadata.dependencies = None;
        let bytes = self.connection.query_row("SELECT CAST(toc_json AS BLOB) FROM book WHERE content_hash=?1", [identity], |row| row.get::<_, Option<Vec<u8>>>(0)).optional()?.flatten();
        let toc = bytes.map(|bytes| crate::stored_document::decode(&bytes)).transpose()?.unwrap_or_default();
        Ok(Some(CachedPdfReader { metadata, toc }))
    }

    /// Read only immutable EPUB/PDF facts for an unchanged, current inspection.
    /// Reimport does not need to decode all existing notes or reading history.
    pub fn cached_import_inspection(&self, hash: BookId, format: BookFormat, version: i64) -> Result<Option<InspectedBook>, DatabaseError> {
        self.with_browse_snapshot(|| {
            if !matches!(format, BookFormat::Epub | BookFormat::Pdf) || !self.has_import_inspection(&hash, format, version)? {
                return Ok(None);
            }
            let (_, addition) = self.book_transfer_addition_in_snapshot(hash)?;
            Ok(Some(InspectedBook { metadata: addition.record, pdf: addition.pdf, audiobook: None, toc: addition.navigation.map(|navigation| navigation.entries).unwrap_or_default() }))
        })
    }

    fn book_transfer_addition_in_snapshot(&self, hash: BookId) -> Result<(BookId, BookAddition), DatabaseError> {
        let kind = "book_add";
        let bytes = match canonical_body(&self.connection, kind, &StateKey::Book(hash), "")? {
            Some((body, _)) => body,
            None => self.connection.query_row("SELECT body FROM local_book_draft WHERE content_hash=?1", [hash], |row| row.get::<_, Vec<u8>>(0)).optional()?.ok_or_else(|| DatabaseError::message("source book metadata is unavailable"))?,
        };
        let (content_hash, addition) = match sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
            LibraryStateEntry::BookAdd { content_hash, value } => (content_hash, value),
            _ => return Err(DatabaseError::message("source book metadata is invalid")),
        };
        if hash != content_hash {
            return Err(DatabaseError::message("source book identity is invalid"));
        }
        Ok((content_hash, addition))
    }

    pub(crate) fn book_transfer_state_in_snapshot(&self, hash: BookId) -> Result<BookTransferState, DatabaseError> {
        let (_, addition) = self.book_transfer_addition_in_snapshot(hash)?;
        let reading =
            canonical_body(&self.connection, "reading_position", &StateKey::Book(hash), "")?.map(|(bytes, _)| sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)).transpose()?;
        let chapter_read = canonical_body(&self.connection, "chapter_read_status", &StateKey::Book(hash), "")?
            .map(|(bytes, _)| sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation))
            .transpose()?;
        let byte_checksum = self.connection.query_row("SELECT f.byte_hash FROM local_file_content f JOIN book b ON b.content_hash=f.book_id JOIN book_dir d ON d.book_row_id=b.row_id AND d.dir_id=f.dir_id WHERE f.book_id=?1 AND d.deleted_at IS NULL AND d.is_downloaded=1 ORDER BY f.dir_id LIMIT 1", [hash], |row| row.get::<_,ContentHash>(0)).optional()?
            .or(crate::content_identity::canonical_hash(&self.connection, hash)?)
            .or_else(|| hash.content_hash());
        let state = BookTransferState { content_hash: hash, byte_checksum, addition, reading, chapter_read, annotations: self.annotations(hash)?, annotation_source: None };
        Ok(state)
    }

    pub fn merge_transferred_reader_state(&self, destination: BookId, source: BookTransferState) -> Result<(), DatabaseError> {
        let mut source = source.retarget(destination)?;
        self.write(|tx| {
            reconcile_reader_annotations(tx, &mut source)?;
            merge_transfer_progress(tx, destination, source.reading, source.chapter_read, source.annotations)
        })
    }
}

/// Reconcile only explicit Foliate adapter imports, inside the same transaction
/// as their placement/reader state. General transfers preserve existing edits.
pub(crate) fn reconcile_reader_annotations(tx: &rusqlite::Transaction<'_>, source: &mut BookTransferState) -> Result<(), DatabaseError> {
    use crate::sync::apply::registers::record_local;
    if source.annotation_source != Some(ReaderAnnotationSource::Foliate) || source.annotations.is_empty() {
        return Ok(());
    }
    let mut query = tx.prepare("SELECT a.body FROM annotation a JOIN book b ON b.row_id=a.book_row_id WHERE b.content_hash=?1 ORDER BY a.id")?;
    let bodies = query.query_map([source.content_hash], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
    let history = bodies.iter().map(|body| Annotation::decode(body).map_err(DatabaseError::operation)).collect::<Result<Vec<_>, _>>()?;
    let matched = library_model::match_foliate_annotations(&source.annotations, &history).map_err(DatabaseError::operation)?;
    for (annotation, matched) in source.annotations.iter_mut().zip(matched) {
        let Some(index) = matched else {
            continue;
        };
        let current = &history[index];
        annotation.id = current.id.clone();
        // A re-import never restores a tombstone or overwrites a local note,
        // style, or color edit. Only a newer external location can advance.
        if !current.deleted && annotation.modified_at > current.modified_at && annotation.anchor.epub_cfi_value() != current.anchor.epub_cfi_value() {
            let mut moved = current.clone();
            moved.anchor = current.anchor.with_origin(annotation.anchor.origin().clone());
            moved.exact_text = annotation.exact_text.clone();
            moved.toc_ordinal = annotation.toc_ordinal;
            moved.progress = annotation.progress;
            moved.modified_at = annotation.modified_at;
            record_local(tx, LibraryStateEntry::Annotation { value: moved })?;
        }
    }
    Ok(())
}

pub(crate) fn merge_transfer_progress(tx: &rusqlite::Transaction<'_>, book: BookId, reading: Option<LibraryStateEntry>, chapter_read: Option<LibraryStateEntry>, annotations: Vec<Annotation>) -> Result<(), DatabaseError> {
    use crate::sync::apply::registers::record_local;
    for (entry, kind) in [(reading, "reading_position"), (chapter_read, "chapter_read_status")] {
        if let Some(entry) = entry {
            if entry.book() != Some(book) || entry.kind() != kind {
                return Err(DatabaseError::message("invalid transferred reading state"));
            }
            if canonical_body(tx, kind, &StateKey::Book(book), "")?.is_none() {
                record_local(tx, entry)?;
            }
        }
    }
    for mut annotation in annotations {
        if annotation.content_hash != book {
            return Err(DatabaseError::message("invalid transferred annotation"));
        }
        let original_id = annotation.id.clone();
        for attempt in 0_u64.. {
            match canonical_body(tx, "annotation", &StateKey::Text(annotation.id.clone()), "")? {
                None => {
                    record_local(tx, LibraryStateEntry::Annotation { value: annotation })?;
                    break;
                }
                Some((body, _)) => {
                    let existing: LibraryStateEntry = sync_common::wire::decode(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
                    if existing.book() == Some(book) {
                        if let LibraryStateEntry::Annotation { mut value } = existing {
                            // Re-import can enrich notes created before TOC
                            // matching existed, preserving edits and deletions.
                            if !value.deleted && value.toc_ordinal.is_none() && annotation.toc_ordinal.is_some() && value.anchor == annotation.anchor {
                                value.toc_ordinal = annotation.toc_ordinal;
                                record_local(tx, LibraryStateEntry::Annotation { value })?;
                            }
                        }
                        break;
                    }
                    // Annotation keys belong to the library, while provider
                    // copies receive new book identities. Keep the original
                    // note and give this copy a stable key so retries also
                    // preserve edits or deletion of the destination note.
                    annotation.id = Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("bokheim/transfer-annotation/{book}/{original_id}/{attempt}").as_bytes()).to_string();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::IngestionSql;
    use book_model::{AnnotationAnchor, AnnotationStyle};

    #[test]
    fn copying_a_note_to_another_identity_preserves_both_notes_and_retries_do_not_restore_deleted_copies() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let original = BookId::from(blake3::hash(b"original"));
        let destination = BookId::from(blake3::hash(b"copy"));
        db.write(|tx| {
            tx.ingestion_ensure_book(&original)?;
            tx.ingestion_ensure_book(&destination)?;
            Ok(())
        })
        .unwrap();
        let note = Annotation {
            id: "shared-note".into(),
            content_hash: original,
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
            exact_text: "Text".into(),
            style: AnnotationStyle::Underline,
            color: 0xffffff,
            note: "Original note".into(),
            created_at: 7,
            toc_ordinal: None,
            progress: Some(42.0),
            modified_at: 9,
            deleted: false,
        };
        db.upsert_annotation(&note).unwrap();
        let mut copy = note.clone();
        copy.content_hash = destination;
        db.write(|tx| merge_transfer_progress(tx, destination, None, None, vec![copy.clone()])).unwrap();
        assert_eq!(db.annotations(original).unwrap(), vec![note.clone()]);
        let mut copied = db.annotations(destination).unwrap().pop().unwrap();
        assert_ne!(copied.id, note.id);
        let mut expected = copy.clone();
        expected.id = copied.id.clone();
        assert_eq!(copied, expected);

        copied.note = "Edited copy".into();
        copied.modified_at = 10;
        db.upsert_annotation(&copied).unwrap();
        db.write(|tx| merge_transfer_progress(tx, destination, None, None, vec![copy.clone()])).unwrap();
        assert_eq!(db.annotations(destination).unwrap(), vec![copied.clone()]);
        db.delete_annotation(&copied.id, 11).unwrap();
        db.write(|tx| merge_transfer_progress(tx, destination, None, None, vec![copy.clone()])).unwrap();
        // The canonical tombstone still owns the generated key.
        assert!(db.annotations(destination).unwrap().is_empty());
        assert_eq!(db.annotations(original).unwrap(), vec![note]);
    }
}
