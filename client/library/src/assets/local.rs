use crate::assets::{AssetsSql, BookAccessSql};
use crate::books::BooksSql;
use library_replica::{BookPlacement, RelativeBookPath, RelativeDirPath};
use crate::placements::PlacementsSql;
use crate::storage_values::invalid_text_column;
use crate::storage_values::parse_uuid_column;
use book_model::BookFormat;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use std::collections::HashMap;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::FileName;
pub fn get_book_file_path(conn: &Connection, content_hash: ContentHash) -> Result<Option<RelativeBookPath>, rusqlite::Error> {
    let mut file_path: Option<String> = None;
    conn.get_book_file_path(content_hash.as_str(), |row| {
        file_path = Some(row.get(0)?);
        Ok(())
    })?;
    file_path.map(|path| RelativeBookPath::parse(&path).map_err(|error| invalid_text_column(0, error.to_string()))).transpose()
}
pub fn get_book_format(conn: &Connection, content_hash: &ContentHash) -> Result<Option<BookFormat>, rusqlite::Error> {
    conn.query_row(include_str!("sql/local/get_book_format_select.sql"), [content_hash.as_str()], |row| {
        let raw: String = row.get(0)?;
        BookFormat::from_extension(&raw).ok_or_else(|| invalid_text_column(0, format!("book {content_hash} has no known format")))
    })
    .optional()
}
pub fn is_book_downloaded(conn: &Connection, content_hash: ContentHash) -> Result<bool, rusqlite::Error> {
    let mut downloaded = false;
    conn.is_book_downloaded(content_hash.as_str(), |row| {
        downloaded = row.get::<_, bool>(0)?;
        Ok(())
    })?;
    Ok(downloaded)
}
pub fn is_book_download_requested(conn: &Connection, content_hash: ContentHash) -> Result<bool, rusqlite::Error> {
    let mut requested = false;
    conn.is_book_download_requested(content_hash.as_str(), |row| {
        requested = row.get::<_, bool>(0)?;
        Ok(())
    })?;
    Ok(requested)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPathComponent {
    pub id: DirId,
    pub name: FileName,
}
pub fn get_dir_path_components(conn: &Connection, dir_id: &str) -> Result<Vec<DirPathComponent>, rusqlite::Error> {
    let mut components = Vec::new();
    conn.get_dir_path_components(dir_id, |row| {
        let id = parse_uuid_column(row.get(0)?, 0)?;
        components.push(DirPathComponent { id, name: row.get(1)? });
        Ok(())
    })?;
    Ok(components)
}
pub fn get_live_dir_paths_tx(tx: &Transaction) -> Result<HashMap<DirId, RelativeDirPath>, rusqlite::Error> {
    let mut paths = HashMap::new();
    tx.get_live_dir_paths(|row| {
        let id = row.get::<_, String>(0)?;
        let path = RelativeDirPath::parse(&row.get::<_, String>(1)?).map_err(|error| invalid_text_column(1, error.to_string()))?;
        paths.insert(parse_uuid_column(id, 0)?, path);
        Ok(())
    })?;
    Ok(paths)
}
pub fn get_book_dir_entry_by_path(conn: &Connection, rel_path: &RelativeBookPath) -> Result<Option<(ContentHash, DirId, FileName)>, rusqlite::Error> {
    let mut entry = None;
    conn.get_book_dir_entry_by_path(rel_path.as_str(), |row| {
        let raw_content_hash: String = row.get(0)?;
        let raw_dir_id: String = row.get(1)?;
        let raw_file_name: String = row.get(2)?;
        let content_hash = ContentHash::new(&raw_content_hash);
        let dir_id = parse_uuid_column(raw_dir_id, 1)?;
        let file_name = raw_file_name;
        entry = Some((content_hash, dir_id, file_name));
        Ok(())
    })?;
    Ok(entry)
}
pub fn book_placement_expects_hash(conn: &Connection, placement: &BookPlacement, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    let Some((stored_hash, dir_id, file_name)) = get_book_dir_entry_by_path(conn, &placement.rel_path)? else {
        return Ok(false);
    };
    if stored_hash != placement.content_hash || stored_hash != content_hash.clone() {
        return Ok(false);
    }
    let mut matches = false;
    conn.book_placement_expects_hash(&dir_id.to_string(), content_hash.as_str(), file_name.as_str(), |row| {
        matches = row.get(0)?;
        Ok(())
    })?;
    Ok(matches)
}
const LIVE_BOOK_PAGE: &str = include_str!("sql/local/book_placement_expects_hash_with.sql");

/// Books with at least one placement reachable through live directories.
pub fn live_book_hash_page(conn: &Connection, after: i64) -> rusqlite::Result<Vec<(i64, ContentHash)>> {
    let mut statement = conn.prepare(LIVE_BOOK_PAGE)?;
    statement.query_map([after], |row| Ok((row.get(0)?, ContentHash::new(&row.get::<_, String>(1)?))))?.collect()
}

pub struct LiveBookAssetState {
    pub row_id: i64,
    pub content_hash: ContentHash,
    pub book_upload_allowed: bool,
    pub thumbnail_upload_allowed: bool,
    pub thumbnail_remote: bool,
    pub thumbnail_pending: bool,
}

/// Read transfer flags with the bounded book page, before filesystem checks.
pub fn live_book_asset_page(conn: &Connection, after: i64) -> rusqlite::Result<Vec<LiveBookAssetState>> {
    let mut statement = conn.prepare(include_str!("sql/local/live_book_asset_page_query.sql"))?;
    statement
        .query_map([after], |row| {
            Ok(LiveBookAssetState {
                row_id: row.get(0)?,
                content_hash: ContentHash::new(&row.get::<_, String>(1)?),
                book_upload_allowed: row.get(2)?,
                thumbnail_upload_allowed: row.get(3)?,
                thumbnail_remote: row.get(4)?,
                thumbnail_pending: row.get(5)?,
            })
        })?
        .collect()
}

pub fn queued_book_asset_state(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Option<LiveBookAssetState>> {
    let sql = include_str!("sql/local/queued_book_asset_state_query.sql");
    conn.query_row(&sql, [hash.as_str()], |row| {
        Ok(LiveBookAssetState {
            row_id: row.get(0)?,
            content_hash: ContentHash::new(&row.get::<_, String>(1)?),
            book_upload_allowed: row.get(2)?,
            thumbnail_upload_allowed: row.get(3)?,
            thumbnail_remote: row.get(4)?,
            thumbnail_pending: row.get(5)?,
        })
    })
    .optional()
}
