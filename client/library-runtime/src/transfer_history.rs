//! Completed-transfer persistence owned by one library session.

use crate::database::Database;
use asset_transfer::TransferQueue;

#[cfg(not(target_arch = "wasm32"))]
pub fn queue_with_history(_database: &Database, path: std::path::PathBuf) -> TransferQueue {
    TransferQueue::with_history(asset_transfer::MessagePackTransferHistory::new(path))
}

#[cfg(target_arch = "wasm32")]
pub fn queue_with_history(database: &Database, _path: std::path::PathBuf) -> TransferQueue {
    sqlite::open(database.clone())
}

#[cfg(target_arch = "wasm32")]
mod sqlite {
    use super::*;
    use asset_transfer::{HistoryPersistence, TransferStatus};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    const KEY: &str = "local_transfer_history_v1";

    struct History {
        database: Database,
        error: Mutex<Option<String>>,
    }

    pub(super) fn open(database: Database) -> TransferQueue {
        let entries = (|| -> Result<VecDeque<TransferStatus>, String> {
            let conn = database.get().map_err(|error| error.to_string())?;
            let saved = client_library::sync_state::get_sync_metadata(&conn, KEY).map_err(|error| error.to_string())?;
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
                let conn = self.database.get().map_err(|error| error.to_string())?;
                client_library::sync_state::set_sync_metadata(&conn, KEY, &json).map_err(|error| error.to_string())
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
