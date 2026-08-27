use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, Render, Subscription, Window, div, px, rems};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{IconName, Sizable};
use library_model::{EditionIdentityReview, EditionIdentityReviewCandidate};
use sync_common::ContentHash;
use ui_components::{self as components, BrowserTheme};

use crate::library::services::LibraryContext;
use crate::library::widgets::{Paginator, PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};

use super::super::review_cover::ReviewCoverView;
use super::{ReviewsChanged, review_paginator_controls};

const CANDIDATE_ACTION_SIZE: f32 = 32.0;
const ROW_EDGE_PADDING: f32 = 10.0;
const COMPARISON_GAP: f32 = 8.0;
const FIELD_HORIZONTAL_PADDING: f32 = 7.0;
const COMPARISON_MINIMUM_TEXT_WIDTH_REM: f32 = 16.0;
const COMPARISON_PREFERRED_TEXT_WIDTH_REM: f32 = 19.0;
const COMPARISON_MAXIMUM_TEXT_WIDTH_REM: f32 = 24.0;

fn paginator_policy() -> PaginatorGroupPolicy {
    let fixed_width = components::BOOK_COVER_WIDTH + ROW_EDGE_PADDING + COMPARISON_GAP * 2.0 + FIELD_HORIZONTAL_PADDING * 4.0;
    PaginatorGroupPolicy::new(
        PaginatorWidthPolicy::new(rems(COMPARISON_MINIMUM_TEXT_WIDTH_REM * 2.0), rems(COMPARISON_PREFERRED_TEXT_WIDTH_REM * 2.0), rems(COMPARISON_MAXIMUM_TEXT_WIDTH_REM * 2.0)).with_pixels(px(fixed_width)),
        PaginatorSizing::pixels(px(components::BOOK_COVER_HEIGHT)),
        PaginatorGapPolicy::new(px(components::SPACE_SM), px(components::SPACE_MD)),
        PaginatorGapPolicy::new(px(6.0), px(components::SPACE_SM)),
    )
}

#[derive(Clone)]
struct AmbiguousReviewItem {
    review: EditionIdentityReview,
    selected_candidate: usize,
    cover: Entity<ReviewCoverView>,
}

pub(super) struct AmbiguousLane {
    ui: LibraryContext,
    items: Rc<Vec<AmbiguousReviewItem>>,
    paginator: Entity<Paginator>,
    _paginator_updates: Subscription,
    error: Option<String>,
}

impl gpui::EventEmitter<ReviewsChanged> for AmbiguousLane {}

impl AmbiguousLane {
    pub(super) fn new(ui: LibraryContext, reviews: Vec<EditionIdentityReview>, cx: &mut Context<Self>) -> Self {
        let items = Rc::new(reviews.into_iter().map(|review| AmbiguousReviewItem { cover: ReviewCoverView::create(review.content_hash, ui.clone(), cx), review, selected_candidate: 0 }).collect::<Vec<_>>());
        let lane = cx.entity().downgrade();
        let children = items
            .iter()
            .map(|item| {
                let hash = item.review.content_hash;
                let lane = lane.clone();
                PaginatorChild::new(move |_, cx| {
                    let Some(lane) = lane.upgrade() else {
                        return div().into_any_element();
                    };
                    let state = lane.read(cx);
                    let Some(item) = state.items.iter().find(|item| item.review.content_hash == hash) else {
                        return div().into_any_element();
                    };
                    review_row(item, &lane, components::browser_theme(cx)).into_any_element()
                })
            })
            .collect::<Vec<_>>();
        let paginator = cx.new(|_| Paginator::new("ambiguous-review-paginator", [PaginatorGroup::new(paginator_policy(), children)]));
        let paginator_updates = cx.observe(&paginator, |_, _, cx| cx.notify());
        Self { ui, items, paginator, _paginator_updates: paginator_updates, error: None }
    }

    pub(super) fn count(&self) -> usize {
        self.items.len()
    }

    fn select_previous_candidate(&mut self, hash: &ContentHash, cx: &mut Context<Self>) {
        let Some(item) = Rc::make_mut(&mut self.items).iter_mut().find(|item| &item.review.content_hash == hash) else {
            return;
        };
        let candidate_count = item.review.candidates.len();
        if candidate_count > 0 {
            item.selected_candidate = if item.selected_candidate == 0 { candidate_count - 1 } else { item.selected_candidate - 1 };
            self.paginator.update(cx, |_, cx| cx.notify());
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
            self.paginator.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    fn finish_action(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.error = None;
                cx.emit(ReviewsChanged);
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
        let result = self.ui.backend().select_edition_identity_candidate(hash, &candidate.edition_id, &candidate.isbn13).map(|_| ()).map_err(|error| error.to_string());
        self.finish_action(result, cx);
    }

    fn reject_candidates(&mut self, hash: ContentHash, cx: &mut Context<Self>) {
        if !self.items.iter().any(|item| item.review.content_hash == hash) {
            return;
        }
        let result = self.ui.backend().reject_edition_identity_candidates(hash).map_err(|error| error.to_string());
        self.finish_action(result, cx);
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

fn candidate_controls(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousLane>, theme: BrowserTheme) -> Div {
    let review = &item.review;
    let mut controls = div().flex_none().flex().items_center().gap(px(6.0));
    if review.candidates.len() <= 1 {
        return controls;
    }

    let selected = item.selected_candidate.min(review.candidates.len() - 1);
    let previous_panel = panel.clone();
    let previous_hash = review.content_hash;
    let next_panel = panel.clone();
    let next_hash = review.content_hash;
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

fn external_column(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousLane>, theme: BrowserTheme) -> Div {
    let review = &item.review;
    let selected = item.selected_candidate.min(review.candidates.len().saturating_sub(1));
    let Some(candidate) = review.candidates.get(selected) else {
        return comparison_column(ReviewMetadata::empty(), None, theme);
    };

    let hash = review.content_hash;
    let choose_panel = panel.clone();
    let choose_hash = hash;
    let reject_panel = panel.clone();
    let reject_hash = hash;
    let actions = div().w_full().h(px(CANDIDATE_ACTION_SIZE + ROW_EDGE_PADDING * 2.0)).flex_none().p(px(ROW_EDGE_PADDING)).flex().items_center().justify_between().gap(px(8.0)).child(candidate_controls(item, panel, theme)).child(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(Button::new(format!("isbn-use-{hash}")).label("Choose").primary().small().on_click(move |_, _, cx| {
                choose_panel.update(cx, |panel, cx| panel.choose_candidate(choose_hash, cx));
            }))
            .child(components::outlined_text_button(format!("isbn-reject-{hash}"), "Reject").on_click(move |_, _, cx| {
                reject_panel.update(cx, |panel, cx| panel.reject_candidates(reject_hash, cx));
            })),
    );
    comparison_column(ReviewMetadata::external(candidate), Some(actions), theme)
}

fn review_row(item: &AmbiguousReviewItem, panel: &Entity<AmbiguousLane>, theme: BrowserTheme) -> Div {
    let cover_column = div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().child(item.cover.clone());
    let comparison_columns = div().flex_1().min_w_0().flex().items_start().gap(px(COMPARISON_GAP)).child(local_column(&item.review, theme)).child(external_column(item, panel, theme));
    div()
        .w_full()
        .h(px(components::BOOK_COVER_HEIGHT))
        .pr(px(ROW_EDGE_PADDING))
        .border_1()
        .border_color(theme.border)
        .bg(theme.page_bg)
        .flex()
        .items_start()
        .child(div().w_full().h_full().min_w_0().flex().items_start().gap(px(COMPARISON_GAP)).child(cover_column).child(comparison_columns))
}

impl Render for AmbiguousLane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let body = if self.items.is_empty() {
            components::browser_inline_note("No ambiguous metadata decisions need review.", theme).into_any_element()
        } else {
            components::remaining_space_column().overflow_hidden().child(self.paginator.clone()).into_any_element()
        };
        let controls = (!self.items.is_empty() && !cx.supports_touch_input()).then(|| div().w_full().flex().justify_end().child(review_paginator_controls("ambiguous-review", &self.paginator, theme, cx)));
        components::full_size_column().gap(px(12.0)).when_some(self.error.clone(), |panel, error| panel.child(gpui_component::alert::Alert::error("ambiguous-review-error", error))).children(controls).child(body)
    }
}
