//! Recover explicit toolbar navigation when WebKit's page process is hung.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use webkit2gtk::{BackForwardListExt, BackForwardListItem, LoadEvent, NetworkError, WebProcessTerminationReason, WebView, WebViewExt};

#[derive(Clone)]
enum Navigation {
    Url(String),
    History(BackForwardListItem),
    Reload,
}

impl Navigation {
    fn apply(&self, page: &WebView) {
        match self {
            Self::Url(url) => page.load_uri(url),
            Self::History(item) => page.go_to_back_forward_list_item(item),
            Self::Reload => page.reload(),
        }
    }
}

pub(super) struct NativeNavigation {
    pending: Rc<RefCell<Option<Navigation>>>,
    restarting: Rc<Cell<bool>>,
}

impl NativeNavigation {
    pub(super) fn attach(page: &WebView) -> Self {
        let pending = Rc::new(RefCell::new(None::<Navigation>));
        let restarting = Rc::new(Cell::new(false));
        page.connect_load_changed({
            let pending = pending.clone();
            move |_, event| {
                // A committed document means the page process handled the
                // request. Loading its network resources can still take time.
                if event == LoadEvent::Committed {
                    pending.borrow_mut().take();
                }
            }
        });
        page.connect_load_failed({
            let pending = pending.clone();
            let restarting = restarting.clone();
            move |_, _, _, error| {
                // Starting a newer request can synchronously cancel the old
                // load. Keep the newer intent through that cancellation.
                if !restarting.get() && !error.matches(NetworkError::Cancelled) {
                    pending.borrow_mut().take();
                }
                false
            }
        });
        page.connect_web_process_terminated({
            let pending = pending.clone();
            let restarting = restarting.clone();
            move |page, reason| {
                if restarting.replace(false) && reason == WebProcessTerminationReason::TerminatedByApi {
                    let navigation = pending.borrow().clone();
                    if let Some(navigation) = navigation {
                        navigation.apply(page);
                    }
                } else {
                    pending.borrow_mut().take();
                }
            }
        });
        Self { pending, restarting }
    }

    pub(super) fn load(&self, page: &WebView, url: &str) {
        self.start(page, Navigation::Url(url.into()));
    }
    pub(super) fn reload(&self, page: &WebView) {
        self.start(page, Navigation::Reload);
    }

    pub(super) fn history(&self, page: &WebView, forward: bool) {
        if let Some(item) = page.back_forward_list().and_then(|list| if forward { list.forward_item() } else { list.back_item() }) {
            self.start(page, Navigation::History(item));
        }
    }

    fn start(&self, page: &WebView, navigation: Navigation) {
        *self.pending.borrow_mut() = Some(navigation.clone());
        if self.restarting.get() {
            return;
        }
        if page.is_web_process_responsive() {
            navigation.apply(page);
        }
        self.tick(page);
    }

    pub(super) fn pending(&self) -> bool {
        self.pending.borrow().is_some()
    }

    pub(super) fn tick(&self, page: &WebView) {
        // Only recover a user's navigation request, and only after WebKit
        // detects a hung process. Slow network loads do not trigger a restart.
        if self.pending() && !page.is_web_process_responsive() && !self.restarting.replace(true) {
            page.terminate_web_process();
        }
    }
}
