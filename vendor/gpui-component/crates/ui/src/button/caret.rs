use crate::{Icon, IconName, Sizable, Size};
use gpui::{App, Hsla, IntoElement, RenderOnce, Styled, Window, prelude::FluentBuilder};

#[derive(IntoElement)]
pub struct Caret {
    size: Size,
    color: Option<Hsla>,
}

impl Caret {
    /// Create a dropdown caret sized for its trigger.
    pub fn new(size: Size) -> Self {
        Self { size, color: None }
    }

    /// Set the caret color.
    pub fn text_color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Caret {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        Icon::new(IconName::ChevronDown)
            .with_size(match self.size {
                Size::XSmall => Size::XSmall,
                Size::Small => Size::Small,
                _ => Size::Medium,
            })
            .when_some(self.color, |this, color| this.text_color(color))
    }
}
