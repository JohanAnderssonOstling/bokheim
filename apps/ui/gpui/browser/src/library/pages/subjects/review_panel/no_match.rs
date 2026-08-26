use std::rc::Rc;

use app_contract::library::EditionIdentityReview;
use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, Render, Subscription, Window, div, px, rems};
use gpui_component::Disableable;
use gpui_component::input::{Input, InputState};
use sync_common::ContentHash;
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::widgets::{Paginator, PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};

use super::super::review_cover::ReviewCoverView;
use super::{ReviewsChanged, review_paginator_controls};

const ROW_HORIZONTAL_PADDING: f32 = 10.0;
const COVER_CONTENT_GAP: f32 = 10.0;
const MINIMUM_TEXT_WIDTH_REM: f32 = 20.0;
const PREFERRED_TEXT_WIDTH_REM: f32 = 26.0;
const MAXIMUM_TEXT_WIDTH_REM: f32 = 34.0;

fn paginator_policy() -> PaginatorGroupPolicy {
    let fixed_width = components::BOOK_COVER_WIDTH + ROW_HORIZONTAL_PADDING * 2.0 + COVER_CONTENT_GAP;
    PaginatorGroupPolicy::new(
        PaginatorWidthPolicy::new(rems(MINIMUM_TEXT_WIDTH_REM), rems(PREFERRED_TEXT_WIDTH_REM), rems(MAXIMUM_TEXT_WIDTH_REM)).with_pixels(px(fixed_width)),
        PaginatorSizing::pixels(px(app_contract::METADATA_REVIEW_ITEM_HEIGHT_PX as f32)),
        PaginatorGapPolicy::new(px(components::SPACE_SM), px(components::SPACE_MD)),
        PaginatorGapPolicy::new(px(6.0), px(components::SPACE_SM)),
    )
}

#[derive(Clone)]
struct NoMatchReviewItem {
    review: EditionIdentityReview,
    input: Entity<InputState>,
    cover: Entity<ReviewCoverView>,
}

pub(super) struct NoMatchLane {
    ui: LibraryContext,
    items: Rc<Vec<NoMatchReviewItem>>,
    paginator: Entity<Paginator>,
    _paginator_updates: Subscription,
    busy: Option<ContentHash>,
    error: Option<String>,
}

impl gpui::EventEmitter<ReviewsChanged> for NoMatchLane {}

impl NoMatchLane {
    pub(super) fn new(ui: LibraryContext, reviews: Vec<EditionIdentityReview>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = Rc::new(
            reviews
                .into_iter()
                .map(|review| NoMatchReviewItem { cover: ReviewCoverView::create(review.content_hash, ui.clone(), cx), input: cx.new(|cx| InputState::new(window, cx).placeholder("ISBN-10 or ISBN-13")), review })
                .collect::<Vec<_>>(),
        );
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
                    review_row(item, state.busy, &lane, components::browser_theme(cx)).into_any_element()
                })
            })
            .collect::<Vec<_>>();
        let paginator = cx.new(|_| Paginator::new("no-match-review-paginator", [PaginatorGroup::new(paginator_policy(), children)]));
        let paginator_updates = cx.observe(&paginator, |_, _, cx| cx.notify());
        Self { ui, items, paginator, _paginator_updates: paginator_updates, busy: None, error: None }
    }

    pub(super) fn count(&self) -> usize {
        self.items.len()
    }

    fn save_isbn(&mut self, hash: ContentHash, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(input) = self.items.iter().find(|item| item.review.content_hash == hash).map(|item| &item.input) else {
            return;
        };
        let isbn = input.read(cx).value().trim().to_owned();
        if isbn.is_empty() {
            self.error = Some("Enter an ISBN-10 or ISBN-13 before saving.".to_owned());
            cx.notify();
            return;
        }
        self.busy = Some(hash);
        self.error = None;
        self.paginator.update(cx, |_, cx| cx.notify());
        let operation = self.ui.backend().apply_manual_edition_identity(hash, isbn);
        cx.spawn(async move |panel, cx| {
            let result = operation.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.busy = None;
                panel.paginator.update(cx, |_, cx| cx.notify());
                match result {
                    Ok(_) => {
                        panel.error = None;
                        cx.emit(ReviewsChanged);
                        cx.notify();
                    }
                    Err(error) => {
                        panel.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
}

fn review_row(item: &NoMatchReviewItem, busy: Option<ContentHash>, panel: &Entity<NoMatchLane>, theme: components::BrowserTheme) -> Div {
    let review = &item.review;
    let hash = review.content_hash;
    let save_panel = panel.clone();
    let is_busy = busy.is_some();
    let own_busy = busy.as_ref() == Some(&hash);
    let detail = review.detail.clone().unwrap_or_else(|| "No candidate was returned.".to_owned());
    div()
        .w_full()
        .h(px(app_contract::METADATA_REVIEW_ITEM_HEIGHT_PX as f32))
        .px(px(ROW_HORIZONTAL_PADDING))
        .py(px(app_contract::DETAILED_ROW_VERTICAL_PADDING_PX as f32))
        .border_1()
        .border_color(theme.border)
        .bg(theme.page_bg)
        .flex()
        .items_start()
        .gap(px(COVER_CONTENT_GAP))
        .child(div().w(px(components::BOOK_COVER_WIDTH)).h(px(components::BOOK_COVER_HEIGHT)).flex_none().child(item.cover.clone()))
        .child(
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
                        .child(div().flex_none().text_color(theme.warning).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child("No match"))
                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(review.title.clone())),
                )
                .child(div().w_full().min_w_0().overflow_hidden().text_color(theme.text_muted).child(detail))
                .child(div().w_full().flex().items_end().gap(px(8.0)).child(div().ml_auto().w(px(190.0)).h(px(32.0)).child(Input::new(&item.input))).child(
                    components::outlined_text_button(format!("isbn-manual-{hash}"), if own_busy { "Saving…" } else { "Save ISBN" }).disabled(is_busy).on_click(move |_, _, cx| save_panel.update(cx, |panel, cx| panel.save_isbn(hash, cx))),
                )),
        )
}

impl Render for NoMatchLane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let body = if self.items.is_empty() {
            components::browser_inline_note("No unmatched metadata decisions need review.", theme).into_any_element()
        } else {
            components::remaining_space_column().overflow_hidden().child(self.paginator.clone()).into_any_element()
        };
        let controls = (!self.items.is_empty() && !cx.supports_touch_input()).then(|| div().w_full().flex().justify_end().child(review_paginator_controls("no-match-review", &self.paginator, theme, cx)));
        components::full_size_column().gap(px(12.0)).when_some(self.error.clone(), |panel, error| panel.child(gpui_component::alert::Alert::error("no-match-review-error", error))).children(controls).child(body)
    }
}
