//! One disposable SQLite cache for all catalog covers, including browser OPFS.
use library_database::rusqlite::{params, Connection, Error, ErrorCode, OptionalExtension, Result, TransactionBehavior};
use library_model::CardKey;
use std::{collections::HashMap, path::Path, time::Duration};

pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: i64 = 8192;
const MEMORY_BYTES: usize = 8 * 1024 * 1024;
const MEMORY_ENTRIES: usize = 1024;

pub(super) type Address = (String, CardKey, String);

struct MemoryEntry {
    cover: Option<Vec<u8>>,
    accessed: u64,
}

#[derive(Default)]
pub(super) struct CoverCache {
    memory: HashMap<Address, MemoryEntry>,
    memory_bytes: usize,
    clock: u64,
    disk: Option<DiskCache>,
}

impl CoverCache {
    pub(super) fn configure(&mut self, path: &Path, max_bytes: u64) -> Result<()> {
        let disk = DiskCache::open(path, max_bytes)?;
        self.clear();
        self.disk = Some(disk);
        Ok(())
    }

    // Entering the reader releases JPEG memory without deleting offline covers.
    pub(super) fn clear(&mut self) {
        self.memory.clear();
        self.memory_bytes = 0;
    }

    pub(super) fn get(&mut self, address: &Address) -> Option<Option<Vec<u8>>> {
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.memory.get_mut(address) {
            entry.accessed = self.clock;
            return Some(entry.cover.clone());
        }
        match self.disk.as_mut()?.get(address) {
            Ok(cover) => cover.map(Some),
            Err(error) => {
                log::warn!("Catalog cover disk cache disabled: {error}");
                self.disk = None;
                None
            }
        }
    }

    pub(super) fn insert(&mut self, address: Address, cover: Option<Vec<u8>>) {
        if let Some(old) = self.memory.remove(&address) {
            self.memory_bytes -= old.cover.as_ref().map_or(0, Vec::len);
        }
        if let Some(bytes) = &cover {
            if let Some(disk) = self.disk.as_mut() {
                match disk.insert(&address, bytes) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(error) => log::warn!("Catalog cover disk cache write failed: {error}"),
                }
            }
        }
        let size = cover.as_ref().map_or(0, Vec::len);
        if size > MEMORY_BYTES {
            return;
        }
        while self.memory_bytes + size > MEMORY_BYTES || self.memory.len() >= MEMORY_ENTRIES {
            let Some(oldest) = self.memory.iter().min_by_key(|(_, entry)| entry.accessed).map(|(key, _)| key.clone()) else { break };
            let old = self.memory.remove(&oldest).unwrap();
            self.memory_bytes -= old.cover.as_ref().map_or(0, Vec::len);
        }
        self.clock = self.clock.wrapping_add(1);
        self.memory_bytes += size;
        self.memory.insert(address, MemoryEntry { cover, accessed: self.clock });
    }

    pub(super) fn remove_book(&mut self, origin: &str, key: CardKey) {
        self.memory.retain(|address, entry| {
            if address.0 == origin && address.1 == key {
                self.memory_bytes -= entry.cover.as_ref().map_or(0, Vec::len);
                false
            } else {
                true
            }
        });
        if let Some(disk) = &self.disk {
            if let Err(error) = disk.connection.execute("DELETE FROM cover WHERE origin=?1 AND book_key=?2", params![origin, &key.0[..]]) {
                log::warn!("Catalog cover disk cache disabled after invalidation failed: {error}");
                self.disk = None;
            }
        }
    }
}

struct DiskCache {
    connection: Connection,
    payload_limit: i64,
}

impl DiskCache {
    fn open(path: &Path, max_bytes: u64) -> Result<Self> {
        let pages = max_bytes / 4096;
        if !(16..=u32::MAX as u64 - 1).contains(&pages) {
            return Err(Error::InvalidParameterName("cover cache limit must be at least 64 KiB and fit SQLite's page limit".into()));
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA page_size=4096; PRAGMA auto_vacuum=FULL; PRAGMA journal_mode=DELETE; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-256; PRAGMA foreign_keys=ON;")?;
        connection.execute_batch(include_str!("cover_cache.sql"))?;
        // Leave room for keys and indexes. max_page_count also enforces the
        // physical database ceiling, rather than just counting image payloads.
        let mut cache = Self { connection, payload_limit: (max_bytes - (max_bytes / 10).max(32 * 1024)) as i64 };
        cache.trim()?;
        let actual: i64 = cache.connection.pragma_update_and_check(None, "max_page_count", pages as u32, |row| row.get(0))?;
        if actual > pages as i64 {
            return Err(Error::InvalidParameterName("cover cache exceeds the requested disk limit".into()));
        }
        Ok(cache)
    }

    fn get(&mut self, address: &Address) -> Result<Option<Vec<u8>>> {
        let (origin, key, suffix) = address;
        let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cover = tx.query_row("SELECT cover_data.bytes FROM cover JOIN cover_data USING(id) WHERE origin=?1 AND book_key=?2 AND suffix=?3", params![origin, &key.0[..], suffix], |row| row.get(0)).optional()?;
        if cover.is_some() {
            tx.execute("UPDATE cover SET accessed=(SELECT COALESCE(MAX(accessed),0)+1 FROM cover) WHERE origin=?1 AND book_key=?2 AND suffix=?3", params![origin, &key.0[..], suffix])?;
        }
        tx.commit()?;
        Ok(cover)
    }

    fn trim(&mut self) -> Result<()> {
        let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::prune(&tx, self.payload_limit, MAX_ENTRIES)?;
        tx.commit()
    }

    fn prune(connection: &Connection, available: i64, max_entries: i64) -> Result<usize> {
        let (mut used, mut count): (i64, i64) = connection.query_row("SELECT COALESCE(SUM(size),0),COUNT(*) FROM cover INDEXED BY cover_lru", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let original_count = count;
        if used <= available && count <= max_entries {
            return Ok(0);
        }
        let oldest = connection.prepare("SELECT id,size FROM cover ORDER BY accessed,id")?.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?.collect::<Result<Vec<_>>>()?;
        for (id, size) in oldest {
            if used <= available && count <= max_entries {
                break;
            }
            connection.execute("DELETE FROM cover WHERE id=?1", [id])?;
            used -= size;
            count -= 1;
        }
        Ok((original_count - count) as usize)
    }

    fn insert(&mut self, address: &Address, bytes: &[u8]) -> Result<bool> {
        let Ok(size) = i64::try_from(bytes.len()) else { return Ok(false) };
        let available = self.payload_limit - size;
        if available < 0 {
            return Ok(false);
        }
        loop {
            match self.insert_in_transaction(address, bytes, size, available) {
                Err(Error::SqliteFailure(error, _)) if error.code == ErrorCode::DiskFull => {
                    // SQLITE_FULL rolls back the insert, including its pruning.
                    // Commit pruning separately to reclaim physical pages. If
                    // payload pruning freed nothing, evict one oldest entry
                    // to make room for index/record overhead instead.
                    let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let pruned = Self::prune(&tx, available, MAX_ENTRIES - 1)?;
                    let removed = if pruned == 0 { tx.execute("DELETE FROM cover WHERE id=(SELECT id FROM cover ORDER BY accessed,id LIMIT 1)", [])? } else { 0 };
                    tx.commit()?;
                    if removed == 0 && pruned == 0 {
                        return Ok(false);
                    }
                }
                result => return result,
            }
        }
    }

    fn insert_in_transaction(&mut self, address: &Address, bytes: &[u8], size: i64, available: i64) -> Result<bool> {
        let (origin, key, suffix) = address;
        let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM cover WHERE origin=?1 AND book_key=?2 AND suffix=?3", params![origin, &key.0[..], suffix])?;
        Self::prune(&tx, available, MAX_ENTRIES - 1)?;
        tx.execute("INSERT INTO cover(origin,book_key,suffix,size,accessed) VALUES(?1,?2,?3,?4,(SELECT COALESCE(MAX(accessed),0)+1 FROM cover))", params![origin, &key.0[..], suffix, size])?;
        tx.execute("INSERT INTO cover_data(id,bytes) VALUES(?1,?2)", params![tx.last_insert_rowid(), bytes])?;
        tx.commit()?;
        Ok(true)
    }
}
