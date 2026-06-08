use crate::style::palette;
use floem::prelude::{button, Decorators, IntoView, View};
use floem::style::CursorStyle;

#[derive(Clone, Copy, Default)]
pub enum ShadButtonVariant {
    #[default]
    Default,
    Outline,
}

pub struct ShadButton<V> {
    child: V,
    variant: ShadButtonVariant,
}

impl<V: IntoView + 'static> ShadButton<V> {
    pub fn new(child: V) -> Self {
        Self { child, variant: ShadButtonVariant::Default }
    }

    pub fn outline(mut self) -> Self {
        self.variant = ShadButtonVariant::Outline;
        self
    }

    pub fn build(self) -> impl View {
        let variant = self.variant;
        button(self.child).style(move |s| {
            let theme = palette();
            let s = s
                .height(40)
                .padding_horiz(16)
                .items_center()
                .justify_center()
                .cursor(CursorStyle::Pointer)
                .font_size(14)
                .border_radius(8)
                .border(1);

            match variant {
                ShadButtonVariant::Default => s
                    .background(theme.ink_text)
                    .color(theme.white_surface)
                    .border_color(theme.ink_text),
                ShadButtonVariant::Outline => s
                    .background(theme.white_surface)
                    .color(theme.ink_text)
                    .border_color(theme.border_color),
            }
        })
    }
}
