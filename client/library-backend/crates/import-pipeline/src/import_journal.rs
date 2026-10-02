//! Replay the durable browser-import journal. Only newline-terminated records
//! are committed; a worker may have stopped midway through the final write.

use crate::ImportFailure;
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use std::collections::BTreeMap;
pub(crate) use sync_common::{ContentHash, DirId};

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Checkpoint {
    Copied {
        index: usize,
        hash: String,
    },
    Added {
        index: usize,
        error: Option<String>,
    },
    Complete {
        failures: Vec<ImportFailure>,
    },
    #[serde(other)]
    Unknown,
}

pub struct Journal {
    pub offset: usize,
    pub(crate) copied: BTreeMap<usize, String>,
    pub(crate) added: BTreeMap<usize, Option<String>>,
    pub completed: Option<Vec<ImportFailure>>,
    pub(crate) sizes: Option<Vec<u64>>,
    pub(crate) total_bytes: u64,
    pub(crate) copied_bytes: u64,
    pub(crate) failed: usize,
    pub(crate) directories: crate::import_workflow::Destinations,
    pub(crate) reporter: crate::import_workflow::ProgressReporter,
}

#[derive(Clone, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub total_bytes: u64,
    pub copied_bytes: u64,
    pub total_files: usize,
    pub copied_files: usize,
    pub processed: usize,
    pub failed: usize,
}

impl Progress {
    pub(crate) fn with_in_flight(mut self, bytes: u64) -> Self {
        self.copied_bytes = self.copied_bytes.saturating_add(bytes).min(self.total_bytes);
        self
    }
}

pub fn replay(bytes: &[u8]) -> Result<Journal, serde_json::Error> {
    let offset = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |position| position + 1);
    let mut state = Journal { offset, copied: BTreeMap::new(), added: BTreeMap::new(), completed: None, sizes: None, total_bytes: 0, copied_bytes: 0, failed: 0, directories: Default::default(), reporter: Default::default() };
    for line in bytes[..offset].split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
        state.apply(serde_json::from_slice(line)?);
    }
    Ok(state)
}

impl Journal {
    // Reports belong to one live import attempt, not the durable journal. A
    // replacement worker reports recovered results to its new import activity.

    pub(crate) fn destination(&self, path: &[String]) -> Result<(DirId, String), String> {
        self.directories.resolve(path).map(|(parent, name)| (parent, name.to_owned()))
    }

    pub fn progress(&self, in_flight_bytes: u64) -> Progress {
        Progress { total_bytes: self.total_bytes, copied_bytes: self.copied_bytes, total_files: self.sizes.as_ref().map_or(0, Vec::len), copied_files: { self.copied.len() }, processed: self.added_count(), failed: { self.failed } }
            .with_in_flight(in_flight_bytes)
    }
    pub(crate) fn apply(&mut self, checkpoint: Checkpoint) {
        match checkpoint {
            Checkpoint::Copied { index, hash } => {
                if self.copied.insert(index, hash).is_none() {
                    self.copied_bytes += self.sizes.as_ref().map_or(0, |sizes| sizes[index]);
                }
            }
            Checkpoint::Added { index, error } => {
                let failed = |error: &Option<String>| error.as_deref().is_some_and(|error| !error.is_empty());
                self.failed += usize::from(failed(&error));
                if self.added.insert(index, error).is_some_and(|previous| failed(&previous)) {
                    self.failed -= 1;
                }
            }
            Checkpoint::Complete { failures } => {
                self.completed.get_or_insert(failures);
            }
            Checkpoint::Unknown => {}
        }
    }

    // The caller must flush these bytes before advancing the import. On a
    // storage error it drops this state and replays the durable journal later.
    pub(crate) fn record(&mut self, checkpoint: Checkpoint) -> Result<Vec<u8>, serde_json::Error> {
        if let Checkpoint::Copied { index, .. } | Checkpoint::Added { index, .. } = &checkpoint {
            if self.sizes.as_ref().is_some_and(|sizes| *index >= sizes.len()) {
                return Err(serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, "import checkpoint refers to an unknown file")));
            }
        }
        let mut bytes = serde_json::to_vec(&checkpoint)?;
        bytes.push(b'\n');
        self.offset += bytes.len();
        self.apply(checkpoint);
        Ok(bytes)
    }

    pub fn copied_hash(&self, index: usize) -> Option<&str> {
        self.copied.get(&index).map(String::as_str)
    }

    pub fn added_count(&self) -> usize {
        self.added.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_common::ROOT_DIR_ID;

    #[test]
    fn progress_batches_report_totals_once_and_restore_results_on_replay() {
        let mut state = replay(&[]).unwrap().with_files(vec![1, 2, 3]).unwrap();
        let counts = |command| match command {
            Some(crate::staged_import::AdvanceProgress { total, succeeded, failed, .. }) => (total, succeeded, failed),
            _ => panic!("expected progress command"),
        };
        assert_eq!(counts(state.advance_command(uuid::Uuid::nil()).unwrap()), (3, 0, 0));
        assert!(state.advance_command(uuid::Uuid::nil()).unwrap().is_none());
        let mut bytes = state.record_added(0, None).unwrap();
        bytes.extend(state.record_added(1, Some("invalid book".into())).unwrap());
        assert_eq!(counts(state.advance_command(uuid::Uuid::nil()).unwrap()), (0, 1, 1));
        assert!(state.advance_command(uuid::Uuid::nil()).unwrap().is_none());
        let mut recovered = replay(&bytes).unwrap().with_files(vec![1, 2, 3]).unwrap();
        assert_eq!(counts(recovered.advance_command(uuid::Uuid::nil()).unwrap()), (3, 1, 1));
        recovered.record_added(2, None).unwrap();
        assert_eq!(counts(recovered.advance_command(uuid::Uuid::nil()).unwrap()), (0, 1, 0));
        assert!(recovered.advance_command(uuid::Uuid::nil()).unwrap().is_none());
    }

    #[test]
    fn prepared_destinations_match_complete_parent_paths_and_replace_old_plans() {
        let mut state = replay(&[]).unwrap();
        let root = ROOT_DIR_ID;
        let shelf = DirId::new_v4();
        state.prepare_directories(vec![(Vec::new(), root), (vec!["Shelf".into(), "Nested".into()], shelf)]);
        assert_eq!(state.destination(&["root.epub".into()]).unwrap(), (root, "root.epub".into()));
        assert_eq!(state.destination(&["Shelf".into(), "Nested".into(), "book.epub".into()]).unwrap(), (shelf, "book.epub".into()));
        assert!(state.destination(&["Nested".into(), "book.epub".into()]).is_err());
        assert!(state.destination(&[]).is_err());
        state.prepare_directories(vec![(Vec::new(), root)]);
        assert!(state.destination(&["Shelf".into(), "Nested".into(), "book.epub".into()]).is_err());
    }

    #[test]
    fn progress_counts_durable_copies_once_and_keeps_partial_bytes_ephemeral() {
        let mut state = replay(&[]).unwrap().with_files(vec![3, 11, 0]).unwrap();
        let mut bytes = state.record_copied(0, "first".into()).unwrap();
        assert_eq!(state.progress(5).copied_bytes, 8);
        assert_eq!(state.progress(0).copied_bytes, 3);
        bytes.extend(state.record_copied(0, "first".into()).unwrap());
        bytes.extend(state.record_copied(2, "empty".into()).unwrap());
        bytes.extend(state.record_added(0, Some("invalid EPUB".into())).unwrap());
        let expected = Progress { total_bytes: 14, copied_bytes: 3, total_files: 3, copied_files: 2, processed: 1, failed: 1 };
        assert_eq!(state.progress(0), expected);
        assert_eq!(replay(&bytes).unwrap().with_files(vec![3, 11, 0]).unwrap().progress(0), expected);
        assert!(replay(&bytes).unwrap().with_files(vec![3]).is_err());
        let before = state.offset;
        assert!(state.record_copied(3, "out of bounds".into()).is_err());
        assert_eq!(state.offset, before);
        assert_eq!(state.progress(0), expected);
        assert!(replay(&[]).unwrap().with_files(vec![u64::MAX, 1]).is_err());
    }

    #[test]
    fn legacy_failure_strings_and_typed_failures_survive_replay() {
        let old = replay(b"{\"kind\":\"complete\",\"failures\":[\"old UI wording\"]}\n").unwrap();
        assert_eq!(old.completed.as_ref().unwrap()[0].kind, crate::ImportFailureKind::Legacy);
        let mut next = replay(&[]).unwrap();
        let bytes = next.record_complete(old.completed.unwrap()).unwrap();
        let restored = replay(&bytes).unwrap();
        assert_eq!(restored.completed, next.completed);
    }

    #[test]
    fn replay_keeps_latest_file_checkpoints_and_first_completion() {
        let result = replay(b"{\"kind\":\"copied\",\"index\":2,\"hash\":\"old\"}\n{\"kind\":\"copied\",\"index\":2,\"hash\":\"new\"}\n{\"kind\":\"added\",\"index\":2,\"error\":\"bad EPUB\"}\n{\"kind\":\"added\",\"index\":2}\n{\"kind\":\"complete\",\"failures\":[]}\n{\"kind\":\"complete\",\"failures\":[\"later\"]}\n").unwrap();
        assert_eq!(result.copied.len(), 1);
        assert_eq!(result.copied[&2], "new");
        assert_eq!(result.added.len(), 1);
        assert!(result.added[&2].is_none());
        assert_eq!(result.completed, Some(Vec::new()));
    }

    #[test]
    fn torn_utf8_tail_is_discarded_at_the_original_byte_offset() {
        let committed = "{\"kind\":\"added\",\"index\":0,\"error\":\"fel på bok\"}\n";
        let mut bytes = committed.as_bytes().to_vec();
        bytes.extend_from_slice(b"{\"kind\":\"complete\",\"failures\":[\"\xc3");
        let result = replay(&bytes).unwrap();
        assert_eq!(result.offset, committed.len());
        assert!(result.completed.is_none());
        assert_eq!(result.added[&0].as_deref(), Some("fel på bok"));
        assert_eq!(replay(b"unfinished").unwrap().offset, 0);
        assert!(replay(b"invalid committed record\n").is_err());
    }
}
