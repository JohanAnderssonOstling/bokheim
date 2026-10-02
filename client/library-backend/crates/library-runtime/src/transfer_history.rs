//! Completed-transfer persistence owned by one library session.

use std::path::PathBuf;
use crate::TransferQueue;
use library_database::Database;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
pub fn queue_with_history(_database: &Arc<Database>, path: PathBuf) -> TransferQueue {
    {
        use crate::transfer::queue::*;
        let history: MessagePackTransferHistory = crate::MessagePackTransferHistory::new(path);

        let entries = history.load().unwrap_or_else(|error| {
            log::warn!("Could not load transfer history: {error}");
            VecDeque::new()
        });
        let (history, history_error) = match crate::transfer::history::writer::HistoryWriter::new(history) {
            Ok(writer) => (Some(Box::new(writer) as Box<dyn crate::transfer::HistoryPersistence>), None),
            Err(error) => {
                log::warn!("Could not start transfer history writer: {error}");
                (None, Some(error.to_string()))
            }
        };
        TransferQueue { state: Mutex::new(QueueState { history: entries, ..QueueState::default() }), history, history_error, ..TransferQueue::default() }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn queue_with_history(database: &Arc<Database>, _path: PathBuf) -> TransferQueue {
    sqlite::open(database.clone())
}

#[cfg(target_arch = "wasm32")]
mod sqlite {
    use super::*;
    use crate::HistoryPersistence;
    use library_model::TransferStatus;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct History {
        database: Arc<Database>,
        error: Mutex<Option<String>>,
    }

    // SAFETY: wasm32-unknown-unknown has no threads, so nothing can ever
    // access `database` (a single-threaded `rusqlite::Connection`) from more
    // than one thread concurrently. `HistoryPersistence: Send + Sync` exists
    // for the native multi-threaded transfer queue; on this target both
    // bounds are a compile-time-only formality with no real concurrency
    // to violate.
    unsafe impl Send for History {}
    unsafe impl Sync for History {}

    pub(super) fn open(database: Arc<Database>) -> TransferQueue {
        let entries = {
            'entries: {
                let saved = {
                    use library_database::OwnerSql;
                    let mut history = None;
                    (match database
                        .connection
                        .owner_transfer_history(|row| {
                            history = Some(row.get::<_, String>(0)?);
                            Ok(())
                        })
                        .map_err(|error| error.to_string())
                    {
                        Ok(value) => value,
                        Err(error) => break 'entries Err(error.into()),
                    });
                    history
                };
                saved.map(|json| serde_json::from_str(&json).map_err(|error| error.to_string())).unwrap_or_else(|| Ok(VecDeque::new()))
            }
        }
        .unwrap_or_else(|error| {
            log::warn!("Could not load transfer history: {error}");
            VecDeque::new()
        });
        {
            use crate::transfer::queue::*;
            let mut entries: VecDeque<TransferStatus> = entries;
            let persistence: _ = History { database, error: Mutex::new(None) };

            entries.truncate(HISTORY_LIMIT);
            TransferQueue { state: Mutex::new(QueueState { history: entries, ..QueueState::default() }), history: Some(Box::new(persistence)), ..TransferQueue::default() }
        }
    }

    impl HistoryPersistence for History {
        fn submit(&self, entries: VecDeque<TransferStatus>) {
            let result = {
                'submit: {
                    let json = (match serde_json::to_string(&entries).map_err(|error| error.to_string()) {
                        Ok(value) => value,
                        Err(error) => break 'submit Err(error.into()),
                    });
                    {
                        use library_database::OwnerSql;
                        self.database.connection.owner_save_transfer_history(&json).map(|_| ()).map_err(|error| error.to_string())
                    }
                }
            };
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
