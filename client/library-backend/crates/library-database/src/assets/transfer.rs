use super::TransfersSql;
use crate::books::audiobook::{chapters_from_navigation, track_title_hints};
use crate::placements::{FolderPaths, directory_exists_in, parse_dir_id};
use crate::query_methods::{blob_column, optional_blob_column, query_rows, try_for_each_row};
use crate::shared_sql::SharedSql;
use crate::{Database, DatabaseError, NativeBookSnapshot};
use book_model::{BookFormat, BookTocEntry};
use library_model::BlobKind;
use std::collections::HashMap;
use sync_common::{BookId, ROOT_DIR_ID};

impl Database {
    /// Read one book's live download destinations in a consistent snapshot.
    /// Publication rechecks their identities after filesystem/network work.
    pub fn book_download_snapshot(&self, hash: &BookId) -> Result<Vec<NativeBookSnapshot>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let placements = query_rows(|read| transaction.shared_live_book_placements(hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut folders = FolderPaths::for_directories(&transaction, placements.iter().map(|(id, _)| id.as_str()))?;
        let mut destinations = Vec::new();
        for (directory, name) in placements {
            if let Some(path) = folders.placement(&directory, &name)? {
                destinations.push((directory, name, path));
            }
        }
        if destinations.is_empty() || !directory_exists_in(&transaction, &ROOT_DIR_ID.to_string())? {
            return Ok(Vec::new());
        }
        let mut metadata = None;
        try_for_each_row(
            |read| transaction.transfer_download_metadata(hash, read),
            |row| {
                let format = row.get::<_, Option<i64>>(0)?.and_then(BookFormat::from_code).ok_or_else(|| DatabaseError::message("book has no known format"))?;
                let mut track_titles = Vec::new();
                if format == BookFormat::Mp3Folder
                    && let Some(duration) = row.get::<_, Option<i64>>(1)?.filter(|duration| *duration > 0)
                {
                    let entries: Vec<BookTocEntry> = crate::stored_document::decode(blob_column(row, 2)?)?;
                    let tracks = optional_blob_column(row, 3)?.map(crate::stored_document::decode).transpose()?.unwrap_or_default();
                    let chapters = chapters_from_navigation(&entries, duration as u64).collect::<Vec<_>>();
                    track_titles = track_title_hints(*hash, &chapters, tracks);
                }
                metadata = Some((format, track_titles));
                Ok(())
            },
        )?;
        let Some((format, track_titles)) = metadata else { return Ok(Vec::new()) };
        let mut occupied: HashMap<String, Vec<String>> = HashMap::new();
        transaction.transfer_download_occupied_names(hash, |row| {
            occupied.entry(row.get(0)?).or_default().push(row.get(1)?);
            Ok(())
        })?;
        let books = destinations
            .into_iter()
            .map(|(directory, name, relative_path)| {
                Ok(NativeBookSnapshot {
                    book_id: *hash,
                    directory: parse_dir_id(&directory)?,
                    name,
                    relative_path,
                    occupied_names: occupied.get(&directory).cloned().unwrap_or_default(),
                    format,
                    audiobook_track_titles: track_titles.clone(),
                })
            })
            .collect::<Result<_, DatabaseError>>()?;
        transaction.commit()?;
        Ok(books)
    }

    /// Ends a download request without its bytes.
    pub fn complete_download(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.connection.shared_transfer_complete_download(hash)?;
        Ok(())
    }

    /// The server has no copy to download: forget it and end the request.
    pub fn cancel_download(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            transaction.forget_remote_asset(BlobKind::Book.storage(), hash)?;
            transaction.shared_transfer_complete_download(hash)?;
            Ok(())
        })
    }

    /// Downloaded bytes are stored here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn complete_local_book_request(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| Self::complete_local_book_request_on(transaction, hash))
    }

    /// Covers the server holds.
    pub fn record_server_covers(&self, hashes: &[BookId]) -> Result<(), DatabaseError> {
        self.record_cover_presence(hashes, &[])
    }

    /// Present and missing hashes from one validated response, written atomically.
    pub fn record_cover_presence(&self, present: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        let present = serde_json::to_string(present).map_err(DatabaseError::operation)?;
        let missing = serde_json::to_string(missing).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            // One statement per set keeps SQL preparation and Rust/SQLite calls
            // independent of response size. Unchanged facts produce no writes.
            transaction.record_cover_presence_present(present.as_str())?;
            transaction.record_cover_presence_missing(missing.as_str())?;
            Ok(())
        })
    }

    /// One cover download batch: covers now ready here, the ones among them
    /// that came from the server, and covers the server does not have.
    pub fn complete_cover_downloads(&self, ready: &[BookId], received: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        let ready = serde_json::to_string(ready).map_err(DatabaseError::operation)?;
        let received = serde_json::to_string(received).map_err(DatabaseError::operation)?;
        let missing = serde_json::to_string(missing).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            transaction.record_cover_presence_present(received.as_str())?;
            transaction.complete_cover_downloads_ready(ready.as_str())?;
            transaction.record_cover_presence_missing(missing.as_str())?;
            Ok(())
        })
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn book_download_snapshot(hash: &BookId) -> Vec<NativeBookSnapshot>;
    arguments (BookId,) = (*hash,);
    execute database, arguments => database.book_download_snapshot(&arguments.0);

    fn complete_download(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.complete_download(&arguments.0);

    fn cancel_download(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.cancel_download(&arguments.0);

    #[cfg(not(target_arch = "wasm32"))]
    fn complete_local_book_request(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.complete_local_book_request(&arguments.0);

    fn record_server_covers(hashes: &[BookId]) -> ();
    arguments (Vec<BookId>,) = (hashes.to_vec(),);
    execute database, arguments => database.record_server_covers(&arguments.0);

    fn record_cover_presence(present: &[BookId], missing: &[BookId]) -> ();
    arguments (Vec<BookId>, Vec<BookId>,) = (present.to_vec(), missing.to_vec(),);
    execute database, arguments => database.record_cover_presence(&arguments.0, &arguments.1);

    fn complete_cover_downloads(ready: &[BookId], received: &[BookId], missing: &[BookId]) -> ();
    arguments (Vec<BookId>, Vec<BookId>, Vec<BookId>,) = (ready.to_vec(), received.to_vec(), missing.to_vec(),);
    execute database, arguments => database.complete_cover_downloads(&arguments.0, &arguments.1, &arguments.2);
}
