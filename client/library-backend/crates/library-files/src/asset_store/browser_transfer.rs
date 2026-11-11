//! Stream verified storage copies into OPFS without buffering the whole book.
use super::{imp, AssetStore, AssetStoreError, StagedBook, VerifiedBook};
use content_address::ContentHash;

impl AssetStore {
    /// Stage a pinned, verified local book using its existing byte checksum.
    pub async fn copy_verified_book(&self, source: &VerifiedBook) -> Result<StagedBook, AssetStoreError> {
        let checksum = source.book_id.content_hash().ok_or_else(|| AssetStoreError::operation("verified source has no byte checksum"))?;
        Ok(StagedBook { inner: imp::stage_verified_book(&self.0, &source.reader, checksum).await? })
    }

    pub async fn stage_transfer_chunks<F>(&self, length: u64, expected: Option<ContentHash>, mut read: F) -> Result<StagedBook, AssetStoreError>
    where
        F: AsyncFnMut(u64, usize) -> Result<Vec<u8>, AssetStoreError>,
    {
        if length == 0 {
            return Err(AssetStoreError::operation("book is empty"));
        }
        let mut writer = imp::StagingWriter::new(&self.0).await?;
        let mut offset = 0;
        while offset < length {
            let count = (length - offset).min(256 * 1024) as usize;
            let bytes = read(offset, count).await?;
            if bytes.len() != count {
                return Err(AssetStoreError::operation("transferred book ended unexpectedly"));
            }
            writer.write_all(&bytes).await?;
            offset += count as u64;
        }
        Ok(StagedBook { inner: writer.finish(&self.0, expected).await? })
    }
}
