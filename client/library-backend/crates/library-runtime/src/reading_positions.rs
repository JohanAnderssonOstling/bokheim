//! Publication timing owned by the library actor. Sync reads immutable snapshots.

use std::collections::{HashMap, HashSet};
use std::time::Duration;
use sync_common::ContentHash;
use web_time::Instant;

const QUIET_PERIOD: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct PendingPosition {
    first: Instant,
    last: Instant,
    revision: u64,
}

impl PendingPosition {
    fn deadline(&self) -> Instant {
        (self.last + QUIET_PERIOD).min(self.first + MAX_WAIT)
    }
}

/// A value snapshot, with revisions so a late selection cannot clear a newer edit.
#[derive(Clone, Default)]
pub struct ReadingPositionPolicy {
    pending: HashMap<ContentHash, PendingPosition>,
    revision: u64,
}

impl ReadingPositionPolicy {
    pub fn changed(&mut self, hash: ContentHash, now: Instant) {
        self.revision = self.revision.checked_add(1).expect("reading-position revisions exhausted");
        self.pending
            .entry(hash)
            .and_modify(|pending| {
                pending.last = now;
                pending.revision = self.revision;
            })
            .or_insert(PendingPosition { first: now, last: now, revision: self.revision });
    }

    pub fn waiting(&self, now: Instant) -> HashSet<ContentHash> {
        self.pending.iter().filter_map(|(hash, pending)| (now < pending.deadline()).then_some(*hash)).collect()
    }

    pub fn revisions(&self, hashes: impl IntoIterator<Item = ContentHash>) -> Vec<(ContentHash, u64)> {
        hashes.into_iter().filter_map(|hash| self.pending.get(&hash).map(|pending| (hash, pending.revision))).collect()
    }

    pub fn selected(&mut self, positions: &[(ContentHash, u64)]) {
        for (hash, revision) in positions {
            if self.pending.get(hash).is_some_and(|pending| pending.revision == *revision) {
                self.pending.remove(hash);
            }
        }
    }

    pub fn no_longer_queued(&self, queued: &HashSet<ContentHash>) -> Vec<(ContentHash, u64)> {
        self.revisions(self.pending.keys().filter(|hash| !queued.contains(hash)).copied())
    }

    /// Due work waits for normal sync or its upload dependency, avoiding a spin.
    pub fn delay(&self, now: Instant, queued: &HashSet<ContentHash>) -> Option<Duration> {
        self.pending.iter().filter(|(hash, _)| queued.contains(hash)).map(|(_, pending)| pending.deadline()).filter(|deadline| *deadline > now).min().map(|deadline| deadline.duration_since(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: u8) -> ContentHash {
        ContentHash::from_bytes([value; 32])
    }

    #[test]
    fn page_turns_extend_the_quiet_period_independently_for_each_book() {
        let now = Instant::now();
        let mut policy = ReadingPositionPolicy::default();
        policy.changed(hash(7), now);
        policy.changed(hash(8), now);
        policy.changed(hash(7), now + Duration::from_secs(4));
        assert_eq!(policy.waiting(now + Duration::from_secs(5)), HashSet::from([hash(7)]));
        assert!(policy.waiting(now + Duration::from_millis(8999)).contains(&hash(7)));
        assert!(policy.waiting(now + Duration::from_secs(9)).is_empty());
    }

    #[test]
    fn continuous_flipping_cannot_postpone_publication_past_thirty_seconds() {
        let now = Instant::now();
        let mut policy = ReadingPositionPolicy::default();
        for second in 0..30 {
            let at = now + Duration::from_secs(second);
            policy.changed(hash(7), at);
            assert!(policy.waiting(at).contains(&hash(7)));
        }
        assert!(policy.waiting(now + MAX_WAIT).is_empty());
        assert_eq!(policy.delay(now + MAX_WAIT, &HashSet::from([hash(7)])), None);
    }

    #[test]
    fn selection_starts_a_fresh_window_and_late_completion_preserves_newer_edits() {
        let now = Instant::now();
        let mut policy = ReadingPositionPolicy::default();
        policy.changed(hash(7), now);
        let first = policy.revisions([hash(7)]);
        policy.selected(&first);
        policy.changed(hash(7), now + MAX_WAIT);
        policy.selected(&first);
        assert!(policy.waiting(now + MAX_WAIT).contains(&hash(7)));
        assert!(!policy.waiting(now + MAX_WAIT + QUIET_PERIOD).contains(&hash(7)));
    }

    #[test]
    fn retained_snapshots_do_not_change_with_the_owner() {
        let now = Instant::now();
        let mut policy = ReadingPositionPolicy::default();
        policy.changed(hash(7), now);
        let snapshot = policy.clone();
        policy.changed(hash(7), now + Duration::from_secs(4));
        assert!(snapshot.waiting(now + QUIET_PERIOD).is_empty());
        assert!(policy.waiting(now + QUIET_PERIOD).contains(&hash(7)));
        assert_eq!(policy.delay(now, &HashSet::new()), None);
        policy.selected(&policy.no_longer_queued(&HashSet::new()));
        assert!(policy.revisions([hash(7)]).is_empty());
    }
}
