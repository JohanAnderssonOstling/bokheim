//! Metadata synchronization and binary asset transfer.

pub(crate) mod asset_sync;
pub(crate) mod error;
pub(crate) mod library_sync;
pub(crate) mod recovery;
mod state_sync;
#[cfg(target_arch = "wasm32")]
mod web_coordinator;

pub use library_runtime::scheduler::{run_timed, BackgroundTiming, SyncRequest, SyncScheduler};
pub use library_sync::{LibrarySync, LibrarySyncConfig};
#[cfg(target_arch = "wasm32")]
pub use web_coordinator::run;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_coordinator::synchronize;

/// Orders a fixture page as the server does: lifecycles before fields. A book
/// whose field appears without its lifecycle gets a Present lifecycle at that
/// field's revision, as if the server had delivered it on this page.
#[cfg(test)]
pub(crate) fn fixture_lifecycles_first(changes: Vec<sync_common::StateDelivery>) -> Vec<sync_common::StateDelivery> {
    use library_replica::{BookLifecycleState, LibraryStateEntry};
    let (mut lifecycles, fields): (Vec<_>, Vec<_>) = changes.into_iter().partition(|change| change.state.value.address.kind == "book_lifecycle");
    let mut books = lifecycles.iter().map(|change| change.state.value.address.entity_key.clone()).collect::<std::collections::HashSet<_>>();
    for field in &fields {
        let Ok(Some(body)) = LibraryStateEntry::decode(&field.state) else { continue };
        let Some(hash) = body.upload_dependency() else { continue };
        if books.insert(hash.to_string()) {
            let lifecycle = LibraryStateEntry::BookLifecycle { content_hash: hash, value: BookLifecycleState::Present }.publication(
                sync_common::ReplicaId::nil(),
                0,
                sync_common::ReplicaSeq::new(1).unwrap(),
            );
            lifecycles.push(sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&lifecycle).unwrap(), uuid::Uuid::from_u128(1), field.revision));
        }
    }
    lifecycles.sort_by_key(|change| change.revision);
    lifecycles.extend(fields);
    lifecycles
}
