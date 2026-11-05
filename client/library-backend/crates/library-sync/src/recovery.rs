//! Paged cursor recovery shared by native and browser storage owners.
use library_database::{DatabaseClient, DatabaseError};

pub(crate) async fn complete(database: &DatabaseClient) -> Result<usize, DatabaseError> {
    let mut after = None;
    let mut inserted = 0;
    loop {
        let (next, count) = database.sync_recover_state_page(after.as_ref()).await?;
        inserted += count;
        if next.is_none() {
            return Ok(inserted);
        }
        after = next;
        // Each page has committed before yielding so the browser event loop
        // stays responsive through long recoveries.
        tokio::task::yield_now().await;
        #[cfg(target_arch = "wasm32")]
        client_platform_runtime::executor::sleep(std::time::Duration::from_millis(1)).await.map_err(DatabaseError::operation)?;
    }
}
