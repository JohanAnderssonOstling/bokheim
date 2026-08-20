//! Unified subject browsing: populated child subjects above, exact books below.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, AppContext, Context, Entity, FontWeight, Image, ImageFormat, IntoElement, ObjectFit, ParentElement, Render, Styled, StyledImage, Subscription, Task, WeakEntity, Window, div, img, px, uniform_list};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{Disableable, Sizable};
use ui_components as components;

use super::hierarchy::{HierarchySplits, HierarchyState};
use crate::library::services::{LatestRequest, LibraryContext};
use crate::library::{DetailOwner, context_with_page_detail, detail::BookDetailPage, page_detail};
use crate::model::SubjectCrumb;

const UNCATEGORIZED_SUBJECT_PATH: &str = "__uncategorized__";

#[derive(Clone)]
enum ReviewCoverState {
    Pending,
    Loaded(Arc<Image>),
    Missing,
}

struct ReviewCoverView {
    content_hash: epub_svelte_lib::ContentHash,
    ui: LibraryContext,
    state: ReviewCoverState,
    task: Option<Task<()>>,
}

impl ReviewCoverView {
    fn create<P: 'static>(content_hash: epub_svelte_lib::ContentHash, ui: LibraryContext, cx: &mut Context<P>) -> Entity<Self> {
        cx.new(|_| Self { content_hash, ui, state: ReviewCoverState::Pending, task: None })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let load = self.ui.load_cover(self.content_hash.clone());
        self.task = Some(cx.spawn(async move |cover, cx| {
            let result = load.await;
            let _ = cover.update(cx, |cover, cx| {
                cover.task.take();
                cover.state = match result {
                    Ok(Some(bytes)) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => ReviewCoverState::Loaded(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes))),
                    Ok(_) => ReviewCoverState::Missing,
                    Err(error) => {
                        log::error!("failed to load ISBN review thumbnail: {error}");
                        ReviewCoverState::Missing
                    }
                };
                cx.notify();
            });
        }));
    }
}

impl Render for ReviewCoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.state, ReviewCoverState::Pending) && self.task.is_none() {
            self.load(cx);
        }
        let theme = components::browser_theme(cx);
        let frame = div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().overflow_hidden().bg(theme.surface_alt);
        match &self.state {
            ReviewCoverState::Loaded(image) => frame.child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)).into_any_element(),
            ReviewCoverState::Pending => frame.into_any_element(),
            ReviewCoverState::Missing => frame.child(components::missing_cover_mark("No cover", theme)).into_any_element(),
        }
    }
}

pub(crate) struct SubjectsPage {
    ui: LibraryContext,
    detail: Option<Entity<BookDetailPage>>,
    hierarchy: HierarchyState<SubjectCrumb>,
    refresh_request: LatestRequest,
    search_request: LatestRequest,
    taxonomy_reload_request: LatestRequest,
    taxonomy_reloading: bool,
    review_request: LatestRequest,
    reviews: Rc<Vec<epub_svelte_lib::library::EditionIdentityReview>>,
    review_status: epub_svelte_lib::library::EditionIdentityReviewStatus,
    review_loading: bool,
    review_error: Option<String>,
    review_busy: Option<epub_svelte_lib::ContentHash>,
    review_batch_busy: bool,
    review_classification_refresh_attempted: bool,
    candidate_selections: HashMap<epub_svelte_lib::ContentHash, usize>,
    manual_isbn_inputs: HashMap<epub_svelte_lib::ContentHash, gpui::Entity<InputState>>,
    review_covers: HashMap<epub_svelte_lib::ContentHash, Entity<ReviewCoverView>>,
    _filter_subscription: Subscription,
    splits: HierarchySplits<SubjectsPage>,
}

impl SubjectsPage {
    pub(crate) fn new(ui: LibraryContext, path: Vec<SubjectCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_pane(ui, path, None, 0, window, cx)
    }

    fn new_pane(ui: LibraryContext, path: Vec<SubjectCrumb>, split_root: Option<WeakEntity<SubjectsPage>>, pane_id: u64, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ui = context_with_page_detail(ui, cx);
        let (hierarchy, filter_subscription) = HierarchyState::new(ui.clone(), path, None, |page: &mut Self, cx| page.search(cx), window, cx);
        let mut page = Self {
            hierarchy,
            ui,
            detail: None,
            refresh_request: LatestRequest::default(),
            search_request: LatestRequest::default(),
            taxonomy_reload_request: LatestRequest::default(),
            taxonomy_reloading: false,
            review_request: LatestRequest::default(),
            reviews: Rc::new(Vec::new()),
            review_status: epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous,
            review_loading: false,
            review_error: None,
            review_busy: None,
            review_batch_busy: false,
            review_classification_refresh_attempted: false,
            candidate_selections: HashMap::new(),
            manual_isbn_inputs: HashMap::new(),
            review_covers: HashMap::new(),
            _filter_subscription: filter_subscription,
            splits: HierarchySplits::new(split_root, pane_id),
        };
        page.refresh(cx);
        page
    }

    fn selected_path(&self) -> Option<&str> {
        self.hierarchy.last().map(|subject| subject.id.as_str())
    }

    fn set_path(&mut self, path: Vec<SubjectCrumb>, window: &mut Window, cx: &mut Context<Self>) {
        if self.hierarchy.set_path(path, window, cx) {
            if !self.splits.is_secondary() {
                self.ui.persist_subject_path(self.hierarchy.path().to_vec());
            }
            self.refresh(cx);
        }
    }

    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        self.hierarchy.set_library_view(library_view, cx);
        for split in self.splits.pages() {
            split.update(cx, |page, cx| page.set_library_view(library_view, cx));
        }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.hierarchy.begin_load(cx);
        let subject_path = self.selected_path().map(str::to_owned);
        let file_type = self.hierarchy.file_type();
        let is_uncategorized = subject_path.as_deref() == Some(UNCATEGORIZED_SUBJECT_PATH);
        if is_uncategorized {
            self.refresh_request.begin();
            self.hierarchy.clear_error();
            self.hierarchy.install(Vec::new(), Vec::new(), cx);
            self.refresh_reviews(cx);
            for split in self.splits.pages() {
                split.update(cx, |page, cx| page.refresh(cx));
            }
            return;
        }
        let request_path = subject_path.clone();
        let generation = self.refresh_request.begin();
        let load = self.ui.load_subjects(request_path.clone(), file_type);
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh_request.is_current(generation) || page.selected_path() != request_path.as_deref() || page.hierarchy.file_type() != file_type {
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
                        page.hierarchy.finish_load(cx);
                        page.hierarchy.set_error(error, cx);
                    }
                }
            });
        });
        self.refresh_request.install(task);
        self.review_request.begin();
        self.reviews = Rc::new(Vec::new());
        self.review_loading = false;
        self.review_error = None;
        for split in self.splits.pages() {
            split.update(cx, |page, cx| page.refresh(cx));
        }
    }

    fn request_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(root) = self.splits.root() {
            let _ = root.update(cx, |root, cx| root.add_split(window, cx));
            return;
        }
        self.add_split(window, cx);
    }

    fn add_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane_id = self.splits.next_pane_id();
        let ui = self.ui.clone();
        let root = cx.entity().downgrade();
        let split_page = cx.new(|cx| Self::new_pane(ui, Vec::new(), Some(root), pane_id, window, cx));
        split_page.update(cx, |page, cx| page.refresh(cx));
        self.splits.push(pane_id, split_page, Vec::new());
        cx.notify();
    }

    fn request_close_split(&self, cx: &mut Context<Self>) {
        let Some(root) = self.splits.root() else { return };
        let pane_id = self.splits.pane_id();
        let _ = root.update(cx, |root, cx| root.close_split(pane_id, cx));
    }

    fn close_split(&mut self, pane_id: u64, cx: &mut Context<Self>) {
        if self.splits.close(pane_id) {
            cx.notify();
        }
    }

    fn refresh_reviews(&mut self, cx: &mut Context<Self>) {
        // Scanner and enrichment batches can emit several library events while
        // this read is in flight. Replacing the request for every event aborts
        // each future before it can publish its result, leaving the review page
        // permanently on its loading state. The active query observes the latest
        // committed SQLite snapshot when it gets a connection, so let it finish.
        if self.review_loading {
            return;
        }
        let generation = self.review_request.begin();
        self.review_loading = true;
        self.review_error = None;
        let load = self.ui.load_edition_identity_reviews();
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.review_request.is_current(generation) || page.selected_path() != Some(UNCATEGORIZED_SUBJECT_PATH) {
                    return;
                }
                page.review_loading = false;
                match result {
                    Ok(reviews) => {
                        let should_refresh_classifications = !page.review_classification_refresh_attempted
                            && reviews.iter().any(|review| review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous && review.candidates.iter().any(|candidate| candidate.classifications.is_empty()));
                        let present = reviews.iter().map(|review| review.content_hash.clone()).collect::<std::collections::HashSet<_>>();
                        page.candidate_selections.retain(|hash, _| present.contains(hash));
                        page.manual_isbn_inputs.retain(|hash, _| present.contains(hash));
                        page.review_covers.retain(|hash, _| present.contains(hash));
                        page.reviews = Rc::new(reviews);
                        if should_refresh_classifications {
                            page.review_classification_refresh_attempted = true;
                            page.refresh_review_classifications(cx);
                        }
                    }
                    Err(error) => page.review_error = Some(error),
                }
                cx.notify();
            });
        });
        self.review_request.install(task);
        cx.notify();
    }

    fn refresh_review_classifications(&mut self, cx: &mut Context<Self>) {
        let operation = self.ui.refresh_edition_identity_candidate_classifications();
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(updated) if updated > 0 => page.refresh_reviews(cx),
                Ok(_) => cx.notify(),
                Err(error) => {
                    page.review_error = Some(format!("Could not update candidate classifications: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn run_review_action<F>(&mut self, content_hash: epub_svelte_lib::ContentHash, operation: F, cx: &mut Context<Self>)
    where
        F: Future<Output = Result<(), String>> + 'static,
    {
        if self.review_busy.is_some() || self.review_batch_busy {
            return;
        }
        self.review_busy = Some(content_hash.clone());
        self.review_error = None;
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.review_busy = None;
                match result {
                    Ok(()) => {
                        page.reviews = Rc::new(page.reviews.iter().filter(|review| review.content_hash != content_hash).cloned().collect());
                        page.candidate_selections.remove(&content_hash);
                        page.manual_isbn_inputs.remove(&content_hash);
                        page.review_covers.remove(&content_hash);
                        cx.notify();
                    }
                    Err(error) => {
                        page.review_error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn use_selected_candidate(&mut self, hash: epub_svelte_lib::ContentHash, cx: &mut Context<Self>) {
        let Some(review) = self.reviews.iter().find(|review| review.content_hash == hash) else { return };
        let selected = self.candidate_selections.get(&hash).copied().unwrap_or(0).min(review.candidates.len().saturating_sub(1));
        let Some(candidate) = review.candidates.get(selected) else { return };
        let operation = self.ui.select_edition_identity_candidate(hash.clone(), candidate.edition_id.clone(), candidate.isbn13.clone());
        self.run_review_action(hash, async move { operation.await.map(|_| ()) }, cx);
    }

    fn save_manual_isbn(&mut self, hash: epub_svelte_lib::ContentHash, cx: &mut Context<Self>) {
        let Some(input) = self.manual_isbn_inputs.get(&hash) else { return };
        let isbn = input.read(cx).value().trim().to_owned();
        if isbn.is_empty() {
            self.review_error = Some("Enter an ISBN-10 or ISBN-13 before saving.".to_owned());
            cx.notify();
            return;
        }
        let operation = self.ui.apply_manual_edition_identity(hash.clone(), isbn);
        self.run_review_action(hash, async move { operation.await.map(|_| ()) }, cx);
    }

    fn reject_candidates(&mut self, hash: epub_svelte_lib::ContentHash, cx: &mut Context<Self>) {
        if self.review_busy.is_some() || self.review_batch_busy {
            return;
        }
        self.review_busy = Some(hash.clone());
        self.review_error = None;
        let operation = self.ui.reject_edition_identity_candidates(hash.clone());
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.review_busy = None;
                match result {
                    Ok(()) => {
                        let mut reviews = page.reviews.as_ref().clone();
                        if let Some(review) = reviews.iter_mut().find(|review| review.content_hash == hash) {
                            review.status = epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch;
                            review.detail = Some("No suitable edition selected during manual review".to_owned());
                            review.candidates.clear();
                            review.candidates_truncated = false;
                        }
                        page.reviews = Rc::new(reviews);
                        page.candidate_selections.remove(&hash);
                        cx.notify();
                    }
                    Err(error) => {
                        page.review_error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn retry_failed_identities(&mut self, cx: &mut Context<Self>) {
        if self.review_busy.is_some() || self.review_batch_busy {
            return;
        }
        self.review_batch_busy = true;
        self.review_error = None;
        let operation = self.ui.retry_failed_edition_identities();
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.review_batch_busy = false;
                match result {
                    Ok(_) => page.refresh(cx),
                    Err(error) => {
                        page.review_error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn reload_taxonomy(&mut self, cx: &mut Context<Self>) {
        if self.taxonomy_reloading {
            return;
        }
        self.taxonomy_reloading = true;
        let generation = self.taxonomy_reload_request.begin();
        let reload = self.ui.reload_unified_taxonomy();
        let task = cx.spawn(async move |page, cx| {
            let result = reload.await;
            let _ = page.update(cx, |page, cx| {
                if !page.taxonomy_reload_request.is_current(generation) {
                    return;
                }
                page.taxonomy_reloading = false;
                match result {
                    Ok(_) => {
                        page.hierarchy.clear_error();
                        page.refresh(cx);
                    }
                    Err(error) => page.hierarchy.set_error(format!("Taxonomy reload failed: {error}"), cx),
                }
                cx.notify();
            });
        });
        self.taxonomy_reload_request.install(task);
        cx.notify();
    }

    fn split_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let page = cx.entity();
        self.splits.actions("subject", page, |page, window, cx| page.update(cx, |page, cx| page.request_split(window, cx)), |page, cx| page.update(cx, |page, cx| page.request_close_split(cx)), cx)
    }

    fn control_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let selected_file_type = self.hierarchy.file_type();
        let filter_page = page.clone();
        let file_type_control =
            components::catalog_dropdown_button("subject-file-type", selected_file_type.label(), false, false, theme).h(gpui::px(components::TOPBAR_ACTION_HEIGHT)).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
                epub_svelte_lib::library::LibraryFileTypeFilter::ALL.into_iter().fold(menu, |menu, file_type| {
                    let page = filter_page.clone();
                    menu.item(PopupMenuItem::new(file_type.label()).checked(file_type == selected_file_type).on_click(move |_, _, cx| {
                        page.update(cx, |page, cx| page.set_file_type(file_type, cx));
                    }))
                })
            });
        let label = if self.taxonomy_reloading { "Reloading…" } else { "Reload taxonomy" };
        components::browser_topbar_right()
            .child(file_type_control)
            .child(components::topbar_action_button("reload-taxonomy", label, gpui_component::IconName::Redo2, theme).disabled(self.taxonomy_reloading).on_click(move |_, _, cx| page.update(cx, |page, cx| page.reload_taxonomy(cx))))
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
        let file_type = self.hierarchy.file_type();
        let ui = self.ui.clone();
        let debounce = std::time::Duration::from_millis(u64::from(ui.contracts().browsing_content.search_debounce_ms));
        let timer = cx.background_executor().timer(debounce);
        let task = cx.spawn(async move |page, cx| {
            timer.await;
            let result = ui.search_subject(subject_path, query, file_type).await;
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

    fn set_file_type(&mut self, file_type: epub_svelte_lib::library::LibraryFileTypeFilter, cx: &mut Context<Self>) {
        if self.hierarchy.set_file_type(file_type, cx) {
            self.refresh(cx);
        }
    }

    fn render_review_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.selected_path() != Some(UNCATEGORIZED_SUBJECT_PATH) {
            return None;
        }
        for review in self.reviews.iter().filter(|review| review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch) {
            self.manual_isbn_inputs.entry(review.content_hash.clone()).or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("ISBN-10 or ISBN-13")));
        }
        for review in self.reviews.iter().filter(|review| review.status == self.review_status) {
            self.review_covers.entry(review.content_hash.clone()).or_insert_with(|| ReviewCoverView::create(review.content_hash.clone(), self.ui.clone(), cx));
        }

        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let statuses = [
            (epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous, "Ambiguous"),
            (epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch, "No match"),
            (epub_svelte_lib::library::EditionIdentityReviewStatus::Failed, "Failed"),
        ];
        let mut tabs = div().w_full().flex().gap(px(1.0)).border_1().border_color(theme.border).bg(theme.border);
        for (status, label) in statuses {
            let count = self.reviews.iter().filter(|review| review.status == status).count();
            let selected = self.review_status == status;
            let tab_page = page.clone();
            tabs = tabs.child(
                Button::new(format!("isbn-review-tab-{label}"))
                    .label(format!("{label}  {count}"))
                    .ghost()
                    .flex_1()
                    .h(px(38.0))
                    .bg(if selected { theme.accent_soft } else { theme.surface })
                    .text_color(if selected { theme.text } else { theme.text_muted })
                    .on_click(move |_, _, cx| {
                        tab_page.update(cx, |page, cx| {
                            page.review_status = status;
                            cx.notify();
                        })
                    }),
            );
        }

        let rows = self.reviews.clone();
        let filtered = Rc::new(rows.iter().enumerate().filter_map(|(index, review)| (review.status == self.review_status).then_some(index)).collect::<Vec<_>>());
        let inputs = Rc::new(self.manual_isbn_inputs.clone());
        let covers = Rc::new(self.review_covers.clone());
        let selections = Rc::new(self.candidate_selections.clone());
        let busy = self.review_busy.clone();
        let batch_busy = self.review_batch_busy;
        let row_page = page.clone();
        let visible_rows = filtered.clone();
        let row_height = px(app_contract::METADATA_REVIEW_ITEM_HEIGHT_PX as f32);
        let list = uniform_list(format!("isbn-review-list-{:?}", self.review_status), filtered.len(), move |range, _, _| {
            range
                .map(|index| {
                    let review = &rows[visible_rows[index]];
                    let hash = review.content_hash.clone();
                    let cover = covers.get(&hash).expect("every review row has a cover view").clone();
                    let selected = selections.get(&hash).copied().unwrap_or(0).min(review.candidates.len().saturating_sub(1));
                    let candidate = review.candidates.get(selected);
                    let is_busy = busy.is_some() || batch_busy;
                    let own_busy = busy.as_ref() == Some(&hash);
                    let status_label = match review.status {
                        epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous => "Ambiguous",
                        epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch => "No match",
                        epub_svelte_lib::library::EditionIdentityReviewStatus::Failed => "Failed",
                    };
                    let status_color = if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Failed { theme.danger } else { theme.warning };
                    let mut candidate_controls = div().flex().items_center().gap(px(6.0));
                    if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous && review.candidates.len() > 1 {
                        let previous_page = row_page.clone();
                        let previous_hash = hash.clone();
                        let candidate_count = review.candidates.len();
                        candidate_controls = candidate_controls
                            .child(Button::new(format!("isbn-candidate-prev-{hash}")).label("Previous").secondary().outline().small().disabled(is_busy).on_click(move |_, _, cx| {
                                previous_page.update(cx, |page, cx| {
                                    let current = page.candidate_selections.get(&previous_hash).copied().unwrap_or(0);
                                    page.candidate_selections.insert(previous_hash.clone(), if current == 0 { candidate_count - 1 } else { current - 1 });
                                    cx.notify();
                                })
                            }))
                            .child(div().text_color(theme.text_muted).child(format!("{} of {}", selected + 1, review.candidates.len())));
                        let next_page = row_page.clone();
                        let next_hash = hash.clone();
                        candidate_controls = candidate_controls.child(Button::new(format!("isbn-candidate-next-{hash}")).label("Next").secondary().outline().small().disabled(is_busy).on_click(move |_, _, cx| {
                            next_page.update(cx, |page, cx| {
                                let current = page.candidate_selections.get(&next_hash).copied().unwrap_or(0);
                                page.candidate_selections.insert(next_hash.clone(), (current + 1) % candidate_count);
                                cx.notify();
                            })
                        }));
                    }

                    let evidence = if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous {
                        let metadata_values = |title: &str,
                                               authors: &[String],
                                               publishers: &[String],
                                               publication_year: Option<i32>,
                                               classifications: &[String],
                                               show_classification_status: bool,
                                               inferred_title: bool,
                                               inferred_author: Option<&str>,
                                               inferred_publication_year: Option<i32>| {
                            let mut values = div().flex_1().min_w_0().h_full().overflow_hidden().flex().flex_col().gap(px(5.0));
                            if !title.trim().is_empty() {
                                let mut field = div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).line_clamp(2).line_height(gpui::relative(1.25)).font_weight(FontWeight::SEMIBOLD);
                                if inferred_title {
                                    field = field.child(div().text_size(gpui::rems(10.0 / 16.0)).font_weight(FontWeight::NORMAL).text_color(theme.accent).child("Inferred from title"));
                                }
                                values = values.child(field.child(title.to_owned()));
                            }
                            if !authors.is_empty() || inferred_author.is_some() {
                                let mut author_values = authors.to_vec();
                                if let Some(author) = inferred_author {
                                    if !author_values.iter().any(|existing| existing.eq_ignore_ascii_case(author)) {
                                        author_values.push(author.to_owned());
                                    }
                                }
                                let mut field = div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).line_clamp(2).line_height(gpui::relative(1.25)).text_color(theme.text_muted);
                                if inferred_author.is_some() {
                                    field = field.child(div().text_size(gpui::rems(10.0 / 16.0)).text_color(theme.accent).child("Author inferred from title"));
                                }
                                values = values.child(field.child(author_values.join(", ")));
                            }
                            if !publishers.is_empty() {
                                values = values.child(div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).line_clamp(2).line_height(gpui::relative(1.25)).text_color(theme.text_muted).child(publishers.join(", ")));
                            }
                            if let Some(year) = publication_year.or(inferred_publication_year) {
                                let mut field = div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).text_color(theme.text_muted);
                                if publication_year.is_none() && inferred_publication_year.is_some() {
                                    field = field.child(div().text_size(gpui::rems(10.0 / 16.0)).text_color(theme.accent).child("Year inferred from title"));
                                }
                                values = values.child(field.child(year.to_string()));
                            }
                            if !classifications.is_empty() {
                                values = values.child(div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).line_clamp(3).line_height(gpui::relative(1.25)).text_color(theme.accent).child(classifications.join("\n")));
                            } else if show_classification_status {
                                values = values.child(div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).text_color(theme.text_muted).child("No classifications"));
                            }
                            values
                        };
                        let local_title = review.inferred_title.as_deref().unwrap_or(&review.title);
                        let local_column = div().flex_1().min_w_0().h(px(352.0)).overflow_hidden().flex().flex_col().child(div().h(px(32.0)).flex_none().flex().items_center().font_weight(FontWeight::SEMIBOLD).child("Your book")).child(
                            div().w_full().h(px(320.0)).overflow_hidden().flex().items_start().child(metadata_values(
                                local_title,
                                &review.authors,
                                &review.publishers,
                                review.publication_year,
                                &[],
                                false,
                                review.inferred_title.is_some(),
                                review.inferred_author.as_deref(),
                                review.inferred_publication_year,
                            )),
                        );
                        let external_column = if let Some(candidate) = candidate {
                            let use_page = row_page.clone();
                            let use_hash = hash.clone();
                            let reject_page = row_page.clone();
                            let reject_hash = hash.clone();
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(352.0))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .child(div().h(px(32.0)).flex_none().flex().items_center().justify_between().gap(px(8.0)).child(div().font_weight(FontWeight::SEMIBOLD).child("External book")).child(candidate_controls))
                                .child(div().w_full().h(px(280.0)).flex_none().overflow_hidden().flex().items_start().child(metadata_values(
                                    &candidate.title,
                                    &candidate.authors,
                                    &candidate.publishers,
                                    candidate.publication_year,
                                    &candidate.classifications,
                                    true,
                                    false,
                                    None,
                                    None,
                                )))
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(40.0))
                                        .flex_none()
                                        .pt(px(8.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            Button::new(format!("isbn-use-{hash}"))
                                                .label(if own_busy { "Saving…" } else { "Choose candidate" })
                                                .primary()
                                                .small()
                                                .disabled(is_busy)
                                                .on_click(move |_, _, cx| use_page.update(cx, |page, cx| page.use_selected_candidate(use_hash.clone(), cx))),
                                        )
                                        .child(
                                            Button::new(format!("isbn-reject-{hash}"))
                                                .label("No suitable edition")
                                                .secondary()
                                                .outline()
                                                .small()
                                                .disabled(is_busy)
                                                .on_click(move |_, _, cx| reject_page.update(cx, |page, cx| page.reject_candidates(reject_hash.clone(), cx))),
                                        ),
                                )
                        } else {
                            div().flex_1().min_w_0().child(div().h(px(32.0)).flex().items_center().font_weight(FontWeight::SEMIBOLD).child("External book"))
                        };
                        let cover_column = div().w(px(components::BOOK_COVER_WIDTH)).h(px(352.0)).flex_none().pt(px(32.0)).child(cover.clone());
                        div().w_full().min_w_0().flex().items_start().gap(px(8.0)).child(cover_column).child(local_column).child(external_column).into_any_element()
                    } else {
                        let detail = review.detail.clone().unwrap_or_else(|| {
                            if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Failed { "The metadata lookup failed without an error message.".to_owned() } else { "No candidate was returned.".to_owned() }
                        });
                        div().w_full().min_w_0().overflow_hidden().text_color(if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Failed { theme.danger } else { theme.text_muted }).child(detail).into_any_element()
                    };

                    let mut actions = div().w_full().flex().items_end().gap(px(8.0));
                    if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch
                        && let Some(input) = inputs.get(&hash)
                    {
                        let save_page = row_page.clone();
                        let save_hash = hash.clone();
                        actions = actions.child(div().ml_auto().w(px(190.0)).h(px(32.0)).child(Input::new(input))).child(
                            Button::new(format!("isbn-manual-{hash}"))
                                .label("Save ISBN")
                                .secondary()
                                .outline()
                                .small()
                                .disabled(is_busy)
                                .on_click(move |_, _, cx| save_page.update(cx, |page, cx| page.save_manual_isbn(save_hash.clone(), cx))),
                        );
                    }

                    let row = div().w_full().h(row_height).px(px(10.0)).py(px(app_contract::DETAILED_ROW_VERTICAL_PADDING_PX as f32)).mb(px(6.0)).border_1().border_color(theme.border).bg(theme.page_bg).flex();
                    if review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous {
                        row.items_start().child(div().w_full().min_w_0().flex().flex_col().child(evidence))
                    } else {
                        row.items_start().gap(px(10.0)).child(cover).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().flex_none().text_color(status_color).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::BOLD).child(status_label))
                                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(review.title.clone())),
                                )
                                .child(evidence)
                                .child(actions),
                        )
                    }
                })
                .collect::<Vec<_>>()
        });

        let refresh_page = page.clone();
        let update_failed_page = page;
        let failed_count = self.reviews.iter().filter(|review| review.status == epub_svelte_lib::library::EditionIdentityReviewStatus::Failed).count();
        let show_update_all = self.review_status == epub_svelte_lib::library::EditionIdentityReviewStatus::Failed && failed_count > 0;
        let mut header_actions = div().flex().items_center().gap(px(8.0));
        if show_update_all {
            header_actions = header_actions.child(
                Button::new("update-all-failed-isbn-reviews")
                    .label(if self.review_batch_busy { "Updating…" } else { "Update all" })
                    .primary()
                    .small()
                    .disabled(self.review_loading || self.review_busy.is_some() || self.review_batch_busy)
                    .on_click(move |_, _, cx| update_failed_page.update(cx, |page, cx| page.retry_failed_identities(cx))),
            );
        }
        header_actions = header_actions.child(
            Button::new("refresh-isbn-reviews")
                .label("Refresh")
                .secondary()
                .outline()
                .small()
                .disabled(self.review_loading || self.review_busy.is_some() || self.review_batch_busy)
                .on_click(move |_, _, cx| refresh_page.update(cx, |page, cx| page.refresh_reviews(cx))),
        );
        let review_description = match self.review_status {
            epub_svelte_lib::library::EditionIdentityReviewStatus::Ambiguous => "Compare your metadata with each candidate, then choose the matching record or reject them.",
            epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch => "Enter a known ISBN directly for books with no Open Library match.",
            epub_svelte_lib::library::EditionIdentityReviewStatus::Failed => "Retry all failed metadata lookups as one batch.",
        };
        let mut header_card = div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(16.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.raised_bg)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.0))
                    .child(div().flex().flex_col().gap(px(4.0)).child(div().text_size(gpui::rems(17.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child("Metadata review")).child(div().text_color(theme.text_muted).child(review_description)))
                    .child(header_actions),
            )
            .child(tabs);
        if let Some(error) = &self.review_error {
            header_card = header_card.child(gpui_component::alert::Alert::error("isbn-review-error", error.clone()).with_size(gpui_component::Size::Small));
        }
        let body = if self.review_loading && self.reviews.is_empty() {
            components::browser_inline_note("Loading metadata reviews…", theme).into_any_element()
        } else if filtered.is_empty() {
            components::browser_inline_note("No metadata decisions in this status need review.", theme).into_any_element()
        } else {
            div().w_full().flex_1().min_h_0().flex().flex_col().border_1().border_color(theme.border).overflow_hidden().child(components::virtual_list(list)).into_any_element()
        };
        Some(div().size_full().min_h_0().flex().flex_col().gap(px(12.0)).child(header_card).child(body).into_any_element())
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let file_type = self.hierarchy.file_type();
        let (title, description) = if file_type != epub_svelte_lib::library::LibraryFileTypeFilter::All {
            let label = file_type.label().to_lowercase();
            (format!("No {label} here"), format!("This subject and its subsubjects contain no {label}."))
        } else if self.hierarchy.path().is_empty() {
            ("No categorized books".to_owned(), "Books with no supported subject assignment will appear under Uncategorized.".to_owned())
        } else {
            ("No books assigned directly".to_owned(), "This subject currently has no direct book assignments.".to_owned())
        };
        components::browser_actionable_empty(title, description, components::browser_topbar_right(), theme).into_any_element()
    }
}

impl Render for SubjectsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(detail) = page_detail(self) {
            return detail;
        }
        let review_only = self.selected_path() == Some(UNCATEGORIZED_SUBJECT_PATH);
        let empty = self.render_empty(cx);
        let breadcrumb_actions = self.split_actions(cx);
        let control_actions = self.control_actions(cx);
        let review_panel = self.render_review_panel(window, cx);
        let hierarchy = self.hierarchy.render(
            "Subjects",
            app_contract::LibraryFeature::Subjects.icon(),
            "subject-button",
            "subject-books",
            self.ui.contracts().browsing_states.retry_label.clone().into(),
            self.ui.contracts().browsing_states.searching_label.clone().into(),
            breadcrumb_actions,
            control_actions,
            empty,
            self.ui.contracts().browsing_states.loading_library_label.clone().into(),
            cx.entity(),
            |page, path, window, cx| page.set_path(path, window, cx),
            |page, cx| page.refresh(cx),
            None,
            None,
            None,
            !review_only,
            review_panel,
            None,
            None,
            None,
            None,
            window,
            cx,
        );
        let pane = components::hierarchy_page_host(hierarchy);
        self.splits.render(pane.into_any_element(), window, cx)
    }
}

impl DetailOwner for SubjectsPage {
    fn detail(&self) -> Option<&Entity<BookDetailPage>> {
        self.detail.as_ref()
    }

    fn detail_mut(&mut self) -> &mut Option<Entity<BookDetailPage>> {
        &mut self.detail
    }
}
