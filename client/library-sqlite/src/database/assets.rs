use super::*;
fn require_directory_marker(directory: &Path, dir_id: &DirId) -> std::io::Result<()> {
    let marker = directory.join(".biblos_uuid");
    let content = fs::read_to_string(&marker)?;
    if content.trim() != dir_id.to_string() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory marker at {} does not identify {dir_id}", marker.display())));
    }
    Ok(())
}
pub fn get_book_file_path(conn: &Connection, content_hash: ContentHash) -> Result<Option<RelativeBookPath>, rusqlite::Error> {
    let mut file_path: Option<String> = None;
    conn.get_book_file_path(content_hash.as_str(), |row| {
        file_path = Some(row.get(0)?);
        Ok(())
    })?;
    file_path.map(|path| RelativeBookPath::parse(&path).map_err(|error| invalid_text_column(0, error.to_string()))).transpose()
}
pub fn get_book_format(conn: &Connection, content_hash: &ContentHash) -> Result<Option<BookFormat>, rusqlite::Error> {
    let mut format = None;
    conn.get_book_materialization_metadata(content_hash.as_str(), |row| {
        let raw: String = row.get(3)?;
        format = Some(BookFormat::from_extension(&raw).ok_or_else(|| invalid_text_column(3, format!("book {content_hash} has no known format")))?);
        Ok(())
    })?;
    Ok(format)
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
#[derive(Debug, Clone)]
pub struct BookAsset {
    pub content_hash: ContentHash,
    pub rel_path: RelativeBookPath,
    pub downloaded: bool,
}
fn parse_content_hash_column(raw: &str, _column: usize) -> Result<ContentHash, rusqlite::Error> {
    Ok(ContentHash::new(&raw))
}
pub fn get_book_assets(conn: &Connection) -> Result<Vec<BookAsset>, rusqlite::Error> {
    let mut assets = Vec::new();
    conn.get_book_assets(|row| {
        let raw_content_hash: String = row.get(0)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 0)?;
        let raw_rel_path: String = row.get(1)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let downloaded: bool = row.get(2)?;
        assets.push(BookAsset { content_hash, rel_path, downloaded });
        Ok(())
    })?;
    Ok(assets)
}
pub(super) fn now_ts_secs() -> Result<i64, rusqlite::Error> {
    unix_secs()
}
pub fn upsert_blob_local_ref(conn: &Connection, kind: BlobKind, content_hash: &ContentHash, path: &str, exists: bool) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    let exists_flag = if exists { 1 } else { 0 };
    conn.upsert_blob_local_ref(kind.storage(), content_hash.as_str(), path, exists_flag, now)?;
    Ok(())
}
pub fn get_locally_present_blob_hashes(conn: &Connection, kind: BlobKind) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut hashes = Vec::new();
    conn.get_locally_present_blob_hashes(kind.storage(), |row| {
        let raw_content_hash: String = row.get(0)?;
        hashes.push(parse_content_hash_column(&raw_content_hash, 0)?);
        Ok(())
    })?;
    Ok(hashes)
}
pub fn get_blob_local_path(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<Option<String>, rusqlite::Error> {
    let mut path: Option<String> = None;
    conn.get_blob_local_path(kind.storage(), content_hash.as_str(), |row| {
        path = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(path)
}
pub fn get_blob_local_paths(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<Vec<String>, rusqlite::Error> {
    let mut paths = Vec::new();
    conn.get_blob_local_paths(kind.storage(), content_hash.as_str(), |row| {
        paths.push(row.get(0)?);
        Ok(())
    })?;
    Ok(paths)
}
/// Resolves a tombstoned placement to `(library-relative path, content_hash)`,
/// or None if it is no longer tombstoned (an add resurrected it later in the same
/// pull batch, so its file must not be touched). The content hash lets the caller
/// relocate the file to a live placement of the same content (a move).
pub fn resolve_removed_book_dir(conn: &Connection, dir_id: &str, content_hash: ContentHash) -> Result<Option<(RelativeBookPath, Option<ContentHash>)>, rusqlite::Error> {
    let mut result = None;
    conn.resolve_removed_book_dir(dir_id, content_hash.as_str(), |row| {
        let raw_rel_path: String = row.get(0)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_content_hash: String = row.get(1)?;
        let content_hash = if raw_content_hash.is_empty() { None } else { Some(parse_content_hash_column(&raw_content_hash, 1)?) };
        result = Some((rel_path, content_hash));
        Ok(())
    })?;
    Ok(result)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookPath {
    pub content_hash: ContentHash,
    pub rel_path: RelativeBookPath,
}

/// Assigns a local-only filename to a pending download.  The synchronized
/// placement identifies only `(book, directory)`; names are derived at the
/// boundary where bytes are materialized and are never emitted into the
/// outbox.
pub fn materialize_download_path(conn: &Connection, library_root: &Path, placement: &BookPlacement) -> Result<RelativeBookPath, rusqlite::Error> {
    let Some((content_hash, dir_id, current_name)) = get_book_dir_entry_by_path(conn, &placement.rel_path)? else {
        return Err(invalid_text_column(0, "download placement no longer exists"));
    };
    if content_hash != placement.content_hash {
        return Err(invalid_text_column(0, "download placement identity mismatch"));
    }
    let dir_path = if dir_id == sync_common::ROOT_DIR_ID {
        None
    } else {
        let components = get_dir_path_components(conn, &dir_id.to_string())?;
        Some(dir_path_from_components(&components)?.ok_or_else(|| invalid_text_column(0, "download directory has no live path"))?)
    };
    let absolute_dir = dir_path.as_ref().map_or_else(|| library_root.to_path_buf(), |path| path.resolve_under(library_root));
    let relative_path = |file_name: &FileName| {
        let path = dir_path.as_ref().map_or_else(|| format!("/{}", file_name.as_str()), |path| format!("{}/{}", path.as_path().display(), file_name.as_str()));
        RelativeBookPath::parse(&path).map_err(|error| invalid_text_column(0, error.to_string()))
    };

    // Re-materializing a book that already has its file keeps that file's name.
    // Without this a repeated or resumed download would find the book's own file
    // occupying the name it just chose, escalate to the next rung, and write a
    // second copy that no placement row then references. Checked before the
    // metadata below so idempotence never depends on it.
    if absolute_dir.join(current_name.as_str()).exists() {
        return relative_path(&current_name);
    }

    let mut materialization_metadata = None;
    conn.get_book_materialization_metadata(content_hash.as_str(), |row| {
        materialization_metadata = Some((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?));
        Ok(())
    })?;
    let (title, publication, contributors, format): (String, Vec<u8>, Vec<u8>, String) = materialization_metadata.ok_or_else(|| invalid_text_column(0, "download book no longer exists"))?;
    // The format is a property of the bytes, synchronized with the lifecycle. A
    // download must not guess it from the placeholder name the placement was
    // seeded with: that produced a name no reader (and no later scan) accepts.
    let format = BookFormat::from_extension(&format).ok_or_else(|| invalid_text_column(3, format!("book {content_hash} has no known format")))?;
    let publication = if publication.is_empty() { PublicationMetadata::default() } else { sync_common::wire::decode(&publication, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid_text_column(1, error.to_string()))? };
    let contributors = if contributors.is_empty() { Vec::new() } else { sync_common::wire::decode(&contributors, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid_text_column(2, error.to_string()))? };
    let metadata = BookMetadataState { title, contributors, description: String::new(), publication };
    let file_name = crate::filenames::materialize(content_hash.clone(), &metadata, format, |candidate| {
        // The name this placement already holds belongs to this book, so it is
        // free for it: the database half of the probe excludes the book's own
        // row, and the filesystem half must agree.
        if candidate == &current_name {
            return false;
        }
        let mut claimed = true;
        let _ = conn.book_dir_file_name_claimed(&dir_id.to_string(), candidate.as_str(), content_hash.as_str(), |row| {
            claimed = row.get(0)?;
            Ok(())
        });
        claimed || absolute_dir.join(candidate.as_str()).exists()
    });
    conn.set_book_dir_file_name(file_name.as_str(), &dir_id.to_string(), content_hash.as_str())?;
    relative_path(&file_name)
}
pub fn get_book_paths_by_content_hash(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<BookPath>, rusqlite::Error> {
    let mut paths = Vec::new();
    conn.get_book_paths_by_content_hash(content_hash.as_str(), |row| {
        let raw_content_hash: String = row.get(0)?;
        let content_hash = ContentHash::new(&raw_content_hash);
        let raw_rel_path: String = row.get(1)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(1, error.to_string()))?;
        paths.push(BookPath { content_hash, rel_path });
        Ok(())
    })?;
    Ok(paths)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPathComponent {
    pub id: DirId,
    pub name: FileName,
}
fn collect_dir_path_components<F>(mut query: F) -> Result<Vec<DirPathComponent>, rusqlite::Error>
where
    F: FnMut(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>,
{
    let mut components = Vec::new();
    let mut row_fn = |row: &rusqlite::Row<'_>| {
        let raw_id: String = row.get(0)?;
        let raw_name: String = row.get(1)?;
        let id = parse_uuid_column(raw_id, 0)?;
        let name = raw_name;
        components.push(DirPathComponent { id, name });
        Ok(())
    };
    query(&mut row_fn)?;
    Ok(components)
}
pub fn get_dir_path_components(conn: &Connection, dir_id: &str) -> Result<Vec<DirPathComponent>, rusqlite::Error> {
    collect_dir_path_components(|row_fn| conn.get_dir_path_components(dir_id, row_fn))
}
pub(crate) fn dir_path_from_components(components: &[DirPathComponent]) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
    if components.is_empty() {
        return Ok(None);
    }
    RelativeDirPath::from_components(components.iter().map(|component| component.name.as_str())).map(Some).map_err(|error| invalid_text_column(1, error.to_string()))
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
pub fn materialize_dir_with_markers(conn: &Connection, library_path: &Path, dir_id: &str) -> std::io::Result<Option<PathBuf>> {
    let components = get_dir_path_components(conn, dir_id).map_err(std::io::Error::other)?;
    materialize_dir_components_with_markers(library_path, &components, true)
}
pub fn materialize_dir_parent_with_markers(conn: &Connection, library_path: &Path, dir_id: &str) -> std::io::Result<Option<PathBuf>> {
    let components = get_dir_path_components(conn, dir_id).map_err(std::io::Error::other)?;
    materialize_dir_components_with_markers(library_path, &components, false)
}
pub(super) fn materialize_dir_components_with_markers(library_root: &Path, components: &[DirPathComponent], include_leaf: bool) -> std::io::Result<Option<PathBuf>> {
    if components.is_empty() {
        return Ok(None);
    }
    let end = if include_leaf { components.len() } else { components.len().saturating_sub(1) };
    let path = if end == 0 {
        library_root.to_path_buf()
    } else {
        let relative = RelativeDirPath::from_components(components[..end].iter().map(|component| component.name.as_str())).map_err(std::io::Error::other)?;
        relative.resolve_under(library_root)
    };
    let mut current = library_root.to_path_buf();
    for component in &components[..end] {
        current.push(component.name.as_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory materialization crosses a symlink: {}", current.display()))),
            Ok(metadata) if !metadata.is_dir() => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory materialization path is not a directory: {}", current.display()))),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(error),
        }
        let marker = current.join(".biblos_uuid");
        if !marker.exists() {
            fs::write(marker, component.id.to_string())?;
        } else {
            require_directory_marker(&current, &component.id)?;
        }
    }
    Ok(Some(path))
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
pub fn local_file_fingerprint(path: &Path) -> std::io::Result<u64> {
    let mut hasher = Xxh3::new();
    let metadata = fs::metadata(path)?;
    let modified_time = metadata.modified()?.duration_since(UNIX_EPOCH).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    hasher.write_u128(modified_time.as_nanos());
    hasher.write_u64(metadata.len());
    #[cfg(unix)]
    {
        hasher.write_u64(metadata.ino());
        hasher.write_u64(metadata.dev());
    }
    Ok(hasher.digest())
}
pub fn stamp_book_dir_local_hash(conn: &Connection, dir_id: &str, content_hash: ContentHash, _file_name: &str, path: &Path) -> std::io::Result<()> {
    let local_hash = local_file_fingerprint(path)?.to_string();
    conn.update_book_dir_local_hash(dir_id, content_hash.as_str(), &local_hash).map_err(std::io::Error::other)?;
    Ok(())
}
pub fn stamp_book_dir_local_hash_by_rel_path(conn: &Connection, rel_path: &RelativeBookPath, content_hash: ContentHash, abs_path: &Path) -> std::io::Result<()> {
    let Some((found_content_hash, dir_id, file_name)) = get_book_dir_entry_by_path(conn, rel_path).map_err(std::io::Error::other)? else {
        return Ok(());
    };
    if found_content_hash != content_hash {
        return Ok(());
    }
    stamp_book_dir_local_hash(conn, &dir_id.to_string(), content_hash, file_name.as_str(), abs_path)
}
pub fn get_book_hashes(conn: &Connection, content_hash: ContentHash) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut hashes = Vec::new();
    conn.get_book_hashes(content_hash.as_str(), |row| {
        let raw_content_hash: String = row.get(0)?;
        hashes.push(parse_content_hash_column(&raw_content_hash, 0)?);
        Ok(())
    })?;
    Ok(hashes)
}
pub fn is_blob_local(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    let mut exists = 0_i32;
    conn.get_blob_local_presence(kind.storage(), content_hash.as_str(), |row| {
        exists = row.get::<_, i32>(0)?;
        Ok(())
    })?;
    Ok(exists > 0)
}
pub fn get_remote_asset_hashes(conn: &Connection, kind: BlobKind) -> Result<HashSet<ContentHash>, rusqlite::Error> {
    let mut hashes = HashSet::new();
    conn.get_remote_asset_hashes(kind.storage(), |row| {
        let raw_content_hash: String = row.get(0)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 0)?;
        hashes.insert(content_hash);
        Ok(())
    })?;
    Ok(hashes)
}
pub fn record_remote_asset(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.record_remote_asset(kind.storage(), content_hash.as_str())?;
    Ok(())
}
pub fn forget_remote_asset(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.forget_remote_asset(kind.storage(), content_hash.as_str())?;
    Ok(())
}
pub fn get_asset_requests(conn: &Connection, kind: BlobKind) -> Result<Vec<(ContentHash, TransferOrigin)>, rusqlite::Error> {
    read_download_requests(conn, kind.storage(), |raw| parse_content_hash_column(raw, 0))
}
pub fn add_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    add_download_request(conn, kind.storage(), hash.as_str(), origin)
}

/// Atomically records explicit download intent for every live asset belonging
/// to the requested publications. A publication without a live placement
/// aborts the entire request set.
pub fn request_book_downloads(conn: &mut Connection, content_hashes: &[ContentHash], origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    let transaction = conn.transaction()?;
    for content_hash in content_hashes {
        let hashes = get_book_hashes(&transaction, *content_hash)?;
        if hashes.is_empty() {
            return Err(invalid_text_column(0, format!("book {content_hash} has no live placement")));
        }
        for hash in hashes {
            add_asset_request(&transaction, BlobKind::Book, &hash, origin)?;
        }
    }
    transaction.commit()
}
pub fn complete_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash) -> Result<(), rusqlite::Error> {
    complete_download_request(conn, kind.storage(), hash.as_str())
}
fn read_download_requests<T>(conn: &Connection, target_kind: &str, parse: impl Fn(&str) -> Result<T, rusqlite::Error>) -> Result<Vec<(T, TransferOrigin)>, rusqlite::Error> {
    let mut requests = Vec::new();
    conn.get_download_requests(target_kind, |row| {
        let raw_key: String = row.get(0)?;
        let raw_origin: String = row.get(1)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(1, "invalid transfer origin"))?;
        requests.push((parse(&raw_key)?, origin));
        Ok(())
    })?;
    Ok(requests)
}
fn add_download_request(conn: &Connection, target_kind: &str, target_key: &str, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    conn.add_download_request(target_kind, target_key, origin.storage(), now_ts_secs()?)?;
    Ok(())
}
fn complete_download_request(conn: &Connection, target_kind: &str, target_key: &str) -> Result<(), rusqlite::Error> {
    conn.complete_download_request(target_kind, target_key)?;
    Ok(())
}
