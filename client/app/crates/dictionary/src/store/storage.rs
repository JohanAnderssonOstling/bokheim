//! Dictionary-owned catalog and component files using shared platform storage.
use super::Storage;
use crate::Result;
use client_platform_runtime::file_storage as files;
use std::path::PathBuf;

#[derive(Clone)]
pub struct LocalStorage {
    pub(super) root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn directory(&self, id: &str) -> PathBuf {
        self.root.join("files").join(id)
    }
}

impl Storage for LocalStorage {
    type Source = Source;

    async fn read_value(&self, key: &str) -> Result<Option<Vec<u8>>> {
        files::read(&self.root.join(key)).await
    }

    async fn write_value(&self, key: &str, bytes: &[u8]) -> Result<()> {
        files::write_atomic(&self.root.join(key), bytes.to_vec()).await
    }

    async fn open(&self, id: &str, name: &str) -> Result<Self::Source> {
        let source = files::open(&self.directory(id).join(name)).await?;
        #[cfg(target_arch = "wasm32")]
        let source = Source(source);
        Ok(source)
    }

    async fn write_all(&self, id: &str, components: Vec<(String, Vec<u8>)>) -> Result<()> {
        let directory = self.directory(id);
        for (name, bytes) in components {
            files::write_atomic(&directory.join(name), bytes).await?;
        }
        Ok(())
    }

    async fn remove(&self, id: &str) -> Result<()> {
        files::remove_directory(&self.directory(id)).await
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type Source = files::Source;
#[cfg(target_arch = "wasm32")]
pub struct Source(files::Source);

#[cfg(target_arch = "wasm32")]
impl crate::ReadAt for Source {
    async fn size(&mut self) -> Result<u64> {
        Ok(self.0.len())
    }
    async fn read_at(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        self.0.read_at(offset, length).await
    }
}
