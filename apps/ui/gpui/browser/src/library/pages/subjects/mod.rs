//! Unified subject browsing: populated child subjects above, exact books below.

use gpui::{Context, EventEmitter, IntoElement, Render, Subscription, Window};
use ui_components as components;

use super::hierarchy::HierarchyState;
use crate::library::services::{LatestRequest, LibraryContext};
use crate::model::SubjectCrumb;

#[derive(Clone)]
pub(crate) enum SubjectsPageEvent {
    PathChanged(Vec<SubjectCrumb>),
}

pub(crate) struct SubjectsPage {
    ui: LibraryContext,
    hierarchy: HierarchyState<SubjectCrumb>,
    refresh_request: LatestRequest,
    search_request: LatestRequest,
    _filter_subscription: Subscription,
}

impl EventEmitter<SubjectsPageEvent> for SubjectsPage {}

impl SubjectsPage {
    pub(crate) fn new(ui: LibraryContext, path: Vec<SubjectCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (hierarchy, filter_subscription) = HierarchyState::new(ui.clone(), path, |page: &mut Self, cx| page.search(cx), window, cx);
        Self { hierarchy, ui, refresh_request: LatestRequest::default(), search_request: LatestRequest::default(), _filter_subscription: filter_subscription }
    }

    fn selected_path(&self) -> Option<&str> {
        self.hierarchy.last().map(|subject| subject.id.as_str())
    }

    fn set_path(&mut self, path: Vec<SubjectCrumb>, window: &mut Window, cx: &mut Context<Self>) {
        if self.hierarchy.set_path(path, window, cx) {
            cx.emit(SubjectsPageEvent::PathChanged(self.hierarchy.path().to_vec()));
            self.refresh(cx);
        }
    }

    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        self.hierarchy.set_library_view(library_view, cx);
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let subject_path = self.selected_path().map(str::to_owned);
        let generation = self.refresh_request.begin();
        let load = self.ui.load_subjects(subject_path.clone());
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh_request.is_current(generation) || page.selected_path() != subject_path.as_deref() {
                    return;
                }
                match result {
                    Ok(data) => {
                        page.hierarchy.clear_error();
                        page.hierarchy.install(data.children, data.books, cx);
                        cx.notify();
                    }
                    Err(error) => {
                        log::error!("failed to load Subjects hierarchy: {error}");
                        page.hierarchy.set_error(error, cx);
                    }
                }
            });
        });
        self.refresh_request.install(task);
    }

    /// Searches this subject and every narrower subject beneath it. At the root
    /// the scope is the whole library, which is what the Search page used to be.
    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.hierarchy.query(cx);
        if query.is_empty() {
            self.hierarchy.clear_search();
            return;
        }
        let generation = self.hierarchy.begin_search();
        let subject_path = self.selected_path().map(str::to_owned);
        let ui = self.ui.clone();
        let debounce = std::time::Duration::from_millis(u64::from(ui.contracts().browsing_content.search_debounce_ms));
        let timer = cx.background_executor().timer(debounce);
        let task = cx.spawn(async move |page, cx| {
            timer.await;
            let result = ui.search_subject(subject_path, query).await;
            let _ = page.update(cx, |page, cx| {
                if !page.hierarchy.is_current_search(generation) {
                    return;
                }
                match result {
                    Ok(found) => {
                        page.hierarchy.clear_error();
                        page.hierarchy.set_results(crate::library::pages::hierarchy::merge_matches(found), &page.ui.clone(), cx);
                    }
                    Err(error) => {
                        log::error!("failed to search this subject: {error}");
                        page.hierarchy.set_error(error, cx);
                    }
                }
            });
        });
        self.search_request.install(task);
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.hierarchy.focus_search(window, cx);
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let (title, description) = if self.hierarchy.path().is_empty() {
            ("No categorized books", "Books with no supported subject assignment will appear under Uncategorized.")
        } else {
            ("No books assigned directly", "This subject currently has no direct book assignments.")
        };
        components::browser_actionable_empty(title, description, components::browser_topbar_right(), theme).into_any_element()
    }
}

impl Render for SubjectsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.render_empty(cx);
        let hierarchy = self.hierarchy.render(
            "Subjects",
            "subject-button",
            "subject-books",
            self.ui.contracts().browsing_states.retry_label.clone().into(),
            self.ui.contracts().browsing_states.searching_label.clone().into(),
            components::browser_topbar_right(),
            empty,
            cx.entity(),
            |page, path, window, cx| page.set_path(path, window, cx),
            |page, cx| page.refresh(cx),
            None,
            None,
            None,
            window,
            cx,
        );
        components::hierarchy_page_host(hierarchy)
    }
}
