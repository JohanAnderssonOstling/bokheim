use super::super::{error, open_library_id, params, valid_open_library_author_biography, AuthorProfileRecord, MetadataError, Path, SystemTime, AUTHOR_PROFILE_BUILDER_VERSION, AUTHOR_PROFILE_SCHEMA_VERSION, UNIX_EPOCH};
use super::pipeline::read_open_library_resumable;
use super::sidecar::{SidecarBuilder, SidecarSpec};

/// Builds an immutable Open Library author-biography sidecar without
/// duplicating the core work/edition-to-author relationship tables.
pub fn import_author_profile_snapshot(authors_path: &Path, output_path: &Path, dump_date: &str, record_limit: Option<usize>) -> Result<(), MetadataError> {
    let mut builder = SidecarBuilder::open(authors_path, output_path, dump_date, record_limit, SidecarSpec::author_profile(AUTHOR_PROFILE_BUILDER_VERSION))?;
    let author_records = read_open_library_resumable::<AuthorProfileRecord>(builder.connection_mut(), authors_path, record_limit, "author_biographies", |transaction, records| {
        let mut insert = transaction.prepare_cached("INSERT OR REPLACE INTO author_profile(author_id,biography) VALUES(?1,?2)").map_err(error)?;
        for record in records {
            let (Some(author_id), Some(biography)) = (open_library_id(&record.key, 'A'), record.bio.as_deref().and_then(valid_open_library_author_biography)) else { continue };
            insert.execute(params![author_id, biography]).map_err(error)?;
        }
        Ok(())
    })?;
    builder.finalize_once("author_profile_finalize", |connection| {
        let biography_records: i64 = connection.query_row("SELECT COUNT(*) FROM author_profile", (), |row| row.get(0)).map_err(error)?;
        let imported_at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis()).map_err(error)?;
        connection
            .execute(
                "INSERT INTO author_profile_snapshot(singleton,schema_version,dump_date,imported_at_ms,author_records,biography_records) VALUES(1,?1,?2,?3,?4,?5)",
                params![AUTHOR_PROFILE_SCHEMA_VERSION, dump_date, imported_at_ms, i64::try_from(author_records).map_err(error)?, biography_records],
            )
            .map_err(error)?;
        Ok(())
    })?;
    builder.publish()
}
