use crate::assets::AssetsSql;
use crate::storage_values::invalid_text_column;
use crate::storage_values::unix_secs;
use library_replica::BlobKind;
use library_replica::TransferOrigin;
use rusqlite::Connection;
use sync_common::ContentHash;

pub fn get_asset_requests(conn: &Connection, kind: BlobKind) -> Result<Vec<(ContentHash, TransferOrigin)>, rusqlite::Error> {
    let mut requests = Vec::new();
    conn.get_download_requests(kind.storage(), |row| {
        let raw_key: String = row.get(0)?;
        let raw_origin: String = row.get(1)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(1, "invalid transfer origin"))?;
        requests.push((ContentHash::new(&raw_key), origin));
        Ok(())
    })?;
    Ok(requests)
}

pub fn add_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    conn.add_download_request(kind.storage(), hash.as_str(), origin.storage(), unix_secs()?)?;
    Ok(())
}

pub fn request_book_downloads(conn: &mut Connection, content_hashes: &[ContentHash], origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    let transaction = conn.transaction()?;
    for content_hash in content_hashes {
        let has_placement: bool = transaction.query_row(include_str!("sql/requests/request_book_downloads_select.sql"), [content_hash.as_str()], |row| row.get(0))?;
        if !has_placement {
            return Err(invalid_text_column(0, format!("book {content_hash} has no live placement")));
        }
        add_asset_request(&transaction, BlobKind::Book, content_hash, origin)?;
    }
    transaction.commit()
}

pub fn complete_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.complete_download_request(kind.storage(), hash.as_str())?;
    Ok(())
}
