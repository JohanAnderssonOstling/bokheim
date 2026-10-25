//! Optional application-owned preparation before removing a configured library.
use gpui::{App, Global};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
};
use sync_common::LibraryId;

pub type LibraryRemovalPreparation = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// Prepares application state before the browser deletes a library.
pub trait LibraryRemovalService {
    fn prepare_removal(&self, library: LibraryId, cx: &mut App) -> LibraryRemovalPreparation;
}

pub struct LibraryRemovalHook(pub Rc<dyn LibraryRemovalService>);
impl Global for LibraryRemovalHook {}

#[derive(Default)]
struct RemovingLibraries(HashMap<LibraryId, Weak<RemovalLease>>);

struct RemovalLease;

impl RemovingLibraries {
    fn contains(&self, library: LibraryId) -> bool {
        self.0.get(&library).is_some_and(|lease| lease.strong_count() > 0)
    }

    fn begin(&mut self, library: LibraryId) -> Result<LibraryRemovalGuard, String> {
        self.0.retain(|_, lease| lease.strong_count() > 0);
        if self.contains(library) {
            return Err("Library removal is already in progress".into());
        }
        let lease = Rc::new(RemovalLease);
        self.0.insert(library, Rc::downgrade(&lease));
        Ok(LibraryRemovalGuard { _lease: lease })
    }
}
impl Global for RemovingLibraries {}

/// Held through preparation AND backend deletion, including cancellation.
pub(crate) struct LibraryRemovalGuard {
    // The task owns activity. Dropping it expires the registry's weak reference,
    // including cancellation, without borrowing the app or mutating shared state.
    _lease: Rc<RemovalLease>,
}

pub fn library_is_being_removed(library: LibraryId, cx: &App) -> bool {
    cx.try_global::<RemovingLibraries>().is_some_and(|state| state.contains(library))
}

pub(crate) fn begin_library_removal(library: LibraryId, cx: &mut App) -> Result<LibraryRemovalGuard, String> {
    if cx.try_global::<RemovingLibraries>().is_none() {
        cx.set_global(RemovingLibraries::default());
    }
    cx.global_mut::<RemovingLibraries>().begin(library)
}

pub(crate) fn prepare_library_removal(library: LibraryId, cx: &mut App) -> LibraryRemovalPreparation {
    let hook = cx.try_global::<LibraryRemovalHook>().map(|hook| hook.0.clone());
    match hook {
        Some(hook) => hook.prepare_removal(library, cx),
        None => Box::pin(async { Ok(()) }),
    }
}

pub(crate) async fn prepared_removal<T>(preparation: LibraryRemovalPreparation, remove: impl Future<Output = Result<T, app::BackendError>>) -> Result<T, app::BackendError> {
    // Preparation fails only for application state, such as playback saves.
    preparation.await.map_err(app::BackendError::message)?;
    remove.await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_blocks_duplicates_until_the_task_drops_it() {
        let mut registry = RemovingLibraries::default();
        let library = LibraryId::from_u128(1);
        let guard = registry.begin(library).unwrap();
        assert!(registry.contains(library));
        assert!(registry.begin(library).is_err());
        drop(guard);
        assert!(!registry.contains(library));
        assert!(registry.begin(library).is_ok());
    }

    #[test]
    fn cancelling_a_pending_task_expires_only_its_lease() {
        use futures_util::FutureExt;
        let mut registry = RemovingLibraries::default();
        let first = LibraryId::from_u128(1);
        let second = LibraryId::from_u128(2);
        let guard = registry.begin(first).unwrap();
        let other = registry.begin(second).unwrap();
        let task = async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        };
        assert!(task.now_or_never().is_none());
        assert!(!registry.contains(first));
        assert!(registry.contains(second));
        let replacement = registry.begin(first).unwrap();
        assert_eq!(registry.0.len(), 2);
        drop((other, replacement));
        assert!(!registry.contains(first));
        assert!(!registry.contains(second));
    }
}
