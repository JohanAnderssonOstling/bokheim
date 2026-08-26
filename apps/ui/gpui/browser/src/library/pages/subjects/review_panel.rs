mod ambiguous;
mod no_match;
mod raw_subjects;

use app_contract::library::{EditionIdentityReview, EditionIdentityReviewStatus};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, Render, Subscription, Task, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, IconName, Sizable};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::widgets::Paginator;

use ambiguous::AmbiguousLane;
use no_match::NoMatchLane;
pub(crate) use raw_subjects::RawSubjectsLane;

/// Emitted by a lane after its backend mutation succeeds.
///
/// The parent responds by rebuilding every lane from backend truth. Lanes do
/// not move reviews into one another or try to keep sibling counts in sync.
pub(super) struct ReviewsChanged;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewLane {
    Ambiguous,
    NoMatch,
}

struct TriageLanes {
    ambiguous: Entity<AmbiguousLane>,
    no_match: Entity<NoMatchLane>,
    failed_count: usize,
}

enum TriageState {
    Loading,
    Ready(TriageLanes),
    Failed(String),
}

/// Metadata triage surface with decision panels and one batch retry action.
///
/// Each lane owns its distinct controls and action state. This parent owns the
/// shared load lifecycle and treats the backend as the source of truth after
/// every successful decision.
pub(crate) struct MetadataTriage {
    ui: LibraryContext,
    state: TriageState,
    selected_lane: ReviewLane,
    lane_subscriptions: Vec<Subscription>,
    load_task: Option<Task<()>>,
    retrying_failed: bool,
    retry_error: Option<String>,
}

impl MetadataTriage {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let mut triage = Self { ui, state: TriageState::Loading, selected_lane: ReviewLane::Ambiguous, lane_subscriptions: Vec::new(), load_task: None, retrying_failed: false, retry_error: None };
        triage.reload(cx);
        triage
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.state = TriageState::Loading;
        self.lane_subscriptions.clear();
        let ui = self.ui.clone();
        let reviews_load = ui.backend().load_prepared_edition_identity_reviews();
        self.load_task = Some(cx.spawn(async move |triage, cx| {
            let reviews = reviews_load.await;
            let _ = triage.update_in(cx, |triage, window, cx| match reviews {
                Ok(reviews) => triage.install(ui, reviews, window, cx),
                Err(error) => {
                    triage.state = TriageState::Failed(error);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn install(&mut self, ui: LibraryContext, reviews: Vec<EditionIdentityReview>, window: &mut Window, cx: &mut Context<Self>) {
        let mut ambiguous_reviews = Vec::new();
        let mut no_match_reviews = Vec::new();
        let mut failed_count = 0;
        for review in reviews {
            match review.status {
                EditionIdentityReviewStatus::Ambiguous => ambiguous_reviews.push(review),
                EditionIdentityReviewStatus::NoMatch => no_match_reviews.push(review),
                EditionIdentityReviewStatus::Failed => failed_count += 1,
            }
        }

        let ambiguous = cx.new(|cx| AmbiguousLane::new(ui.clone(), ambiguous_reviews, cx));
        let no_match = cx.new(|cx| NoMatchLane::new(ui.clone(), no_match_reviews, window, cx));
        self.lane_subscriptions = vec![cx.subscribe(&ambiguous, |triage, _, _: &ReviewsChanged, cx| triage.reload(cx)), cx.subscribe(&no_match, |triage, _, _: &ReviewsChanged, cx| triage.reload(cx))];
        self.state = TriageState::Ready(TriageLanes { ambiguous, no_match, failed_count });
        cx.notify();
    }

    fn count(&self, lane: ReviewLane, cx: &gpui::App) -> usize {
        let TriageState::Ready(lanes) = &self.state else {
            return 0;
        };
        match lane {
            ReviewLane::Ambiguous => lanes.ambiguous.read(cx).count(),
            ReviewLane::NoMatch => lanes.no_match.read(cx).count(),
        }
    }

    fn selected_lane(&self) -> Option<gpui::AnyElement> {
        let TriageState::Ready(lanes) = &self.state else {
            return None;
        };
        Some(match self.selected_lane {
            ReviewLane::Ambiguous => lanes.ambiguous.clone().into_any_element(),
            ReviewLane::NoMatch => lanes.no_match.clone().into_any_element(),
        })
    }

    fn retry_all_failed(&mut self, cx: &mut Context<Self>) {
        let TriageState::Ready(lanes) = &self.state else {
            return;
        };
        if self.retrying_failed || lanes.failed_count == 0 {
            return;
        }

        self.retrying_failed = true;
        self.retry_error = None;
        let operation = self.ui.backend().retry_failed_edition_identities();
        cx.spawn(async move |triage, cx| {
            let result = operation.await;
            let _ = triage.update(cx, |triage, cx| {
                triage.retrying_failed = false;
                match result {
                    Ok(_) => triage.reload(cx),
                    Err(error) => {
                        triage.retry_error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
}

pub(super) fn review_paginator_controls(id: &'static str, paginator: &Entity<Paginator>, theme: components::BrowserTheme, cx: &gpui::App) -> Div {
    let state = paginator.read(cx);
    let has_previous = state.has_previous();
    let has_next = state.has_next();
    let can_previous = state.can_previous();
    let can_next = state.can_next();
    let previous_paginator = paginator.clone();
    let previous = has_previous.then(|| {
        components::topbar_icon_action_button(format!("{id}-previous"), "Previous", IconName::ChevronLeft, theme)
            .disabled(!can_previous)
            .on_click(move |_, _, cx| {
                previous_paginator.update(cx, |paginator, cx| paginator.previous(cx));
            })
            .into_any_element()
    });
    let next_paginator = paginator.clone();
    let next = has_next.then(|| {
        components::topbar_icon_action_button(format!("{id}-next"), "Next", IconName::ChevronRight, theme)
            .disabled(!can_next)
            .on_click(move |_, _, cx| {
                next_paginator.update(cx, |paginator, cx| paginator.next(cx));
            })
            .into_any_element()
    });
    components::paginator_controls(previous, next)
}

impl Render for MetadataTriage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        match &self.state {
            TriageState::Loading => return components::browser_inline_note("Loading metadata reviews…", theme).into_any_element(),
            TriageState::Failed(error) => return gpui_component::alert::Alert::error(format!("metadata-triage-load-error-{}", cx.entity_id()), error.clone()).into_any_element(),
            TriageState::Ready(_) => {}
        }

        let triage = cx.entity();
        let lanes = [(ReviewLane::Ambiguous, "Ambiguous"), (ReviewLane::NoMatch, "No match")];
        let mut tabs = div().w_full().flex().gap(px(1.0)).border_1().border_color(theme.border).bg(theme.border);
        for (lane, label) in lanes {
            let count = self.count(lane, cx);
            let selected = self.selected_lane == lane;
            let tab_triage = triage.clone();
            tabs = tabs.child(
                Button::new(format!("metadata-triage-tab-{label}"))
                    .label(format!("{label}  {count}"))
                    .ghost()
                    .flex_1()
                    .h(px(38.0))
                    .bg(if selected { theme.accent_soft } else { theme.surface })
                    .text_color(if selected { theme.text } else { theme.text_muted })
                    .on_click(move |_, _, cx| {
                        tab_triage.update(cx, |triage, cx| {
                            triage.selected_lane = lane;
                            cx.notify();
                        })
                    }),
            );
        }

        let description = match self.selected_lane {
            ReviewLane::Ambiguous => "Compare your metadata with each candidate, then choose the matching record or reject them.",
            ReviewLane::NoMatch => "Enter a known ISBN directly for books with no Open Library match.",
        };
        let failed_count = match &self.state {
            TriageState::Ready(lanes) => lanes.failed_count,
            _ => 0,
        };
        let retry_triage = triage.clone();
        let retry_failed = (failed_count > 0).then(|| {
            Button::new("retry-all-failed-metadata-reviews")
                .label(if self.retrying_failed { "Retrying…".to_owned() } else { format!("Retry all failed ({failed_count})") })
                .primary()
                .small()
                .disabled(self.retrying_failed)
                .on_click(move |_, _, cx| retry_triage.update(cx, |triage, cx| triage.retry_all_failed(cx)))
        });
        let header = div()
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
                    .gap(px(12.0))
                    .child(div().flex().flex_col().gap(px(4.0)).child(div().text_size(gpui::rems(17.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child("Metadata review")).child(div().text_color(theme.text_muted).child(description)))
                    .children(retry_failed),
            )
            .child(tabs);

        components::full_size_column().gap(px(12.0)).child(header).children(self.retry_error.clone().map(|error| gpui_component::alert::Alert::error("metadata-review-retry-error", error))).children(self.selected_lane()).into_any_element()
    }
}
