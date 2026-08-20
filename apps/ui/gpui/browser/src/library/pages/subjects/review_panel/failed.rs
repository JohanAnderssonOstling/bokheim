use std::rc::Rc;

use epub_svelte_lib::library::EditionIdentityReview;
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Render, Window, div, px, uniform_list};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Sizable};
use ui_components as components;

use crate::library::services::LibraryContext;

use super::super::review_cover::ReviewCoverView;

#[derive(Clone)]
struct FailedReviewItem {
    review: EditionIdentityReview,
    cover: Entity<ReviewCoverView>,
}

pub(super) struct FailedReviewPanel {
    ui: LibraryContext,
    items: Rc<Vec<FailedReviewItem>>,
    updating_all: bool,
    error: Option<String>,
}

impl FailedReviewPanel {
    pub(super) fn new(ui: LibraryContext, reviews: Vec<EditionIdentityReview>, cx: &mut Context<Self>) -> Self {
        let items = reviews.into_iter().map(|review| FailedReviewItem { cover: ReviewCoverView::create(review.content_hash.clone(), ui.clone(), cx), review }).collect();
        Self { ui, items: Rc::new(items), updating_all: false, error: None }
    }

    pub(super) fn count(&self) -> usize {
        self.items.len()
    }

    fn update_all(&mut self, cx: &mut Context<Self>) {
        if self.updating_all || self.items.is_empty() {
            return;
        }
        self.updating_all = true;
        self.error = None;
        let operation = self.ui.retry_failed_edition_identities();
        cx.spawn(async move |panel, cx| {
            let result = operation.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.updating_all = false;
                match result {
                    Ok(_) => {
                        panel.items = Rc::new(Vec::new());
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

impl Render for FailedReviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let panel = cx.entity();
        let rows = self.items.clone();
        let list = uniform_list("failed-review-list", rows.len(), move |range, _, _| {
            range
                .map(|index| {
                    let item = &rows[index];
                    let review = &item.review;
                    let hash = review.content_hash.clone();
                    let cover = item.cover.clone();
                    let detail = review.detail.clone().unwrap_or_else(|| "The metadata lookup failed without an error message.".to_owned());
                    div()
                        .w_full()
                        .h(px(app_contract::METADATA_REVIEW_ITEM_HEIGHT_PX as f32))
                        .px(px(10.0))
                        .py(px(app_contract::DETAILED_ROW_VERTICAL_PADDING_PX as f32))
                        .mb(px(6.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.page_bg)
                        .flex()
                        .items_start()
                        .gap(px(10.0))
                        .child(cover)
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
                                        .child(div().flex_none().text_color(theme.danger).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::BOLD).child("Failed"))
                                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(review.title.clone())),
                                )
                                .child(div().w_full().min_w_0().overflow_hidden().text_color(theme.danger).child(detail)),
                        )
                })
                .collect::<Vec<_>>()
        });
        let update_panel = panel;
        let update_action = Button::new("update-all-failed-isbn-reviews")
            .label(if self.updating_all { "Updating…" } else { "Update all" })
            .primary()
            .small()
            .disabled(self.updating_all || self.items.is_empty())
            .on_click(move |_, _, cx| update_panel.update(cx, |panel, cx| panel.update_all(cx)));
        let body = if self.items.is_empty() {
            components::browser_inline_note("No failed metadata lookups need review.", theme).into_any_element()
        } else {
            components::remaining_space_column().border_1().border_color(theme.border).overflow_hidden().child(components::virtual_list(list)).into_any_element()
        };
        components::full_size_column()
            .gap(px(12.0))
            .child(div().w_full().flex().justify_end().child(update_action))
            .when_some(self.error.clone(), |panel, error| panel.child(gpui_component::alert::Alert::error("failed-review-error", error)))
            .child(body)
    }
}
