//! Completed-transfer persistence owned by one library session.

use library_database::Database;
use crate::TransferQueue;

#[cfg(not(target_arch = "wasm32"))]
pub fn queue_with_history(_database: &Database, path: std::path::PathBuf) -> TransferQueue {
    TransferQueue::with_history(crate::MessagePackTransferHistory::new(path))
}

#[cfg(target_arch = "wasm32")]
pub fn queue_with_history(database: &Database, _path: std::path::PathBuf) -> TransferQueue {
    sqlite::open(database.clone())
}

#[cfg(target_arch = "wasm32")]
mod sqlite {
    use super::*;
    use crate::{HistoryPersistence, TransferStatus};
    use std::collections::VecDeque;
    use std::sync::Mutex;


    struct History {
        database: Database,
        error: Mutex<Option<String>>,
    }

    pub(super) fn open(database: Database) -> TransferQueue {
        let entries = (|| -> Result<VecDeque<TransferStatus>, String> {
            let saved = database.load_transfer_history().map_err(|error| error.to_string())?;
            saved.map(|json| serde_json::from_str(&json).map_err(|error| error.to_string())).unwrap_or_else(|| Ok(VecDeque::new()))
        })()
        .unwrap_or_else(|error| {
            log::warn!("Could not load transfer history: {error}");
            VecDeque::new()
        });
        TransferQueue::with_persistence(entries, History { database, error: Mutex::new(None) })
    }

    impl HistoryPersistence for History {
        fn submit(&self, entries: VecDeque<TransferStatus>) {
            let result = (|| -> Result<(), String> {
                let json = serde_json::to_string(&entries).map_err(|error| error.to_string())?;
                self.database.save_transfer_history(&json).map_err(|error| error.to_string())
            })();
            if let Err(error) = &result {
                log::warn!("Could not persist transfer history: {error}");
            }
            *self.error.lock().unwrap_or_else(|error| error.into_inner()) = result.err();
        }

        fn flush(&self) -> std::io::Result<()> {
            match self.error.lock().unwrap_or_else(|error| error.into_inner()).as_ref() {
                Some(error) => Err(std::io::Error::other(error.clone())),
                None => Ok(()),
            }
        }
    }
}
