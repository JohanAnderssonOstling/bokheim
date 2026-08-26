//! Interactive visual harness for the custom paginator.
//!
//! Run with: `cargo run -p browser-ui --example paginator_visual`

#[path = "../src/library/widgets/paginator/mod.rs"]
mod paginator;

use gpui::prelude::*;
use gpui::{AnyElement, App, Bounds, Entity, Render, Subscription, Window, WindowBounds, WindowOptions, div, px, rems, rgb, size};

use paginator::{Paginator, PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorStages, PaginatorWidthPolicy};

const COLORS: [u32; 6] = [0x35618c, 0x5f8462, 0xa66d47, 0x76588e, 0x3d8380, 0x9a5661];

struct PaginatorVisual {
    paginator: Entity<Paginator>,
    _paginator_updates: Subscription,
}

fn stage_button(id: &'static str, label: &'static str, active: bool, paginator: Entity<Paginator>, update: fn(&mut PaginatorStages)) -> AnyElement {
    div()
        .id(id)
        .px(px(10.0))
        .py(px(6.0))
        .border_1()
        .border_color(if active { rgb(0x274c70) } else { rgb(0xb8b0a2) })
        .bg(if active { rgb(0x35618c) } else { rgb(0xe8e3d8) })
        .text_color(if active { rgb(0xffffff) } else { rgb(0x26231f) })
        .cursor_pointer()
        .child(label)
        .on_click(move |_, _, cx| {
            paginator.update(cx, |paginator, cx| {
                let mut stages = paginator.stages();
                update(&mut stages);
                paginator.set_stages(stages, cx);
            });
        })
        .into_any_element()
}

impl Render for PaginatorVisual {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let stages = self.paginator.read(cx).stages();
        let previous_paginator = self.paginator.clone();
        let previous = div().id("paginator-previous").px(px(14.0)).py(px(8.0)).border_1().border_color(rgb(0xb8b0a2)).cursor_pointer().child("Previous").on_click(move |_, _, cx| {
            previous_paginator.update(cx, |paginator, cx| paginator.previous(cx));
        });
        let next_paginator = self.paginator.clone();
        let next = div().id("paginator-next").px(px(14.0)).py(px(8.0)).border_1().border_color(rgb(0xb8b0a2)).cursor_pointer().child("Next").on_click(move |_, _, cx| {
            next_paginator.update(cx, |paginator, cx| paginator.next(cx));
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(24.0))
            .gap(px(16.0))
            .bg(rgb(0xf4f1e9))
            .text_color(rgb(0x26231f))
            .child(div().flex_none().flex().items_center().gap(px(10.0)).child(div().flex_1().text_size(px(20.0)).child("Entity-backed paginator")).child(previous).child(next))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().mr(px(4.0)).child("Optional stages:"))
                    .child(stage_button("paginator-stages-baseline", "Baseline", stages == PaginatorStages::baseline(), self.paginator.clone(), |stages| *stages = PaginatorStages::baseline()))
                    .child(stage_button("paginator-stage-expand", "1 · Min gaps + expand", stages.expand_widths, self.paginator.clone(), |stages| stages.expand_widths = !stages.expand_widths))
                    .child(stage_button("paginator-stage-ranks", "2 · Repair ranks", stages.repair_ranks, self.paginator.clone(), |stages| stages.repair_ranks = !stages.repair_ranks))
                    .child(stage_button("paginator-stage-row", "3 · Intrinsic + fill rows", stages.fill_additional_rows, self.paginator.clone(), |stages| stages.fill_additional_rows = !stages.fill_additional_rows))
                    .child(stage_button("paginator-stage-gaps", "4 · Max gaps", stages.distribute_gaps, self.paginator.clone(), |stages| stages.distribute_gaps = !stages.distribute_gaps))
                    .child(stage_button("paginator-stages-all", "All", stages == PaginatorStages::default(), self.paginator.clone(), |stages| *stages = PaginatorStages::default())),
            )
            .child(div().flex_1().min_h_0().border_1().border_color(rgb(0xb8b0a2)).child(self.paginator.clone()))
    }
}

fn visual_group(policy: PaginatorGroupPolicy, group_index: usize, intrinsic_heights: [f32; 3], indices: impl IntoIterator<Item = usize>) -> PaginatorGroup {
    let color = COLORS[group_index % COLORS.len()];
    PaginatorGroup::new(
        policy,
        indices.into_iter().map(|index| {
            let intrinsic_height = intrinsic_heights[index % intrinsic_heights.len()];
            PaginatorChild::new(move |_, _| {
                div()
                    .w_full()
                    .h(px(intrinsic_height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(color))
                    .border_1()
                    .border_color(rgb(0x24211d))
                    .text_color(rgb(0xffffff))
                    .text_size(px(22.0))
                    .child(format!("G{} · {index} · {intrinsic_height}px", group_index + 1))
                    .into_any_element()
            })
        }),
    )
}

fn visual_groups() -> Vec<PaginatorGroup> {
    vec![
        visual_group(
            PaginatorGroupPolicy::new(
                PaginatorWidthPolicy::new(rems(5.0), rems(10.0), rems(18.0)),
                PaginatorSizing::aspect_ratio(1.8),
                PaginatorGapPolicy::new(px(2.0), px(8.0)).with_maximum(px(48.0)),
                PaginatorGapPolicy::new(px(2.0), px(8.0)),
            ),
            0,
            [56.0, 72.0, 84.0],
            0..5,
        ),
        visual_group(
            PaginatorGroupPolicy::new(
                PaginatorWidthPolicy::new(rems(4.0), rems(7.0), rems(12.0)),
                PaginatorSizing::aspect_ratio(0.7),
                PaginatorGapPolicy::new(px(4.0), px(12.0)).with_maximum(px(48.0)),
                PaginatorGapPolicy::new(px(4.0), px(12.0)),
            ),
            1,
            [96.0, 128.0, 152.0],
            5..12,
        ),
        visual_group(
            PaginatorGroupPolicy::new(
                PaginatorWidthPolicy::new(rems(6.0), rems(8.0), rems(14.0)),
                PaginatorSizing::aspect_ratio(1.0),
                PaginatorGapPolicy::new(px(2.0), px(6.0)).with_maximum(px(48.0)),
                PaginatorGapPolicy::new(px(2.0), px(6.0)),
            ),
            2,
            [64.0, 92.0, 120.0],
            12..19,
        ),
        visual_group(
            PaginatorGroupPolicy::new(
                PaginatorWidthPolicy::new(rems(5.0), rems(9.0), rems(16.0)),
                PaginatorSizing::rems(rems(7.0)),
                PaginatorGapPolicy::new(px(4.0), px(10.0)).with_maximum(px(48.0)),
                PaginatorGapPolicy::new(px(4.0), px(10.0)),
            ),
            3,
            [48.0, 76.0, 104.0],
            19..28,
        ),
        visual_group(
            PaginatorGroupPolicy::new(
                PaginatorWidthPolicy::new(rems(4.0), rems(6.0), rems(11.0)),
                PaginatorSizing::pixels(px(112.0)),
                PaginatorGapPolicy::new(px(2.0), px(12.0)).with_maximum(px(48.0)),
                PaginatorGapPolicy::new(px(2.0), px(12.0)),
            ),
            4,
            [60.0, 84.0, 108.0],
            28..40,
        ),
    ]
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1400.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: Some(gpui::TitlebarOptions { title: Some("Paginator visual test".into()), ..Default::default() }), focus: true, ..Default::default() },
            |_, cx| {
                cx.new(|cx| {
                    let paginator = cx.new(|_| Paginator::new("paginator-visual-grid", visual_groups()));
                    let paginator_updates = cx.observe(&paginator, |_, _, cx| cx.notify());
                    PaginatorVisual { paginator, _paginator_updates: paginator_updates }
                })
            },
        )
        .expect("open paginator visual window");
        cx.activate(true);
    });
}
