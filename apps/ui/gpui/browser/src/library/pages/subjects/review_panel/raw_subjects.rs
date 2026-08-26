use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Render, Subscription, Window, div, px, rems};
use ui_components as components;

use crate::library::widgets::{Paginator, PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};

use super::review_paginator_controls;

const ROW_HEIGHT: f32 = 64.0;
const ROW_HORIZONTAL_PADDING: f32 = 12.0;

fn paginator_policy() -> PaginatorGroupPolicy {
    PaginatorGroupPolicy::new(
        PaginatorWidthPolicy::new(rems(18.0), rems(24.0), rems(32.0)).with_pixels(px(ROW_HORIZONTAL_PADDING * 2.0)),
        PaginatorSizing::pixels(px(ROW_HEIGHT)),
        PaginatorGapPolicy::new(px(components::SPACE_SM), px(components::SPACE_MD)),
        PaginatorGapPolicy::new(px(6.0), px(components::SPACE_SM)),
    )
}

pub(crate) struct RawSubjectsLane {
    subjects: Rc<Vec<String>>,
    paginator: Entity<Paginator>,
    _paginator_updates: Subscription,
}

impl RawSubjectsLane {
    pub(crate) fn new(subjects: Vec<String>, cx: &mut Context<Self>) -> Self {
        let subjects = Rc::new(subjects);
        let children = subjects
            .iter()
            .cloned()
            .map(|subject| {
                PaginatorChild::new(move |_, cx| {
                    let theme = components::browser_theme(cx);
                    div()
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px(px(ROW_HORIZONTAL_PADDING))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.page_bg)
                        .flex()
                        .items_center()
                        .font_weight(FontWeight::MEDIUM)
                        .line_clamp(2)
                        .child(subject.clone())
                        .into_any_element()
                })
            })
            .collect::<Vec<_>>();
        let paginator = cx.new(|_| Paginator::new("raw-subjects-paginator", [PaginatorGroup::new(paginator_policy(), children)]));
        let paginator_updates = cx.observe(&paginator, |_, _, cx| cx.notify());
        Self { subjects, paginator, _paginator_updates: paginator_updates }
    }
}

impl Render for RawSubjectsLane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let body = if self.subjects.is_empty() {
            components::browser_inline_note("Every raw subject fits the unified taxonomy.", theme).into_any_element()
        } else {
            components::remaining_space_column().overflow_hidden().child(self.paginator.clone()).into_any_element()
        };
        let controls = (!self.subjects.is_empty() && !cx.supports_touch_input()).then(|| div().w_full().flex().justify_end().child(review_paginator_controls("raw-subjects", &self.paginator, theme, cx)));
        components::full_size_column().gap(px(12.0)).children(controls).child(body)
    }
}
