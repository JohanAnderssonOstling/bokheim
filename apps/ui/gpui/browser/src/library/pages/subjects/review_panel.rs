mod ambiguous;
mod failed;
mod no_match;

use epub_svelte_lib::library::{EditionIdentityReview, EditionIdentityReviewStatus};
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Render, Subscription, Task, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use ui_components as components;

use crate::library::services::LibraryContext;

use ambiguous::AmbiguousReviewPanel;
use failed::FailedReviewPanel;
use no_match::NoMatchReviewPanel;

#[derive(Clone)]
pub(super) struct ReviewRejected(pub(super) EditionIdentityReview);

enum ReviewPanelState {
    Loading,
    Ready,
    Failed(String),
}

/// Parent review surface: one startup load and status switching.
///
/// Status-specific data, controls, mutations, and rendering live in the three
/// child panel entities. Successful mutations update those local collections;
/// the parent never reloads review data while it is alive.
pub(super) struct SubjectReviewPanel {
    state: ReviewPanelState,
    status: EditionIdentityReviewStatus,
    ambiguous: Option<Entity<AmbiguousReviewPanel>>,
    no_match: Option<Entity<NoMatchReviewPanel>>,
    failed: Option<Entity<FailedReviewPanel>>,
    _child_subscriptions: Vec<Subscription>,
    _load_task: Task<()>,
}

impl SubjectReviewPanel {
    pub(super) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let load = ui.load_prepared_edition_identity_reviews();
        let load_task = cx.spawn(async move |panel, cx| {
            let result = load.await;
            let _ = panel.update_in(cx, |panel, window, cx| match result {
                Ok(reviews) => panel.install(ui, reviews, window, cx),
                Err(error) => {
                    panel.state = ReviewPanelState::Failed(error);
                    cx.notify();
                }
            });
        });
        Self { state: ReviewPanelState::Loading, status: EditionIdentityReviewStatus::Ambiguous, ambiguous: None, no_match: None, failed: None, _child_subscriptions: Vec::new(), _load_task: load_task }
    }

    fn install(&mut self, ui: LibraryContext, reviews: Vec<EditionIdentityReview>, window: &mut Window, cx: &mut Context<Self>) {
        let mut ambiguous_reviews = Vec::new();
        let mut no_match_reviews = Vec::new();
        let mut failed_reviews = Vec::new();
        for review in reviews {
            match review.status {
                EditionIdentityReviewStatus::Ambiguous => ambiguous_reviews.push(review),
                EditionIdentityReviewStatus::NoMatch => no_match_reviews.push(review),
                EditionIdentityReviewStatus::Failed => failed_reviews.push(review),
            }
        }

        let ambiguous = cx.new(|cx| AmbiguousReviewPanel::new(ui.clone(), ambiguous_reviews, cx));
        let no_match = cx.new(|cx| NoMatchReviewPanel::new(ui.clone(), no_match_reviews, window, cx));
        let failed = cx.new(|cx| FailedReviewPanel::new(ui, failed_reviews, cx));

        let no_match_for_ambiguous = no_match.clone();
        let subscriptions = vec![
            cx.subscribe_in(&ambiguous, window, move |_, _, event: &ReviewRejected, window, cx| {
                no_match_for_ambiguous.update(cx, |panel, cx| {
                    panel.add_review(event.0.clone(), window, cx);
                });
            }),
            cx.observe(&ambiguous, |_, _, cx| cx.notify()),
            cx.observe(&no_match, |_, _, cx| cx.notify()),
            cx.observe(&failed, |_, _, cx| cx.notify()),
        ];
        self.ambiguous = Some(ambiguous);
        self.no_match = Some(no_match);
        self.failed = Some(failed);
        self._child_subscriptions = subscriptions;
        self.state = ReviewPanelState::Ready;
        cx.notify();
    }

    fn count(&self, status: EditionIdentityReviewStatus, cx: &gpui::App) -> usize {
        match status {
            EditionIdentityReviewStatus::Ambiguous => self.ambiguous.as_ref().map_or(0, |panel| panel.read(cx).count()),
            EditionIdentityReviewStatus::NoMatch => self.no_match.as_ref().map_or(0, |panel| panel.read(cx).count()),
            EditionIdentityReviewStatus::Failed => self.failed.as_ref().map_or(0, |panel| panel.read(cx).count()),
        }
    }

    fn selected_panel(&self) -> Option<gpui::AnyElement> {
        match self.status {
            EditionIdentityReviewStatus::Ambiguous => self.ambiguous.clone().map(IntoElement::into_any_element),
            EditionIdentityReviewStatus::NoMatch => self.no_match.clone().map(IntoElement::into_any_element),
            EditionIdentityReviewStatus::Failed => self.failed.clone().map(IntoElement::into_any_element),
        }
    }
}

impl Render for SubjectReviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        match &self.state {
            ReviewPanelState::Loading => return components::browser_inline_note("Loading metadata reviews…", theme).into_any_element(),
            ReviewPanelState::Failed(error) => return gpui_component::alert::Alert::error(format!("subject-review-load-error-{}", cx.entity_id()), error.clone()).into_any_element(),
            ReviewPanelState::Ready => {}
        }
        let panel = cx.entity();
        let statuses = [(EditionIdentityReviewStatus::Ambiguous, "Ambiguous"), (EditionIdentityReviewStatus::NoMatch, "No match"), (EditionIdentityReviewStatus::Failed, "Failed")];
        let mut tabs = div().w_full().flex().gap(px(1.0)).border_1().border_color(theme.border).bg(theme.border);
        for (status, label) in statuses {
            let count = self.count(status, cx);
            let selected = self.status == status;
            let tab_panel = panel.clone();
            tabs = tabs.child(
                Button::new(format!("isbn-review-tab-{label}"))
                    .label(format!("{label}  {count}"))
                    .ghost()
                    .flex_1()
                    .h(px(38.0))
                    .bg(if selected { theme.accent_soft } else { theme.surface })
                    .text_color(if selected { theme.text } else { theme.text_muted })
                    .on_click(move |_, _, cx| {
                        tab_panel.update(cx, |panel, cx| {
                            panel.status = status;
                            cx.notify();
                        })
                    }),
            );
        }

        let description = match self.status {
            EditionIdentityReviewStatus::Ambiguous => "Compare your metadata with each candidate, then choose the matching record or reject them.",
            EditionIdentityReviewStatus::NoMatch => "Enter a known ISBN directly for books with no Open Library match.",
            EditionIdentityReviewStatus::Failed => "Retry all failed metadata lookups as one batch.",
        };
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
            .child(div().flex().flex_col().gap(px(4.0)).child(div().text_size(gpui::rems(17.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child("Metadata review")).child(div().text_color(theme.text_muted).child(description)))
            .child(tabs);

        components::full_size_column().gap(px(12.0)).child(header).children(self.selected_panel()).into_any_element()
    }
}
