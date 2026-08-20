//! Interactive visual harness for the custom paginator.
//!
//! Run with: `cargo run -p browser-ui --example paginator_visual`

#[path = "../src/library/pages/browse_next/paginator.rs"]
mod paginator;

use gpui::prelude::*;
use gpui::{App, Bounds, Render, Window, WindowBounds, WindowOptions, div, px, rems, rgb, size};

use paginator::{Paginator, PaginatorChild, PaginatorSizing};

const ANCHOR: usize = 12;
const COLORS: [u32; 6] = [0x35618c, 0x5f8462, 0xa66d47, 0x76588e, 0x3d8380, 0x9a5661];

struct PaginatorVisual {
    child_offset: usize,
}

impl Render for PaginatorVisual {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut paginator = Paginator::new("paginator-visual-grid", rems(9.0), px(12.0), visual_children());
        paginator.set_child_offset(self.child_offset);
        let child_offset = paginator.child_offset();

        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(24.0))
            .gap(px(16.0))
            .bg(rgb(0xf4f1e9))
            .text_color(rgb(0x26231f))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(20.0))
                            .child(format!("Paginator slot layout · child {child_offset}")),
                    )
                    .child(
                        div()
                            .id("paginator-previous")
                            .px(px(14.0))
                            .py(px(8.0))
                            .border_1()
                            .border_color(rgb(0xb8b0a2))
                            .cursor_pointer()
                            .child("Previous child")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.child_offset = this.child_offset.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("paginator-next")
                            .px(px(14.0))
                            .py(px(8.0))
                            .border_1()
                            .border_color(rgb(0xb8b0a2))
                            .cursor_pointer()
                            .child("Next child")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.child_offset = (this.child_offset + 1).min(23);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(rgb(0xb8b0a2))
                    .child(paginator),
            )
    }
}

fn visual_children() -> Vec<PaginatorChild> {
    (0..24)
        .map(|index| {
            let sizing = if index < 4 {
                PaginatorSizing::AspectRatio(1.6)
            } else {
                PaginatorSizing::AspectRatio(1.0 / 1.6)
            };
            PaginatorChild::new(sizing, move |_, _| {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(COLORS[index % COLORS.len()]))
                    .border_1()
                    .border_color(rgb(0x24211d))
                    .text_color(rgb(0xffffff))
                    .text_size(px(22.0))
                    .child(index.to_string())
                    .into_any_element()
            })
        })
        .collect()
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1400.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Paginator visual test".into()),
                    ..Default::default()
                }),
                focus: true,
                ..Default::default()
            },
            |_, cx| cx.new(|_| PaginatorVisual { child_offset: ANCHOR }),
        )
        .expect("open paginator visual window");
        cx.activate(true);
    });
}
