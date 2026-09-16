//! Compatibility re-export for library-owned transfer-history persistence.

pub(crate) use library_runtime::transfer_history::*;

// Browser storage-contract orchestration is host-facing; the durable history
// implementation itself lives in library-runtime.
#[cfg(all(target_arch = "wasm32", feature = "storage-contract-tests"))]
pub(crate) fn contract(phase: &str) -> Result<(), String> {
    use asset_transfer::{TransferJob, TransferOrigin, TransferState};

    let database = crate::storage::database::Database::open_library("transfer-history-contract", |conn| client_library::schema::init_db(conn).map(|_| ())).map_err(|error| error.to_string())?;
    let queue = queue_with_history(&database, std::path::PathBuf::new());
    match phase {
        "prepare" => {
            assert!(queue.history().is_empty());
            for value in 0..60 {
                queue.reconcile(vec![TransferJob::upload_blob(crate::ContentHash::new(&format!("{value:064x}")), TransferOrigin::Background)]);
                let job = queue.claim_due().unwrap();
                queue.complete(job.id, None);
            }
            queue.flush_history().map_err(|error| error.to_string())?;
        }
        "verify" => {
            assert_eq!(queue.history().len(), 50);
            assert_eq!(queue.history()[0].content_hash, crate::ContentHash::new(&format!("{:064x}", 59)));
            assert!(queue.history().iter().all(|entry| entry.state == TransferState::Completed));
            let connection = database.get().map_err(|error| error.to_string())?;
            queue.reconcile(vec![TransferJob::upload_blob(crate::ContentHash::new(&format!("{:064x}", 60)), TransferOrigin::Background)]);
            let job = queue.claim_due().unwrap();
            queue.complete(job.id, None);
            assert!(queue.flush_history().is_err());
            drop(connection);
            queue.flush_history().map_err(|error| error.to_string())?;
            assert_eq!(queue_with_history(&database, std::path::PathBuf::new()).history()[0].content_hash, crate::ContentHash::new(&format!("{:064x}", 60)));
        }
        _ => return Err(format!("unknown history contract phase: {phase}")),
    }
    Ok(())
}
