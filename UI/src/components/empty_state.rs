use crate::components::text::subtle_label;
use crate::components::tokens::TOKENS;
use floem::prelude::{container, label, Decorators, Label};

pub fn inline_message(text: &'static str) -> impl floem::View {
    container(subtle_label(move || text.to_string()).style(|s| s.padding_horiz(TOKENS.spacing.sm).padding_vert(TOKENS.spacing.xs))).style(|s| s.width_full())
}

/// A zero-sized placeholder for branches that must render a view but show nothing.
pub fn empty_view() -> Label {
    label(|| "").style(|s| s.width(0).height(0).padding(0))
}
