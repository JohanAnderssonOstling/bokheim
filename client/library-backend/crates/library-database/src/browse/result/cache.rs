//! Retention policy for prepared browse results, including lazy order growth.
//! Revision keys enforce freshness; entry count and memory alone bound retention.
use super::key::ResultKey;
use super::{BrowseResult, DatabaseError};
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

pub(super) const MAX_RESULTS: usize = 8;
pub(super) const MAX_CACHED_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct BrowseResults {
    owner: Uuid,
    pub(super) entries: VecDeque<Entry>,
    #[cfg(test)]
    pub(super) preparations: usize,
}

pub(super) struct Entry {
    key: ResultKey,
    pub(super) result: Arc<BrowseResult>,
}

impl Default for BrowseResults {
    fn default() -> Self {
        Self {
            owner: Uuid::new_v4(),
            entries: VecDeque::new(),
            #[cfg(test)]
            preparations: 0,
        }
    }
}

impl BrowseResults {
    pub(super) fn revision(&self, key: &ResultKey) -> Result<String, DatabaseError> {
        let mut hash = blake3::Hasher::new();
        hash.update(self.owner.as_bytes());
        hash.update(&serde_json::to_vec(key).map_err(DatabaseError::operation)?);
        Ok(hash.finalize().to_hex().to_string())
    }

    pub(super) fn get(&mut self, key: &ResultKey) -> Option<Arc<BrowseResult>> {
        let index = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(index)?;
        let result = Arc::clone(&entry.result);
        self.entries.push_back(entry);
        Some(result)
    }

    pub(super) fn insert(&mut self, key: ResultKey, result: Arc<BrowseResult>) {
        #[cfg(test)]
        {
            self.preparations += 1;
        }
        // An oversized result remains usable for this read. Its deterministic
        // revision allows later pages to rebuild it without retaining it.
        if result.estimated_bytes() > MAX_CACHED_BYTES {
            return;
        }
        self.entries.push_back(Entry { key, result });
        self.trim();
    }

    pub(super) fn trim(&mut self) {
        // Read sizes again because preparing lazy section orders can grow an entry.
        while self.entries.len() > MAX_RESULTS || self.entries.iter().map(|entry| entry.result.estimated_bytes()).sum::<usize>() > MAX_CACHED_BYTES {
            self.entries.pop_front();
        }
    }
}
