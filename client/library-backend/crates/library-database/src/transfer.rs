//! Snapshots and reversible source removal for transfers to another library.
use crate::sync::apply::registers::canonical_body;
use crate::{Database, DatabaseError};
use book_model::Annotation;
use library_model::LibraryFolderDestination;
use library_replica::{BookAddition, LibraryStateEntry};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use sync_common::{BookId, DirId, ROOT_DIR_ID, StateKey};

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
#[derive(Clone, Debug)]
pub struct BookTransferState {
    pub content_hash: BookId,
    pub addition: BookAddition,
    pub reading: Option<LibraryStateEntry>,
    pub annotations: Vec<Annotation>,
}
impl Database {
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
            if !nested && !roots.contains(id) {
                roots.push(*id);
            }
        }
        let mut scope: HashSet<_> = roots.iter().copied().collect();
        let mut folders = Vec::new();
        for id in &roots {
            folders.push((*by_id[id]).clone());
        }
        let mut index = 0;
        while index < folders.len() {
            let parent = folders[index].id;
            for folder in &all {
                if folder.parent_id == Some(parent) && scope.insert(folder.id) {
                    folders.push(folder.clone());
                }
            }
            index += 1;
        }
        let mut statement = self.connection.prepare_cached("SELECT b.content_hash, bd.dir_id, bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir d ON d.id=bd.dir_id WHERE bd.deleted_at IS NULL AND b.deleted_at IS NULL AND d.deleted_at IS NULL AND (bd.dir_id IN (SELECT value FROM json_each(?1)) OR b.content_hash IN (SELECT unhex(value) FROM json_each(?2))) ORDER BY bd.dir_id,b.content_hash")?;
        let scope_json = serde_json::to_string(&scope).map_err(DatabaseError::operation)?;
        let books_json = serde_json::to_string(&selection.books).map_err(DatabaseError::operation)?;
        let placements = statement
            .query_map(rusqlite::params![scope_json, books_json], |row| {
                let id: String = row.get(1)?;
                let directory = id.parse().map_err(|error| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error)))?;
                Ok(TransferBook { content_hash: row.get(0)?, directory, file_name: row.get(2)?, direct: false })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut books: Vec<_> = placements.iter().filter(|book| scope.contains(&book.directory)).cloned().collect();
        for hash in &selection.books {
            let placement = placements.iter().find(|book| book.content_hash == *hash && selection.source.is_none_or(|source| source == book.directory)).ok_or_else(|| DatabaseError::message("source book placement is unavailable"))?;
            if !scope.contains(&placement.directory) && !books.iter().any(|book| book.direct && book.content_hash == *hash) {
                books.push(TransferBook { direct: true, ..placement.clone() });
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
        self.with_browse_snapshot(|| {
            let bytes = match canonical_body(&self.connection, "book_add", &StateKey::Book(hash), "")? {
                Some((body, _)) => body,
                None => {
                    self.connection.query_row("SELECT body FROM local_book_draft WHERE content_hash=?1", [hash], |row| row.get::<_, Vec<u8>>(0)).optional()?.ok_or_else(|| DatabaseError::message("source book metadata is unavailable"))?
                }
            };
            let LibraryStateEntry::BookAdd { content_hash, value: addition } = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                return Err(DatabaseError::message("source book metadata is invalid"));
            };
            if hash != content_hash {
                return Err(DatabaseError::message("source book identity is invalid"));
            }
            let reading = canonical_body(&self.connection, "reading_position", &StateKey::Book(hash), "")?
                .map(|(bytes, _)| sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation))
                .transpose()?;
            Ok(BookTransferState { content_hash: hash, addition, reading, annotations: self.annotations(hash)? })
        })
    }
}
