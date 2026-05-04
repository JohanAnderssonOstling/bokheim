use floem::kurbo::{Point, Rect};
use floem::peniko::Color;
use floem::reactive::SignalUpdate;

use crate::document::Line;

use super::*;

impl HTMLRenderer {
    /// Scan document glyphs for all occurrences of `query` (case-insensitive).
    /// Returns sorted (start_glyph, end_glyph) pairs.
    pub(super) fn find_matches(&self, query: &str) -> Vec<(u32, u32)> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_chars: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
        let n = query_chars.len();
        if n == 0 {
            return Vec::new();
        }

        // Collect (lowercased_char, glyph_index) for every glyph
        let glyph_chars: Vec<char> = self
            .document
            .glyphs
            .iter()
            .map(|&idx| {
                let ch = self.glyph_cache.get_char(idx);
                ch.to_lowercase().next().unwrap_or(ch)
            })
            .collect();

        let total = glyph_chars.len();
        if total < n {
            return Vec::new();
        }

        let mut matches = Vec::new();
        for start in 0..=(total - n) {
            if glyph_chars[start..start + n] == query_chars[..] {
                matches.push((start as u32, (start + n) as u32));
            }
        }
        matches
    }

    /// Scroll so that the current match is visible at the top of the viewport.
    pub(super) fn navigate_to_current_match(&mut self) {
        let Some(&(start, _)) = self.highlight.matches.get(self.highlight.current_match) else {
            return;
        };
        let Some(line_idx) = self.find_line_for_glyph(start) else {
            return;
        };
        // Only scroll if the match is not already on screen
        let is_visible = self.frame.last_line_positions.get(line_idx).and_then(|p| *p).is_some();
        if is_visible {
            self.id.request_paint();
            return;
        }
        let line_y = self.document.lines[line_idx].point.y;
        self.viewport.start_offset_y = (line_y - 40.0).max(0.0);
        self.viewport.direction = PaintDirection::Forward;
        self.id.request_paint();
    }

    pub(super) fn update_match_signal(&self) {
        let total = self.highlight.matches.len();
        let current = if total == 0 { 0 } else { self.highlight.current_match + 1 };
        self.highlight.match_info.set((current, total));
    }

    /// Paint highlight rects for all matches that overlap `line`.
    /// Called for every visible line during `paint_lines_from_positions`.
    pub(super) fn paint_highlights_for_line(&self, cx: &mut impl Painter, line: &Line, screen_point: Point) {
        if self.highlight.matches.is_empty() {
            return;
        }

        // Binary-search for the first match whose end is past the line's start glyph
        let first = self.highlight.matches.partition_point(|&(_, end)| end <= line.glyphs.start);

        let col_start_x = screen_point.x - line.point.x;
        let col_end_x = col_start_x + self.layout.col_width;

        for (i, &(match_start, match_end)) in self.highlight.matches[first..].iter().enumerate() {
            if match_start >= line.glyphs.end {
                break;
            }
            let is_current = first + i == self.highlight.current_match;
            let color = if is_current {
                Color::rgba8(255, 140, 0, 160) // orange for focused match
            } else {
                Color::rgba8(255, 220, 0, 110) // yellow for other matches
            };

            let starts_before = match_start < line.glyphs.start;
            let ends_after = match_end > line.glyphs.end;

            let x0 = if starts_before { col_start_x } else { screen_point.x + self.glyph_x_in_line(line, match_start) };
            let x1 = if ends_after { col_end_x } else { screen_point.x + self.glyph_x_in_line(line, match_end) };

            if x1 > x0 {
                let rect = Rect::new(x0, screen_point.y, x1, screen_point.y + line.height);
                cx.fill_rect(rect, color);
            }
        }
    }
}
