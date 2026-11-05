//! One native watcher serves all open library roots. Each registration owns
//! only its routing entry and unregisters when its library worker stops.

use library_services::scheduler::SyncScheduler;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

static WATCHER: Mutex<Option<SharedWatcher>> = Mutex::new(None);

struct Subscription {
    root: PathBuf,
    scan: SyncScheduler,
    active: Arc<AtomicBool>,
}

struct SharedWatcher {
    watcher: RecommendedWatcher,
    subscriptions: Arc<Mutex<BTreeMap<u64, Subscription>>>,
    next_id: u64,
}

pub(crate) struct WatchSubscription {
    id: u64,
    active: Arc<AtomicBool>,
}

impl WatchSubscription {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

impl SharedWatcher {
    fn new() -> notify::Result<Self> {
        let subscriptions = Arc::new(Mutex::new(BTreeMap::<u64, Subscription>::new()));
        let observed = subscriptions.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if event.as_ref().is_ok_and(|event| event.kind.is_access()) {
                return;
            }
            if let Err(error) = &event {
                log::warn!("library filesystem watcher error: {error}");
            }
            let subscribers = observed.lock().unwrap_or_else(PoisonError::into_inner);
            for subscription in subscribers.values() {
                if event.is_err()
                    || event.as_ref().is_ok_and(|event| matches!(event.kind, notify::EventKind::Remove(_) | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))) && event.paths.iter().any(|path| path == &subscription.root))
                {
                    subscription.active.store(false, Ordering::Release);
                }
                let affected = match &event {
                    Err(_) => true,
                    Ok(event) if event.need_rescan() || event.paths.is_empty() => true,
                    Ok(event) => event.paths.iter().any(|path| path.strip_prefix(&subscription.root).is_ok_and(|relative| !relative.components().any(|component| component.as_os_str().to_string_lossy().starts_with('.')))),
                };
                if affected {
                    subscription.scan.wake();
                }
            }
        })?;
        Ok(Self { watcher, subscriptions, next_id: 0 })
    }

    fn restore_roots(&mut self, removed: &Path) {
        let roots: BTreeSet<_> = self
            .subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|subscription| subscription.root.starts_with(removed) || removed.starts_with(&subscription.root))
            .map(|subscription| subscription.root.clone())
            .collect();
        for root in roots {
            let result = self.watcher.watch(&root, RecursiveMode::Recursive);
            if let Err(error) = &result {
                log::warn!("could not restore library watch {}: {error}", root.display());
            }
            for subscription in self.subscriptions.lock().unwrap_or_else(PoisonError::into_inner).values() {
                if subscription.root == root {
                    subscription.active.store(result.is_ok(), Ordering::Release);
                    subscription.scan.wake();
                }
            }
        }
    }
}

pub(crate) fn watch(root: &Path, scan: SyncScheduler) -> notify::Result<WatchSubscription> {
    let root = std::fs::canonicalize(root).map_err(notify::Error::io)?;
    let mut shared = WATCHER.lock().unwrap_or_else(PoisonError::into_inner);
    if shared.is_none() {
        *shared = Some(SharedWatcher::new()?);
    }
    let state = shared.as_mut().expect("watcher initialized");
    let already_watched = state.subscriptions.lock().unwrap_or_else(PoisonError::into_inner).values().any(|subscription| subscription.root == root && subscription.active.load(Ordering::Acquire));
    if !already_watched {
        // The callback never takes WATCHER, and the routing lock must not be
        // held while notify synchronously registers a root on its own thread.
        if let Err(error) = state.watcher.watch(&root, RecursiveMode::Recursive) {
            // Recursive registration can fail after adding some descendants.
            // Remove that partial registration and repair overlapping roots.
            let _ = state.watcher.unwatch(&root);
            state.restore_roots(&root);
            let empty = state.subscriptions.lock().unwrap_or_else(PoisonError::into_inner).is_empty();
            if empty {
                *shared = None;
            }
            return Err(error);
        }
    }
    let id = state.next_id;
    state.next_id += 1;
    let active = Arc::new(AtomicBool::new(true));
    state.subscriptions.lock().unwrap_or_else(PoisonError::into_inner).insert(id, Subscription { root, scan, active: active.clone() });
    Ok(WatchSubscription { id, active })
}

impl Drop for WatchSubscription {
    fn drop(&mut self) {
        let mut shared = WATCHER.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(state) = shared.as_mut() else { return };
        let (removed, roots) = {
            let mut subscriptions = state.subscriptions.lock().unwrap_or_else(PoisonError::into_inner);
            let removed = subscriptions.remove(&self.id);
            let roots: BTreeSet<_> = subscriptions.values().map(|subscription| subscription.root.clone()).collect();
            (removed, roots)
        };
        if roots.is_empty() {
            *shared = None;
            return;
        }
        let Some(removed) = removed else { return };
        if !roots.contains(&removed.root) {
            let _ = state.watcher.unwatch(&removed.root);
            // Roots can overlap. Removing a parent must not leave an active
            // nested library without watches on its descendants.
            state.restore_roots(&removed.root);
        }
    }
}
