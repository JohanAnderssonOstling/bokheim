use std::rc::Rc;

use epub_svelte_lib::ContentHash;
use epub_svelte_lib::library::{EditionIdentityReview, EditionIdentityReviewCandidate};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, Render, Window, div, px, uniform_list};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{IconName, Sizable};
use ui_components::{self as components, BrowserTheme};

use crate::library::services::LibraryContext;

use super::super::review_cover::ReviewCoverView;
use super::ReviewRejected;

const CANDIDATE_ACTION_SIZE: f32 = 32.0;
const ROW_EDGE_PADDING: f32 = 10.0;

#[derive(Clone)]
struct AmbiguousReviewItem {
    review: EditionIdentityReview,
    selected_candidate: usize,
    cover: Entity<ReviewCoverView>,
}

pub(super) struct AmbiguousReviewPanel {
    ui: LibraryContext,
    items: Rc<Vec<AmbiguousReviewItem>>,
    error: Option<String>,
}

impl gpui::EventEmitter<ReviewRejected> for AmbiguousReviewPanel {}

impl AmbiguousReviewPanel {
    pub(super) fn new(ui: LibraryContext, reviews: Vec<EditionIdentityReview>, cx: &mut Context<Self>) -> Self {
        let items = reviews.into_iter().map(|review| AmbiguousReviewItem { cover: ReviewCoverView::create(review.content_hash.clone(), ui.clone(), cx), review, selected_candidate: 0 }).collect();
        Self { ui, items: Rc::new(items), error: None }
    }

    pub(super) fn count(&self) -> usize {
        self.items.len()
    }

    fn remove_review(&mut self, hash: &ContentHash) {
        Rc::make_mut(&mut self.items).retain(|item| &item.review.content_hash != hash);
    }

    fn select_previous_candidate(&mut self, hash: &ContentHash, cx: &mut Context<Self>) {
        let Some(item) = Rc::make_mut(&mut self.items).iter_mut().find(|item| &item.review.content_hash == hash) else {
            return;
        };
        let candidate_count = item.review.candidates.len();
        if candidate_count > 0 {
            item.selected_candidate = if item.selected_candidate == 0 { candidate_count - 1 } else { item.selected_candidate - 1 };
            cx.notify();
        }
    }

    fn select_next_candidate(&mut self, hash: &ContentHash, cx: &mut Context<Self>) {
        let Some(item) = Rc::make_mut(&mut self.items).iter_mut().find(|item| &item.review.content_hash == hash) else {
            return;
        };
        let candidate_count = item.review.candidates.len();
        if candidate_count > 0 {
            item.selected_candidate = (item.selected_candidate + 1) % candidate_count;
            cx.notify();
        }
    }

    fn finish_action(&mut self, hash: &ContentHash, result: Result<(), String>, event: Option<ReviewRejected>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.remove_review(hash);
                self.error = None;
                if let Some(event) = event {
                    cx.emit(event);
                }
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn choose_candidate(&mut self, hash: ContentHash, cx: &mut Context<Self>) {
        let Some(item) = self.items.iter().find(|item| item.review.content_hash == hash) else {
            return;
        };
        let selected = item.selected_candidate.min(item.review.candidates.len().saturating_sub(1));
        let Some(candidate) = item.review.candidates.get(selected) else {
            return;
        };
        let result = self.ui.select_edition_identity_candidate(hash.clone(), &candidate.edition_id, &candidate.isbn13).map(|_| ());
        self.finish_action(&hash, result, None, cx);
    }

    fn reject_candidates(&mut self, hash: ContentHash, cx: &mut Context<Self>) {
        let Some(mut review) = self.items.iter().find(|item| item.review.content_hash == hash).map(|item| item.review.clone()) else {
            return;
        };
        review.status = epub_svelte_lib::library::EditionIdentityReviewStatus::NoMatch;
        review.detail = Some("No suitable edition selected during manual review".to_owned());
        review.candidates.clear();
        review.candidates_truncated = false;
        let result = self.ui.reject_edition_identity_candidates(hash.clone());
        self.finish_action(&hash, result, Some(ReviewRejected(review)), cx);
    }
}

struct ReviewMetadata<'a> {
    title: &'a str,
    authors: &'a [String],
    publishers: &'a [String],
    publication_year: Option<i32>,
    classifications: &'a [String],
    inferred_author: Option<&'a str>,
    inferred_publication_year: Option<i32>,
}

impl<'a> ReviewMetadata<'a> {
    fn empty() -> Self {
        Self { title: "", authors: &[], publishers: &[], publication_year: None, classifications: &[], inferred_author: None, inferred_publication_year: None }
    }

    fn local(review: &'a EditionIdentityReview) -> Self {
        Self {
            title: review.inferred_title.as_deref().unwrap_or(&review.title),
            authors: &review.authors,
            publishers: &review.publishers,
            publication_year: review.publication_year,
            classifications: &[],
            inferred_author: review.inferred_author.as_deref(),
            inferred_publication_year: review.inferred_publication_year,
        }
    }

    fn external(candidate: &'a EditionIdentityReviewCandidate) -> Self {
        Self {
            title: &candidate.title,
            authors: &candidate.authors,
            publishers: &candidate.publishers,
            publication_year: candidate.publication_year,
            classifications: &candidate.classifications,
            inferred_author: None,
            inferred_publication_year: None,
        }
    }
}

fn metadata_field(theme: BrowserTheme) -> Div {
    div().w_full().px(px(7.0)).py(px(5.0)).border_1().border_color(theme.border).text_color(theme.text_muted).line_clamp(2)
}

fn metadata_values(metadata: ReviewMetadata<'_>, theme: BrowserTheme) -> Div {
    let mut values = div().flex_1().min_w_0().h_full().overflow_hidden().flex().flex_col().gap(px(5.0));
    if !metadata.title.trim().is_empty() {
        let field = metadata_field(theme).line_clamp(2).line_height(gpui::relative(1.25)).font_weight(FontWeight::SEMIBOLD);
        values = values.child(field.child(metadata.title.to_owned()));
    }
    if !metadata.authors.is_empty() || metadata.inferred_author.is_some() {
        let mut authors = metadata.authors.to_vec();
        if let Some(author) = metadata.inferred_author
            && !authors.iter().any(|existing| existing.eq_ignore_ascii_case(author))
        {
            authors.push(author.to_owned());
        }
        let field = metadata_field(theme).line_clamp(2).line_height(gpui::relative(1.25)).text_color(theme.text_muted);
        values = values.child(field.child(authors.join(", ")));
    }
    if !metadata.publishers.is_empty() {
        values = values.child(metadata_field(theme).line_clamp(2).line_height(gpui::relative(1.25)).text_color(theme.text_muted).child(metadata.publishers.join(", ")));
    }
    if let Some(year) = metadata.publication_year.or(metadata.inferred_publication_year) {
        let field = metadata_field(theme).text_color(theme.text_muted);
        values = values.child(field.child(year.to_string()));
    }
    if !metadata.classifications.is_empty() {
        values = values.child(metadata_field(theme).line_clamp(3).line_height(gpui::relative(1.25)).text_color(theme.accent).child(metadata.classifications.join("\n")));
    }
    values
}

fn comparison_column(metadata: ReviewMetadata<'_>, footer: Option<Div>, theme: BrowserTheme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(components::BOOK_COVER_HEIGHT))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(div().w_full().flex_1().min_h_0().overflow_hidden().flex().items_start().child(metadata_values(metadata, theme)))
        .when_some(footer, |column, footer| column.child(footer))
}

fn candidate_controls(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousReviewPanel>, theme: BrowserTheme) -> Div {
    let review = &item.review;
    let mut controls = div().flex_none().flex().items_center().gap(px(6.0));
    if review.candidates.len() <= 1 {
        return controls;
    }

    let selected = item.selected_candidate.min(review.candidates.len() - 1);
    let previous_panel = panel.clone();
    let previous_hash = review.content_hash.clone();
    let next_panel = panel.clone();
    let next_hash = review.content_hash.clone();
    controls = controls
        .child(components::outlined_icon_button(format!("isbn-candidate-prev-{}", review.content_hash), "Previous candidate", IconName::ArrowLeft, px(CANDIDATE_ACTION_SIZE), components::RADIUS_SM, theme).text_color(theme.accent).on_click(
            move |_, _, cx| {
                previous_panel.update(cx, |panel, cx| panel.select_previous_candidate(&previous_hash, cx));
            },
        ))
        .child(div().text_color(theme.text_muted).child(format!("{} of {}", selected + 1, review.candidates.len())))
        .child(components::outlined_icon_button(format!("isbn-candidate-next-{}", review.content_hash), "Next candidate", IconName::ArrowRight, px(CANDIDATE_ACTION_SIZE), components::RADIUS_SM, theme).text_color(theme.accent).on_click(
            move |_, _, cx| {
                next_panel.update(cx, |panel, cx| panel.select_next_candidate(&next_hash, cx));
            },
        ));
    controls
}

fn local_column(review: &EditionIdentityReview, theme: BrowserTheme) -> Div {
    comparison_column(ReviewMetadata::local(review), None, theme)
}

fn external_column(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousReviewPanel>, theme: BrowserTheme) -> Div {
    let review = &item.review;
    let selected = item.selected_candidate.min(review.candidates.len().saturating_sub(1));
    let Some(candidate) = review.candidates.get(selected) else {
        return comparison_column(ReviewMetadata::empty(), None, theme);
    };

    let hash = review.content_hash.clone();
    let choose_panel = panel.clone();
    let choose_hash = hash.clone();
    let reject_panel = panel.clone();
    let reject_hash = hash.clone();
    let actions = div().w_full().h(px(CANDIDATE_ACTION_SIZE + ROW_EDGE_PADDING * 2.0)).flex_none().p(px(ROW_EDGE_PADDING)).flex().items_center().justify_between().gap(px(8.0)).child(candidate_controls(item, panel, theme)).child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(Button::new(format!("isbn-use-{hash}")).label("Choose").primary().small().on_click(move |_, _, cx| {
                choose_panel.update(cx, |panel, cx| panel.choose_candidate(choose_hash.clone(), cx));
            }))
            .child(components::outlined_text_button(format!("isbn-reject-{hash}"), "Reject").on_click(move |_, _, cx| {
                reject_panel.update(cx, |panel, cx| panel.reject_candidates(reject_hash.clone(), cx));
            })),
    );
    comparison_column(ReviewMetadata::external(candidate), Some(actions), theme)
}

fn review_row(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousReviewPanel>, theme: BrowserTheme) -> Div {
    let cover_column = div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().child(item.cover.clone());
    let comparison_columns = div().flex_1().min_w_0().flex().items_start().gap(px(8.0)).child(local_column(&item.review, theme)).child(external_column(item, panel, theme));
    div()
        .w_full()
        .h(px(components::BOOK_COVER_HEIGHT))
        .pr(px(ROW_EDGE_PADDING))
        .mb(px(6.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.page_bg)
        .flex()
        .items_start()
        .child(div().w_full().h_full().min_w_0().flex().items_start().gap(px(8.0)).child(cover_column).child(comparison_columns))
}

impl Render for AmbiguousReviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let panel = cx.entity();
        let rows = self.items.clone();
        let list = uniform_list("ambiguous-review-list", rows.len(), move |range, _, _| range.map(|index| review_row(&rows[index], &panel, theme)).collect::<Vec<_>>());
        let body = if self.items.is_empty() {
            components::browser_inline_note("No ambiguous metadata decisions need review.", theme).into_any_element()
        } else {
            components::remaining_space_column().border_1().border_color(theme.border).overflow_hidden().child(components::virtual_list(list)).into_any_element()
        };
        components::full_size_column().gap(px(12.0)).when_some(self.error.clone(), |panel, error| panel.child(gpui_component::alert::Alert::error("ambiguous-review-error", error))).child(body)
    }
}
