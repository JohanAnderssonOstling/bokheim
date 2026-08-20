use std::rc::Rc;

use epub_svelte_lib::ContentHash;
use epub_svelte_lib::library::EditionIdentityReview;
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Render, Window, div, px, uniform_list};
use gpui_component::Disableable;
use gpui_component::input::{Input, InputState};
use ui_components as components;

use crate::library::services::LibraryContext;

use super::super::review_cover::ReviewCoverView;

#[derive(Clone)]
struct NoMatchReviewItem {
    review: EditionIdentityReview,
    input: Entity<InputState>,
    cover: Entity<ReviewCoverView>,
}

pub(super) struct NoMatchReviewPanel {
    ui: LibraryContext,
    items: Rc<Vec<NoMatchReviewItem>>,
    busy: Option<ContentHash>,
    error: Option<String>,
}

impl NoMatchReviewPanel {
    pub(super) fn new(ui: LibraryContext, reviews: Vec<EditionIdentityReview>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = reviews
            .into_iter()
            .map(|review| NoMatchReviewItem { cover: ReviewCoverView::create(review.content_hash.clone(), ui.clone(), cx), input: cx.new(|cx| InputState::new(window, cx).placeholder("ISBN-10 or ISBN-13")), review })
            .collect();
        Self { ui, items: Rc::new(items), busy: None, error: None }
    }

    pub(super) fn count(&self) -> usize {
        self.items.len()
    }

    pub(super) fn add_review(&mut self, review: EditionIdentityReview, window: &mut Window, cx: &mut Context<Self>) {
        let hash = review.content_hash.clone();
        let items = Rc::make_mut(&mut self.items);
        items.retain(|item| item.review.content_hash != hash);
        items.push(NoMatchReviewItem { cover: ReviewCoverView::create(hash, self.ui.clone(), cx), input: cx.new(|cx| InputState::new(window, cx).placeholder("ISBN-10 or ISBN-13")), review });
        self.error = None;
        cx.notify();
    }

    fn remove_review(&mut self, hash: &ContentHash) {
        Rc::make_mut(&mut self.items).retain(|item| &item.review.content_hash != hash);
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
        self.busy = Some(hash.clone());
        self.error = None;
        let operation = self.ui.apply_manual_edition_identity(hash, isbn);
        cx.spawn(async move |panel, cx| {
            let result = operation.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.busy = None;
                match result {
                    Ok(_) => {
                        panel.remove_review(&hash);
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

impl Render for NoMatchReviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let panel = cx.entity();
        let rows = self.items.clone();
        let busy = self.busy.clone();
        let row_panel = panel;
        let list = uniform_list("no-match-review-list", rows.len(), move |range, _, _| {
            range
                .map(|index| {
                    let item = &rows[index];
                    let review = &item.review;
                    let hash = review.content_hash.clone();
                    let cover = item.cover.clone();
                    let input = item.input.clone();
                    let save_panel = row_panel.clone();
                    let save_hash = hash.clone();
                    let is_busy = busy.is_some();
                    let own_busy = busy.as_ref() == Some(&hash);
                    let detail = review.detail.clone().unwrap_or_else(|| "No candidate was returned.".to_owned());
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
                                        .child(div().flex_none().text_color(theme.warning).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::BOLD).child("No match"))
                                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(review.title.clone())),
                                )
                                .child(div().w_full().min_w_0().overflow_hidden().text_color(theme.text_muted).child(detail))
                                .child(
                                    div().w_full().flex().items_end().gap(px(8.0)).child(div().ml_auto().w(px(190.0)).h(px(32.0)).child(Input::new(&input))).child(
                                        components::outlined_text_button(format!("isbn-manual-{hash}"), if own_busy { "Saving…" } else { "Save ISBN" })
                                            .disabled(is_busy)
                                            .on_click(move |_, _, cx| save_panel.update(cx, |panel, cx| panel.save_isbn(save_hash.clone(), cx))),
                                    ),
                                ),
                        )
                })
                .collect::<Vec<_>>()
        });
        let body = if self.items.is_empty() {
            components::browser_inline_note("No unmatched metadata decisions need review.", theme).into_any_element()
        } else {
            components::remaining_space_column().border_1().border_color(theme.border).overflow_hidden().child(components::virtual_list(list)).into_any_element()
        };
        components::full_size_column().gap(px(12.0)).when_some(self.error.clone(), |panel, error| panel.child(gpui_component::alert::Alert::error("no-match-review-error", error))).child(body)
    }
}
