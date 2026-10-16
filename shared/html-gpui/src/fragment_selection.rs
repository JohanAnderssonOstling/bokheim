//! Selection geometry for continuous fragments, using the painted line layout.

use std::collections::BTreeMap;
use std::ops::Range;

use gpui::{Bounds, FocusHandle, Pixels, Point, point, px};
use html::layout::LaidOutDocument;

use crate::text::GpuiTextCache;

pub(crate) struct SelectionRun {
    pub range: Range<usize>,
    pub bounds: Bounds<Pixels>,
    reversed: bool,
}

impl SelectionRun {
    fn cursor_at(&self, position: Point<Pixels>) -> usize {
        if (position.x < self.bounds.center().x) != self.reversed { self.range.start } else { self.range.end }
    }
}

pub(crate) struct FragmentSelection {
    pub focus: FocusHandle,
    pub text: String,
    pub runs: Vec<SelectionRun>,
    pub range: Range<usize>,
    pub dragging: bool,
    anchor: usize,
}

impl FragmentSelection {
    pub fn new(focus: FocusHandle) -> Self {
        Self { focus, text: String::new(), runs: Vec::new(), range: 0..0, dragging: false, anchor: 0 }
    }

    pub fn install(&mut self, document: &LaidOutDocument, shaper: &GpuiTextCache) {
        self.clear();
        self.text.clear();
        self.runs.clear();
        let text = document.render_view().text();
        let native_runs = shaper.run_store();
        let mut clusters = BTreeMap::new();
        for line in text.lines().iter() {
            let advances = text.line_glyph_advances(line.index());
            for fragment in text.line_text_fragments(line.index()).into_iter().flatten().filter(|fragment| fragment.visible()) {
                let glyphs = fragment.glyphs();
                let mut index = glyphs.start;
                let mut x = line.point_x() + line.optical_offset_x() + fragment.offset_x();
                while index < glyphs.end {
                    let start = index;
                    let mut value = String::new();
                    let mut width = 0.0;
                    loop {
                        if let Some(metric) = text.glyph_at(index as usize).and_then(|glyph| text.glyph_metric(glyph)) {
                            if metric.ch() != '\u{00ad}' {
                                value.push(metric.ch());
                            }
                            let overridden = advances.as_ref().and_then(|runs| runs.iter().find(|run| run.range().contains(&index))).map(|run| run.advance());
                            width += overridden.unwrap_or_else(|| {
                                f64::from(text.character_advance(index).unwrap_or_else(|| metric.advance()))
                                    + if index + 1 < line.end() && text.is_character_cluster_boundary(index + 1) { line.letter_spacing() } else { 0.0 }
                                    + if metric.ch() == ' ' { line.word_spacing() } else { 0.0 }
                            });
                        }
                        index += 1;
                        if index == glyphs.end || text.is_character_cluster_boundary(index) {
                            break;
                        }
                    }
                    let mut left = x;
                    let mut right = x + width;
                    let mut reversed = false;
                    // Native caret positions preserve ligatures and bidi order.
                    // The prepared slice already includes justification offsets.
                    if let Some(run) = text.authoritative_runs(start..index).next() {
                        let local = run.run_range();
                        if let Some(prepared) = text.prepared_text_runs(line.index()).and_then(|runs| runs.iter().find(|candidate| candidate.run() == run.run() && candidate.range().contains(&local.start))) {
                            if let Some(native) = native_runs.get(run.run() as usize) {
                                if let (Some(&a), Some(&b)) = (native.caret_stops.get(local.start as usize), native.caret_stops.get(local.end as usize)) {
                                    left = line.point_x() + prepared.offset().x + f64::from(a.min(b));
                                    right = left + width.max(f64::from((b - a).abs()));
                                    reversed = a > b;
                                }
                            }
                        }
                    }
                    x += width;
                    if value.is_empty() {
                        continue;
                    }
                    let mut bounds = Bounds::from_corners(point(px(left as f32), px(line.point_y() as f32)), point(px(right as f32), px((line.point_y() + line.height()) as f32)));
                    if let Some(clip) = text.line_overflow_clip(line.index()) {
                        let clip_rect = clip.rect();
                        let clip_bounds = Bounds::from_corners(
                            point(if clip.clips_x() { px(clip_rect.x0 as f32) } else { bounds.left() }, if clip.clips_y() { px(clip_rect.y0 as f32) } else { bounds.top() }),
                            point(if clip.clips_x() { px(clip_rect.x1 as f32) } else { bounds.right() }, if clip.clips_y() { px(clip_rect.y1 as f32) } else { bounds.bottom() }),
                        );
                        bounds = bounds.intersect(&clip_bounds);
                    }
                    if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                        clusters.entry(start).or_insert((value, bounds, line.owner_box_idx(), index, reversed));
                    }
                }
            }
        }
        let mut owner = None;
        let mut previous_end = 0;
        for (source_start, (value, bounds, block, source_end, reversed)) in clusters {
            if owner.is_some_and(|owner| owner != block) {
                self.text.push_str("\n\n");
            } else if owner == Some(block) {
                // Layout omits spaces at wrapped line ends. They still belong
                // in copied prose even though they have no painted rectangle.
                for index in previous_end..source_start {
                    if let Some(metric) = text.glyph_at(index as usize).and_then(|glyph| text.glyph_metric(glyph)) {
                        if metric.ch().is_whitespace() {
                            self.text.push(metric.ch());
                        }
                    }
                }
            }
            owner = Some(block);
            previous_end = source_end;
            let start = self.text.len();
            self.text.push_str(&value);
            self.runs.push(SelectionRun { range: start..self.text.len(), bounds, reversed });
        }
    }

    pub fn clear(&mut self) {
        self.range = 0..0;
        self.dragging = false;
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.range.clone()]
    }

    pub fn begin(&mut self, position: Point<Pixels>, clicks: usize) -> bool {
        self.clear();
        let Some(run) = self.run_at(position) else { return false };
        let offset = run.range.start;
        match clicks {
            2 => {
                let category = |ch: char| {
                    if ch.is_alphanumeric() || ch == '_' || ('\u{0300}'..='\u{036f}').contains(&ch) {
                        0
                    } else if ch.is_whitespace() {
                        1
                    } else {
                        2
                    }
                };
                let kind = category(self.text[offset..].chars().next().unwrap());
                if kind == 2 {
                    self.range = run.range.clone();
                    return true;
                }
                let start = self.text[..offset].char_indices().rev().take_while(|(_, ch)| category(*ch) == kind).last().map_or(offset, |(index, _)| index);
                let end = offset + self.text[offset..].chars().take_while(|ch| category(*ch) == kind).map(char::len_utf8).sum::<usize>();
                self.range = start..end;
            }
            3.. => {
                let start = self.text[..offset].rfind("\n\n").map_or(0, |index| index + 2);
                let end = self.text[offset..].find("\n\n").map_or(self.text.len(), |index| offset + index);
                self.range = start..end;
            }
            _ => {
                self.anchor = run.cursor_at(position);
                self.range = self.anchor..self.anchor;
                self.dragging = true;
            }
        }
        true
    }

    pub fn extend(&mut self, position: Point<Pixels>) {
        if !self.dragging {
            return;
        }
        let Some(run) = self.run_at(position) else { return };
        let cursor = run.cursor_at(position);
        self.range = self.anchor.min(cursor)..self.anchor.max(cursor);
    }

    fn run_at(&self, position: Point<Pixels>) -> Option<&SelectionRun> {
        self.runs.iter().min_by(|a, b| {
            let distance = |run: &SelectionRun| {
                let y = (run.bounds.top() - position.y).max(position.y - run.bounds.bottom()).max(px(0.));
                let x = (run.bounds.left() - position.x).max(position.x - run.bounds.right()).max(px(0.));
                (f32::from(y), f32::from(x))
            };
            distance(a).partial_cmp(&distance(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{EmptyView, TestAppContext};

    #[gpui::test]
    fn description_selection_can_start_in_paragraph_padding(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        window
            .update(cx, |_, _, cx| {
                let mut selection = FragmentSelection::new(cx.focus_handle());
                selection.text = "Hi".into();
                selection.runs = vec![
                    SelectionRun { range: 0..1, bounds: Bounds::from_corners(point(px(10.), px(10.)), point(px(20.), px(30.))), reversed: false },
                    SelectionRun { range: 1..2, bounds: Bounds::from_corners(point(px(20.), px(10.)), point(px(30.), px(30.))), reversed: false },
                ];
                assert!(selection.begin(point(px(0.), px(20.)), 1));
                selection.extend(point(px(40.), px(20.)));
                assert_eq!(selection.selected_text(), "Hi");
                assert!(selection.begin(point(px(40.), px(20.)), 1));
                selection.extend(point(px(0.), px(20.)));
                assert_eq!(selection.selected_text(), "Hi");
            })
            .unwrap();
    }

    #[gpui::test]
    fn selection_keeps_clusters_whole_and_respects_rtl_carets(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| EmptyView);
        window
            .update(cx, |_, _, cx| {
                let mut selection = FragmentSelection::new(cx.focus_handle());
                selection.text = "A👩‍👧e\u{301}Z".into();
                let emoji_end = 1 + "👩‍👧".len();
                let accent_end = emoji_end + "e\u{301}".len();
                let rect = |left, right| Bounds::from_corners(point(px(left), px(0.)), point(px(right), px(10.)));
                selection.runs = vec![
                    SelectionRun { range: 0..1, bounds: rect(0., 10.), reversed: false },
                    SelectionRun { range: 1..emoji_end, bounds: rect(10., 30.), reversed: false },
                    SelectionRun { range: emoji_end..accent_end, bounds: rect(30., 40.), reversed: false },
                    SelectionRun { range: accent_end..selection.text.len(), bounds: rect(40., 50.), reversed: false },
                ];
                assert!(selection.begin(point(px(11.), px(5.)), 1));
                selection.extend(point(px(39.), px(5.)));
                assert_eq!(selection.selected_text(), "👩‍👧e\u{301}");
                assert!(selection.begin(point(px(39.), px(5.)), 1));
                selection.extend(point(px(11.), px(5.)));
                assert_eq!(selection.selected_text(), "👩‍👧e\u{301}");

                selection.text = "אב".into();
                selection.runs = vec![SelectionRun { range: 0..2, bounds: rect(10., 20.), reversed: true }, SelectionRun { range: 2..4, bounds: rect(0., 10.), reversed: true }];
                assert!(selection.begin(point(px(19.), px(5.)), 1));
                selection.extend(point(px(1.), px(5.)));
                assert_eq!(selection.selected_text(), "אב");
            })
            .unwrap();
    }
}
