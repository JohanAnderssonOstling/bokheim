//! Disposable cover files with persistent byte accounting and oldest-write eviction.
use client_platform_runtime::executor::spawn_detached;
use client_platform_runtime::file_storage::{self as files, DirectoryEntry};
use library_model::CardKey;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{atomic::Ordering, Mutex, PoisonError};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

use super::COVER_GENERATION;

pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: u64 = 8192;
const MAX_MISSING_COVERS: usize = 1024;

pub(super) type Address = (String, CardKey, String);

#[derive(Default)]
pub(super) struct CoverCache {
    missing: Mutex<HashSet<Address>>,
    // Serialize publication, accounting, eviction and artwork replacement.
    disk: AsyncMutex<Option<DiskCache>>,
}

impl CoverCache {
    pub(super) async fn configure(&self, path: &Path, max_bytes: u64) -> Result<(), String> {
        let mut disk = self.disk.lock().await;
        let opened = DiskCache::open(path, max_bytes).await?;
        self.clear();
        *disk = Some(opened);
        Ok(())
    }

    pub(super) fn clear(&self) {
        self.missing.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub(super) async fn get(&self, address: &Address) -> Option<Option<Vec<u8>>> {
        if self.missing.lock().unwrap_or_else(PoisonError::into_inner).contains(address) {
            return Some(None);
        }
        let mut disk = self.disk.lock().await;
        match disk.as_mut()?.get(address).await {
            Ok(cover) => cover.map(Some),
            Err(error) => {
                log::warn!("Catalog cover disk cache disabled: {error}");
                *disk = None;
                None
            }
        }
    }

    pub(super) async fn insert(&'static self, address: Address, cover: Option<Vec<u8>>, generation: u64) {
        self.store(address, cover, generation, false).await;
    }

    async fn insert_locked(&self, disk: &mut Option<DiskCache>, address: Address, cover: Option<Vec<u8>>, generation: u64) {
        {
            let mut missing = self.missing.lock().unwrap_or_else(PoisonError::into_inner);
            if COVER_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            missing.remove(&address);
            if cover.is_none() {
                if missing.len() >= MAX_MISSING_COVERS {
                    missing.clear();
                }
                missing.insert(address);
                return;
            }
        }
        if let (Some(cache), Some(bytes)) = (disk.as_mut(), cover.as_ref()) {
            if let Err(error) = cache.insert(&address, bytes).await {
                log::warn!("Catalog cover disk cache write failed: {error}");
            }
        }
    }

    pub(super) async fn replace(&'static self, address: Address, cover: Option<Vec<u8>>, generation: u64) {
        self.store(address, cover, generation, true).await;
    }

    async fn store(&'static self, address: Address, cover: Option<Vec<u8>>, generation: u64, replace: bool) {
        let (finished, result) = oneshot::channel();
        // Once admitted, a write retains its lock until files and accounting
        // are complete, even if the requesting view is closed in the meantime.
        spawn_detached(async move {
            let mut disk = self.disk.lock().await;
            if COVER_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let generation = if replace {
                let next = {
                    let mut missing = self.missing.lock().unwrap_or_else(PoisonError::into_inner);
                    if COVER_GENERATION.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let next = COVER_GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
                    missing.retain(|cached| cached.0 != address.0 || cached.1 != address.1);
                    next
                };
                if let Some(cache) = disk.as_mut() {
                    if let Err(error) = cache.remove_book(&address.0, address.1).await {
                        log::warn!("Catalog cover disk cache disabled after invalidation failed: {error}");
                        *disk = None;
                    }
                }
                next
            } else {
                generation
            };
            self.insert_locked(&mut disk, address, cover, generation).await;
            let _ = finished.send(());
        });
        let _ = result.await;
    }
}

#[derive(Clone, Copy, Default)]
struct Accounting {
    bytes: u64,
    entries: u64,
}

struct DiskCache {
    directory: PathBuf,
    max_bytes: u64,
    accounting: Option<Accounting>,
}

impl DiskCache {
    async fn open(path: &Path, max_bytes: u64) -> Result<Self, String> {
        if max_bytes == 0 {
            return Err("cover cache limit must be greater than zero".into());
        }
        let accounting = files::read(&path.join("size")).await?.and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).ok()?;
            let (bytes, entries) = text.trim().split_once(' ')?;
            Some(Accounting { bytes: bytes.parse().ok()?, entries: entries.parse().ok()? })
        });
        let mut cache = Self { directory: path.to_owned(), max_bytes, accounting };
        if cache.accounting.is_none() {
            let accounting = Self::count(&cache.entries().await?);
            cache.finish(accounting).await?;
        }
        if cache.accounting.is_some_and(|accounting| accounting.bytes > max_bytes || accounting.entries > MAX_ENTRIES) {
            let mut accounting = cache.begin().await?;
            cache.prune(&mut accounting, max_bytes, MAX_ENTRIES, None).await?;
            cache.finish(accounting).await?;
        }
        Ok(cache)
    }

    fn book_prefix(origin: &str, key: CardKey) -> String {
        format!("{}-{key}-", blake3::hash(origin.as_bytes()).to_hex())
    }

    fn name(address: &Address) -> String {
        format!("{}{}.jpg", Self::book_prefix(&address.0, address.1), address.2)
    }

    async fn entries(&self) -> Result<Vec<DirectoryEntry>, String> {
        Ok(files::list_directory(&self.directory).await?.into_iter().filter(|entry| entry.name.ends_with(".jpg")).collect())
    }

    fn count(entries: &[DirectoryEntry]) -> Accounting {
        Accounting { bytes: entries.iter().fold(0u64, |total, entry| total.saturating_add(entry.size)), entries: entries.len() as u64 }
    }

    // Invalidate the persisted total before touching covers. Cancellation or IO
    // failure leaves both copies invalid, so the next writer rebuilds them.
    async fn begin(&mut self) -> Result<Accounting, String> {
        let accounting = match self.accounting.take() {
            Some(accounting) => accounting,
            None => Self::count(&self.entries().await?),
        };
        files::write_atomic(&self.directory.join("size"), b"incomplete\n".to_vec()).await?;
        Ok(accounting)
    }

    async fn finish(&mut self, accounting: Accounting) -> Result<(), String> {
        files::write_atomic(&self.directory.join("size"), format!("{} {}\n", accounting.bytes, accounting.entries).into_bytes()).await?;
        self.accounting = Some(accounting);
        Ok(())
    }

    async fn get(&self, address: &Address) -> Result<Option<Vec<u8>>, String> {
        // Reads leave the download timestamp and accounting file untouched.
        files::read(&self.directory.join(Self::name(address))).await
    }

    async fn prune(&self, accounting: &mut Accounting, available: u64, max_entries: u64, replacing: Option<&str>) -> Result<(), String> {
        if accounting.bytes <= available && accounting.entries <= max_entries {
            return Ok(());
        }
        let mut entries = self.entries().await?;
        entries.retain(|entry| Some(entry.name.as_str()) != replacing);
        // A full scan also reconciles the counter with the files that remain.
        *accounting = Self::count(&entries);
        entries.sort_unstable_by(|a, b| (a.modified_ms, &a.name).cmp(&(b.modified_ms, &b.name)));
        for entry in entries {
            if accounting.bytes <= available && accounting.entries <= max_entries {
                break;
            }
            files::remove_file(&self.directory.join(entry.name)).await?;
            accounting.bytes = accounting.bytes.saturating_sub(entry.size);
            accounting.entries = accounting.entries.saturating_sub(1);
        }
        Ok(())
    }

    async fn insert(&mut self, address: &Address, bytes: &[u8]) -> Result<(), String> {
        let size = bytes.len() as u64;
        if size > self.max_bytes {
            return Ok(());
        }
        let name = Self::name(address);
        let path = self.directory.join(&name);
        let previous = files::file_size(&path).await?;
        let mut accounting = self.begin().await?;
        if let Some(size) = previous {
            accounting.bytes = accounting.bytes.saturating_sub(size);
            accounting.entries = accounting.entries.saturating_sub(1);
        }
        self.prune(&mut accounting, self.max_bytes - size, MAX_ENTRIES - 1, Some(&name)).await?;
        files::write_atomic(&path, bytes.to_vec()).await?;
        accounting.bytes += size;
        accounting.entries += 1;
        self.finish(accounting).await
    }

    async fn remove_book(&mut self, origin: &str, key: CardKey) -> Result<(), String> {
        let prefix = Self::book_prefix(origin, key);
        let entries = self.entries().await?;
        let mut accounting = self.begin().await?;
        for entry in entries.into_iter().filter(|entry| entry.name.starts_with(&prefix)) {
            files::remove_file(&self.directory.join(entry.name)).await?;
            accounting.bytes = accounting.bytes.saturating_sub(entry.size);
            accounting.entries = accounting.entries.saturating_sub(1);
        }
        self.finish(accounting).await
    }
}
