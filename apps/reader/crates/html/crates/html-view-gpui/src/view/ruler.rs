//! Reading guides use the renderer's line geometry without owning selection.

use super::HtmlView;
use gpui::{Bounds, ContentMask, Context, Hsla, Pixels, Window, fill, hsla, point, px, size};
use html_gpui::backend::GpuiTextCache;
use html_view_core::{
    RendererSession,
    cfi::{generate_cfi, parse_cfi},
};
use kurbo::{Point, Rect, Size};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReadingRulerStyle {
    Band,
    Focus,
    Underline,
    #[default]
    #[serde(other)]
    Off,
}

#[derive(Clone, Default)]
pub(super) struct ReadingRuler {
    style: ReadingRulerStyle,
    line_count: usize,
    anchor: Option<String>,
    glyph: Option<(usize, u32)>,
    /// The anchor is the last line of a group reached by moving backwards.
    from_end: bool,
    edge: Option<bool>,
    navigation: u64,
}

impl ReadingRuler {
    fn index(&self, renderer: &RendererSession<GpuiTextCache>, lines: &[(Range<u32>, Rect)]) -> Option<usize> {
        if lines.is_empty() {
            return None;
        }
        if let Some(forward) = self.edge {
            return Some(if forward { 0 } else { lines.len() - 1 });
        }
        let glyph = self
            .anchor
            .as_deref()
            .and_then(|cfi| parse_cfi(renderer.document(), cfi))
            .filter(|(document, _)| Some(*document) == renderer.reading_document())
            .or(self.glyph.filter(|(document, _)| Some(*document) == renderer.reading_document()))
            .map(|(_, glyph)| glyph);
        Some(glyph.and_then(|glyph| lines.iter().position(|(range, _)| range.contains(&glyph))).unwrap_or(0))
    }
}

impl HtmlView {
    pub fn set_reading_ruler(&mut self, style: ReadingRulerStyle, lines: usize, cx: &mut Context<Self>) {
        let lines = lines.clamp(1, 5);
        if self.ruler.style != style || self.ruler.line_count != lines {
            self.ruler.style = style;
            self.ruler.line_count = lines;
            cx.notify();
        }
    }

    pub(super) fn reset_ruler(&mut self, forward: bool) {
        self.ruler.anchor = None;
        self.ruler.glyph = None;
        self.ruler.edge = Some(forward);
        self.ruler.from_end = !forward;
        self.ruler.navigation = self.renderer.navigation_revision();
    }

    pub(super) fn ruler_page_turn_completed(&mut self, forward: bool, moved: bool) {
        if moved {
            self.reset_ruler(forward);
        }
        // Even a rejected page turn updates the session's navigation revision.
        self.ruler.navigation = self.renderer.navigation_revision();
    }

    /// Returns false only when the ruler is disabled. At a page boundary the
    /// same input turns the page and resumes at its first/last readable line.
    pub fn step_reading_ruler(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if self.ruler.style == ReadingRulerStyle::Off {
            return false;
        }
        if self.renderer.reading_document().is_none() {
            return true;
        }
        self.sync_ruler_navigation();
        let lines = self.renderer.visible_reading_lines();
        let index = self.ruler.index(&self.renderer, &lines);
        let next = index.and_then(|index| {
            let group = line_group(&lines, index, self.ruler.line_count, self.ruler.from_end);
            if forward { (group.end < lines.len()).then_some(group.end) } else { group.start.checked_sub(1) }
        });
        if let Some(next) = next {
            self.ruler.from_end = !forward;
            self.anchor_ruler(&lines[next].0);
        } else {
            self.turn_page(forward, cx);
        }
        cx.notify();
        true
    }

    fn sync_ruler_navigation(&mut self) {
        if self.ruler.navigation != self.renderer.navigation_revision() {
            self.reset_ruler(true);
        }
    }

    fn anchor_ruler(&mut self, range: &Range<u32>) {
        self.ruler.anchor = self.renderer.reading_document().and_then(|document| generate_cfi(self.renderer.document(), document, range.start));
        self.ruler.glyph = self.renderer.reading_document().map(|document| (document, range.start));
        self.ruler.edge = None;
    }

    pub(super) fn prepare_ruler(&mut self) -> Vec<(Rect, Hsla)> {
        if self.ruler.style == ReadingRulerStyle::Off {
            return Vec::new();
        }
        self.sync_ruler_navigation();
        let lines = self.renderer.visible_reading_lines();
        let Some(index) = self.ruler.index(&self.renderer, &lines) else { return Vec::new() };
        if self.ruler.edge.is_some() || self.ruler.glyph.is_none() {
            self.anchor_ruler(&lines[index].0);
        }
        let band = focus_bounds(&lines, index, self.ruler.line_count, self.ruler.from_end);
        match self.ruler.style {
            ReadingRulerStyle::Off => Vec::new(),
            ReadingRulerStyle::Band => vec![(band, hsla(0.12, 0.9, 0.55, 0.22))],
            ReadingRulerStyle::Underline => vec![(Rect::new(band.x0, band.y1 - 2.0, band.x1, band.y1), hsla(0.48, 0.5, 0.4, 0.9))],
            ReadingRulerStyle::Focus => {
                let size = self.prepared_viewport.unwrap_or(Size::ZERO);
                mask_rects(band, size).into_iter().map(|rect| (rect, hsla(0.0, 0.0, 0.0, 0.55))).collect()
            }
        }
    }
}

/// Groups stop at column/page edges, retaining a shorter group before the
/// next navigation request crosses that edge. Direction changes start outside
/// the entire current group, so adjacent groups never share a text line.
fn line_group(lines: &[(Range<u32>, Rect)], index: usize, count: usize, from_end: bool) -> Range<usize> {
    let current = lines[index].1;
    let same_column = |rect: Rect| (rect.x0 - current.x0).abs() < 1.0 && (rect.x1 - current.x1).abs() < 1.0;
    let column_start = (0..index).rev().find(|&i| !same_column(lines[i].1)).map_or(0, |i| i + 1);
    let column_end = (index + 1..lines.len()).find(|&i| !same_column(lines[i].1)).unwrap_or(lines.len());
    if from_end { (index + 1).saturating_sub(count.max(1)).max(column_start)..index + 1 } else { index..index.saturating_add(count.max(1)).min(column_end) }
}

fn focus_bounds(lines: &[(Range<u32>, Rect)], index: usize, count: usize, from_end: bool) -> Rect {
    line_group(lines, index, count, from_end).fold(lines[index].1, |bounds, i| bounds.union(lines[i].1))
}

fn mask_rects(band: Rect, size: Size) -> Vec<Rect> {
    let viewport = Rect::from_origin_size(Point::ZERO, size);
    let band = band.intersect(viewport);
    [Rect::new(0.0, 0.0, size.width, band.y0), Rect::new(0.0, band.y1, size.width, size.height), Rect::new(0.0, band.y0, band.x0, band.y1), Rect::new(band.x1, band.y0, size.width, band.y1)]
        .into_iter()
        .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0)
        .collect()
}

pub(super) fn paint_ruler(quads: &[(Rect, Hsla)], bounds: Bounds<Pixels>, window: &mut Window) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for (rect, color) in quads {
            let origin = bounds.origin + point(px(rect.x0 as f32), px(rect.y0 as f32));
            window.paint_quad(fill(Bounds::new(origin, size(px(rect.width() as f32), px(rect.height() as f32))), *color));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use html_view_core::{RendererInitialConfig, ResourceProvider};
    use std::{io, sync::Arc};

    struct RulerBook;

    impl ResourceProvider for RulerBook {
        fn read_bytes(&self, _: &str) -> io::Result<Vec<u8>> {
            let paragraphs = (0..100).map(|index| format!("<p>Reading line {index} with enough words to test reflow.</p>")).collect::<String>();
            Ok(format!("<html><head><style>body {{ margin: 0 }} p {{ margin: 0; line-height: 28px }}</style></head><body>{paragraphs}</body></html>").into_bytes())
        }

        fn exists(&self, _: &str) -> bool {
            true
        }
        fn resolve(&self, _: &str, href: &str) -> String {
            href.to_owned()
        }
        fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
            Ok(vec!["book.html".into()])
        }
    }

    #[gpui::test]
    fn reading_ruler_steps_then_turns_pages_in_both_directions(cx: &mut TestAppContext) {
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["book.html".into()], 0, None, RendererInitialConfig { max_column_count: Some(1), ..Default::default() }, window, cx));
        visual.simulate_resize(gpui::size(gpui::px(600.0), gpui::px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            assert!(!view.step_reading_ruler(true, cx), "disabled ruler leaves ordinary navigation available");
            view.set_reading_ruler(ReadingRulerStyle::Band, 1, cx);
            view.prepare_ruler();
            let first_page = view.current_cfi();
            let lines = view.renderer.visible_reading_lines();
            assert!(lines.len() > 3);
            view.step_reading_ruler(false, cx);
            view.prepare_ruler();
            assert_eq!(view.ruler.glyph.unwrap().1, lines[0].0.start, "backward at the book start must keep the first line");
            for line in lines.iter().skip(1) {
                view.step_reading_ruler(true, cx);
                view.prepare_ruler();
                assert_eq!(view.ruler.glyph.unwrap().1, line.0.start);
                assert_eq!(view.current_cfi(), first_page, "line stepping must not scroll the page");
            }
            view.step_reading_ruler(true, cx);
            view.prepare_ruler();
            assert_ne!(view.current_cfi(), first_page);
            assert_eq!(view.ruler.glyph.unwrap().1, view.renderer.visible_reading_lines()[0].0.start);
            view.step_reading_ruler(false, cx);
            view.prepare_ruler();
            assert_eq!(view.current_cfi(), first_page);
            assert_eq!(view.ruler.glyph.unwrap().1, lines.last().unwrap().0.start);
            for _ in 0..30 {
                if !view.next_page(cx) {
                    break;
                }
            }
            let last_page = view.current_cfi();
            let last_lines = view.renderer.visible_reading_lines();
            view.anchor_ruler(&last_lines.last().unwrap().0);
            view.step_reading_ruler(true, cx);
            view.prepare_ruler();
            assert_eq!(view.current_cfi(), last_page);
            assert_eq!(view.ruler.glyph.unwrap().1, last_lines.last().unwrap().0.start, "forward at the book end must keep the last line");
        });
    }

    #[gpui::test]
    fn reading_ruler_reflows_and_does_not_rebuild_page_paint(cx: &mut TestAppContext) {
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["book.html".into()], 0, None, RendererInitialConfig { max_column_count: Some(1), ..Default::default() }, window, cx));
        visual.simulate_resize(gpui::size(gpui::px(600.0), gpui::px(400.0)));
        visual.run_until_parked();
        let anchor = document.update(visual, |view, cx| {
            view.set_reading_ruler(ReadingRulerStyle::Band, 1, cx);
            view.prepare_ruler();
            let paint = view.paint_cache_stats();
            view.step_reading_ruler(true, cx);
            view.prepare_ruler();
            assert_eq!(view.paint_cache_stats(), paint, "moving the guide must retain page paint commands");
            view.ruler.anchor.clone().unwrap()
        });
        visual.simulate_resize(gpui::size(gpui::px(420.0), gpui::px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, _| {
            let (_, glyph) = html_view_core::cfi::parse_cfi(view.renderer.document(), &anchor).unwrap();
            let lines = view.renderer.visible_reading_lines();
            let selected = view.ruler.index(&view.renderer, &lines).unwrap();
            assert!(lines[selected].0.contains(&glyph), "the ruler must follow its passage after reflow");
            assert_eq!(view.prepare_ruler()[0].0, lines[selected].1);
        });
    }

    #[gpui::test]
    fn reading_ruler_stays_painted_before_settings_repaint_messages_arrive(cx: &mut TestAppContext) {
        let config = RendererInitialConfig { max_column_count: Some(1), ..Default::default() };
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["book.html".into()], 0, None, config, window, cx));
        visual.simulate_resize(size(px(600.0), px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            view.set_reading_ruler(ReadingRulerStyle::Focus, 3, cx);
            assert!(!view.prepare_ruler().is_empty());
            assert!(!view.display_dirty);
            assert!(!view.host.repaint_pending.get());
            let viewport = view.prepared_viewport.unwrap();
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(viewport.width as f32), px(viewport.height as f32)));
            let commands = [
                html_view_core::RendererCommand::SetFontSize(28.0),
                html_view_core::RendererCommand::SetColumnWidth(420.0),
                html_view_core::RendererCommand::SetColumnViewportPlan { available_width: viewport.width, column_width: 420.0, column_count: 1, column_gap: 48.0 },
            ];
            for command in commands {
                view.apply(command, cx);
                assert!(view.host.repaint_pending.get());
                // The settings panel can redraw the parent before its child's
                // queued repaint event is dispatched. That frame needs both
                // the updated page and its reading guide.
                view.prepared_paint_for_bounds(bounds, cx);
                assert!(!view.prepare_ruler().is_empty(), "settings must never expose a frame without the focus mask");
            }
        });
    }

    #[gpui::test]
    fn reading_ruler_crosses_columns_before_turning_the_page(cx: &mut TestAppContext) {
        let config = RendererInitialConfig { column_width: 400.0, max_column_count: Some(2), ..Default::default() };
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["book.html".into()], 0, None, config, window, cx));
        visual.simulate_resize(size(px(900.0), px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            view.set_reading_ruler(ReadingRulerStyle::Band, 1, cx);
            let lines = view.renderer.visible_reading_lines();
            let boundary = lines.windows(2).position(|pair| pair[0].1.x0 != pair[1].1.x0).expect("two visible columns");
            let page = view.current_cfi();
            view.anchor_ruler(&lines[boundary].0);
            view.step_reading_ruler(true, cx);
            assert_eq!(view.prepare_ruler()[0].0, lines[boundary + 1].1);
            assert_eq!(view.current_cfi(), page);
            view.step_reading_ruler(false, cx);
            assert_eq!(view.prepare_ruler()[0].0, lines[boundary].1);
        });
    }

    #[gpui::test]
    fn reading_ruler_crosses_chapters_in_both_directions(cx: &mut TestAppContext) {
        let config = RendererInitialConfig { max_column_count: Some(1), ..Default::default() };
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["first.html".into(), "second.html".into()], 0, None, config, window, cx));
        visual.simulate_resize(size(px(600.0), px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            view.set_reading_ruler(ReadingRulerStyle::Band, 1, cx);
            // Reach the end of the first chapter using ordinary page turns.
            for _ in 0..30 {
                if view.renderer.reading_document() != Some(0) {
                    break;
                }
                let lines = view.renderer.visible_reading_lines();
                view.anchor_ruler(&lines.last().unwrap().0);
                view.step_reading_ruler(true, cx);
            }
        });
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            assert_eq!(view.renderer.reading_document(), Some(1));
            view.prepare_ruler();
            assert_eq!(view.ruler.glyph.unwrap().1, view.renderer.visible_reading_lines()[0].0.start);
            view.step_reading_ruler(false, cx);
        });
        visual.run_until_parked();
        document.update(visual, |view, _| {
            assert_eq!(view.renderer.reading_document(), Some(0));
            view.prepare_ruler();
            assert_eq!(view.ruler.glyph.unwrap().1, view.renderer.visible_reading_lines().last().unwrap().0.start);
        });
    }

    #[gpui::test]
    fn reading_ruler_finishes_partial_groups_before_turning_pages(cx: &mut TestAppContext) {
        let config = RendererInitialConfig { max_column_count: Some(1), ..Default::default() };
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(RulerBook), vec!["book.html".into()], 0, None, config, window, cx));
        visual.simulate_resize(size(px(600.0), px(400.0)));
        visual.run_until_parked();
        document.update(visual, |view, cx| {
            for count in 1..=5 {
                view.set_reading_ruler(ReadingRulerStyle::Band, count, cx);
                view.reset_ruler(true);
                let page = view.current_cfi();
                let lines = view.renderer.visible_reading_lines();
                let mut consumed = 0;
                while consumed < lines.len() {
                    let end = (consumed + count).min(lines.len());
                    let expected = lines[consumed..end].iter().fold(lines[consumed].1, |bounds, (_, rect)| bounds.union(*rect));
                    assert_eq!(view.prepare_ruler()[0].0, expected, "forward group must contain only unread lines");
                    assert_eq!(view.current_cfi(), page, "a partial final group must remain on the page");
                    consumed = end;
                    view.step_reading_ruler(true, cx);
                }
                assert_ne!(view.current_cfi(), page, "turn only after the final group has been displayed");
                view.step_reading_ruler(false, cx);
                assert_eq!(view.current_cfi(), page);
                let mut remaining = lines.len();
                while remaining > 0 {
                    let start = remaining.saturating_sub(count);
                    let expected = lines[start..remaining].iter().fold(lines[start].1, |bounds, (_, rect)| bounds.union(*rect));
                    assert_eq!(view.prepare_ruler()[0].0, expected, "backward group must contain only unread lines");
                    assert_eq!(view.current_cfi(), page, "a partial first group must remain on the page");
                    remaining = start;
                    view.step_reading_ruler(false, cx);
                }
                assert_eq!(view.current_cfi(), page, "book-start boundary must reject a backward turn");
            }
        });
    }

    #[test]
    fn reading_ruler_groups_do_not_overlap_when_direction_changes() {
        let lines = (0..8).map(|i| (i..i + 1, Rect::new(0.0, f64::from(i) * 20.0, 100.0, f64::from(i + 1) * 20.0))).collect::<Vec<_>>();
        assert_eq!(line_group(&lines, 0, 3, false), 0..3);
        assert_eq!(line_group(&lines, 3, 3, false), 3..6);
        assert_eq!(line_group(&lines, 6, 3, false), 6..8);
        // Reverse from the short final group, then return to it.
        assert_eq!(line_group(&lines, 5, 3, true), 3..6);
        assert_eq!(line_group(&lines, 6, 3, false), 6..8);
        // Entering backwards starts from the bottom and leaves a short group at the top.
        assert_eq!(line_group(&lines, 7, 3, true), 5..8);
        assert_eq!(line_group(&lines, 4, 3, true), 2..5);
        assert_eq!(line_group(&lines, 1, 3, true), 0..2);
    }

    #[test]
    fn focus_window_stays_in_its_column() {
        let lines = vec![(0..5, Rect::new(0.0, 0.0, 100.0, 20.0)), (5..10, Rect::new(0.0, 20.0, 100.0, 40.0)), (10..15, Rect::new(120.0, 0.0, 220.0, 20.0))];
        assert_eq!(focus_bounds(&lines, 0, 3, false), Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(focus_bounds(&lines, 1, 3, true), Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(focus_bounds(&lines, 2, 3, false), lines[2].1);
        assert_eq!(focus_bounds(&lines, 2, 3, true), lines[2].1);
    }

    #[test]
    fn focus_mask_leaves_only_the_selected_window_clear() {
        let size = Size::new(240.0, 400.0);
        let band = Rect::new(20.0, 100.0, 120.0, 140.0);
        let mask = mask_rects(band, size);
        assert_eq!(mask.len(), 4);
        assert_eq!(mask.iter().map(|rect| rect.area()).sum::<f64>() + band.area(), size.width * size.height);
        assert!(mask.iter().all(|rect| rect.intersect(band).area() == 0.0));
    }
}
