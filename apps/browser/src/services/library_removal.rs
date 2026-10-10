//! Optional application-owned preparation before removing a configured library.
use gpui::{App, Global};
use std::{cell::RefCell, collections::HashSet, future::Future, pin::Pin, rc::Rc};
use sync_common::LibraryId;

pub type LibraryRemovalPreparation = Pin<Box<dyn Future<Output = Result<(), String>>>>;

/// Prepares application state before the browser deletes a library.
pub trait LibraryRemovalService {
    fn prepare_removal(&self, library: LibraryId, cx: &mut App) -> LibraryRemovalPreparation;
}

pub struct LibraryRemovalHook(pub Rc<dyn LibraryRemovalService>);
impl Global for LibraryRemovalHook {}

#[derive(Default)]
struct RemovingLibraries(Rc<RefCell<HashSet<LibraryId>>>);
impl Global for RemovingLibraries {}

/// Held through preparation AND backend deletion, including cancellation.
pub(crate) struct LibraryRemovalGuard {
    library: LibraryId,
    removing: Rc<RefCell<HashSet<LibraryId>>>,
}

impl Drop for LibraryRemovalGuard {
    fn drop(&mut self) {
        self.removing.borrow_mut().remove(&self.library);
    }
}

pub fn library_is_being_removed(library: LibraryId, cx: &App) -> bool {
    cx.try_global::<RemovingLibraries>().is_some_and(|state| state.0.borrow().contains(&library))
}

pub(crate) fn begin_library_removal(library: LibraryId, cx: &mut App) -> Result<LibraryRemovalGuard, String> {
    if cx.try_global::<RemovingLibraries>().is_none() {
        cx.set_global(RemovingLibraries::default());
    }
    let removing = cx.global::<RemovingLibraries>().0.clone();
    if !removing.borrow_mut().insert(library) {
        return Err("Library removal is already in progress".into());
    }
    Ok(LibraryRemovalGuard { library, removing })
}

pub(crate) fn prepare_library_removal(library: LibraryId, cx: &mut App) -> LibraryRemovalPreparation {
    let hook = cx.try_global::<LibraryRemovalHook>().map(|hook| hook.0.clone());
    match hook {
        Some(hook) => hook.prepare_removal(library, cx),
        None => Box::pin(async { Ok(()) }),
    }
}

pub(crate) async fn prepared_removal<T>(preparation: LibraryRemovalPreparation, remove: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    preparation.await?;
    remove.await
}
