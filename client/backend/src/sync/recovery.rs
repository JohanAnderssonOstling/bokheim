//! Recovery sequencing shared by native storage and browser storage commands.
use sync_common::StateCell;

pub(crate) async fn complete<E, F: std::future::Future<Output = Result<(Option<StateCell>, usize), E>>>(mut page: impl FnMut(Option<StateCell>) -> F) -> Result<usize, E> {
    let mut after = None;
    let mut inserted = 0;
    loop {
        let (next, count) = page(after).await?;
        inserted += count;
        if next.is_none() {
            return Ok(inserted);
        }
        after = next;
        // Each page has committed and released storage before yielding.
        crate::executor::sleep(std::time::Duration::from_millis(1)).await;
    }
}
