use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;

struct Entry<V> {
    page: Arc<V>,
    bytes: usize,
}

/// A byte-budgeted LRU cache. Owners of visible pages may retain their Arcs
/// after eviction. Readers without separate visible-page ownership can protect
/// those keys during insertion and trimming; the budget is soft for those keys.
pub struct PageCache<K, V> {
    pages: HashMap<K, Entry<V>>,
    recency: VecDeque<K>,
    byte_budget: usize,
    byte_len: usize,
}

impl<K: Copy + Eq + Hash, V> PageCache<K, V> {
    pub fn new(byte_budget: usize) -> Self {
        Self { pages: HashMap::new(), recency: VecDeque::new(), byte_budget, byte_len: 0 }
    }

    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.pages.keys()
    }

    pub fn contains(&self, key: K) -> bool {
        self.pages.contains_key(&key)
    }

    pub fn peek(&self, key: K) -> Option<&Arc<V>> {
        self.pages.get(&key).map(|entry| &entry.page)
    }

    pub fn get(&mut self, key: K) -> Option<Arc<V>> {
        let page = self.peek(key)?.clone();
        self.recency.retain(|candidate| *candidate != key);
        self.recency.push_back(key);
        Some(page)
    }

    pub fn insert(&mut self, key: K, page: Arc<V>, bytes: usize) {
        self.insert_protected(key, page, bytes, |_| false);
    }

    pub fn insert_protected(&mut self, key: K, page: Arc<V>, bytes: usize, protected: impl Fn(K) -> bool) {
        if bytes > self.byte_budget && !protected(key) {
            return;
        }
        if let Some(previous) = self.pages.remove(&key) {
            self.byte_len -= previous.bytes;
            self.recency.retain(|candidate| *candidate != key);
        }
        self.byte_len += bytes;
        self.pages.insert(key, Entry { page, bytes });
        self.recency.push_back(key);
        self.trim(protected);
    }

    pub fn trim(&mut self, protected: impl Fn(K) -> bool) {
        while self.byte_len > self.byte_budget {
            let Some(index) = self.recency.iter().position(|key| !protected(*key)) else { break };
            let key = self.recency.remove(index).expect("index came from recency");
            if let Some(evicted) = self.pages.remove(&key) {
                self.byte_len -= evicted.bytes;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recently_read_pages_survive_pressure_and_replacement_updates_bytes() {
        let mut cache = PageCache::new(10);
        cache.insert(0, Arc::new("first"), 4);
        cache.insert(1, Arc::new("second"), 4);
        let visible = cache.get(0).unwrap();
        cache.insert(2, Arc::new("third"), 4);
        assert!(!cache.contains(1));
        assert!(cache.contains(0));
        cache.insert(0, Arc::new("replacement"), 6);
        assert_eq!(cache.byte_len(), 10);
        cache.insert(3, Arc::new("oversized"), 11);
        assert!(!cache.contains(3));
        assert_eq!(cache.byte_len(), 10);
        assert_eq!(*visible, "first", "eviction and replacement preserve visible Arcs");
    }

    #[test]
    fn visible_spreads_survive_prefetch_and_release_their_budget_after_navigation() {
        let mut cache = PageCache::new(10);
        for page in 0..2 {
            cache.insert_protected(page, Arc::new(page), 6, |key| key < 2);
        }
        cache.insert_protected(2, Arc::new(2), 6, |key| key < 2);
        assert!(cache.contains(0) && cache.contains(1));
        assert!(!cache.contains(2));
        assert_eq!(cache.byte_len(), 12);
        cache.trim(|key| key == 1);
        assert!(!cache.contains(0));
        assert_eq!(cache.byte_len(), 6);
        cache.insert_protected(3, Arc::new(3), 12, |key| key == 3);
        assert!(!cache.contains(1));
        assert!(cache.contains(3));
        cache.trim(|_| false);
        assert_eq!(cache.byte_len(), 0);
    }
}
