use kurbo::{Point, Rect};
use peniko::Color;

use crate::{PaintDirection, Painter, RendererCore, RendererEvent};

impl RendererCore {
    /// Scan document glyphs for all occurrences of `query` (case-insensitive).
    /// Returns sorted (start_glyph, end_glyph) pairs.
    pub fn find_matches(&self, query: &str) -> Vec<(u32, u32)> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_chars: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
        let n = query_chars.len();
        if n == 0 {
            return Vec::new();
        }

        // Generated marker glyphs are not part of the document's searchable text
        // (matching how browsers exclude ::marker content), so mask them out with
        // a sentinel that cannot match and also breaks runs across them.
        let view = self.document.render_view();
        let total = view.glyph_count();
        let mut is_marker = vec![false; total];
        for run in view.marker_runs() {
            let glyphs = run.glyphs();
            for i in glyphs {
                if (i as usize) < total {
                    is_marker[i as usize] = true;
                }
            }
        }

        // Collect lowercased char for every glyph (marker glyphs become '\0').
        let glyph_chars: Vec<char> = view
            .glyph_slice(0..total as u32)
            .expect("the full glyph range is valid")
            .iter()
            .enumerate()
            .map(|(i, &idx)| {
                if is_marker[i] {
                    return '\u{0}';
                }
                let ch = view.glyph_metric(idx).expect("layout glyph IDs are registered").ch();
                ch.to_lowercase().next().unwrap_or(ch)
            })
            .collect();
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
    pub fn navigate_to_current_match(&mut self) {
        let Some(&(start, _)) = self.highlight.matches.get(self.highlight.current_match) else {
            return;
        };
        let Some(line_idx) = self.find_line_for_glyph(start) else {
            return;
        };
        // Only scroll if the match is not already on screen
        let is_visible = self.frame.last_line_positions.get(line_idx).and_then(|p| *p).is_some();
        if is_visible {
            self.host.request_repaint();
            return;
        }
        let line_y = self.document.render_view().line(line_idx).map(|line| line.point().y).unwrap_or_default();
        self.viewport.start_offset_y = (line_y - 40.0).max(0.0);
        self.viewport.direction = PaintDirection::Forward;
        self.host.request_repaint();
    }

    pub fn update_match_signal(&self) {
        let total = self.highlight.matches.len();
        let current = if total == 0 { 0 } else { self.highlight.current_match + 1 };
        self.host.emit(RendererEvent::MatchInfo { current, total });
    }

    /// Paint highlight rects for all matches that overlap `line`.
    /// Called for every visible line during `paint_lines_from_positions`.
    pub fn paint_highlights_for_line(&self, cx: &mut impl Painter, line: &html_layout::RenderLine, screen_point: Point) {
        if self.highlight.matches.is_empty() {
            return;
        }

        // Binary-search for the first match whose end is past the line's start glyph
        let first = self.highlight.matches.partition_point(|&(_, end)| end <= line.start());

        let point = line.point();
        let col_start_x = screen_point.x - point.x;
        let col_end_x = col_start_x + self.layout.col_width;

        for (i, &(match_start, match_end)) in self.highlight.matches[first..].iter().enumerate() {
            if match_start >= line.end() {
                break;
            }
            let is_current = first + i == self.highlight.current_match;
            let color = if is_current { self.interaction_style.active_search_match } else { self.interaction_style.search_match };
            let color = Color::rgba8(color.r, color.g, color.b, color.a);

            let starts_before = match_start < line.start();
            let ends_after = match_end > line.end();

            let x0 = if starts_before { col_start_x } else { screen_point.x + self.glyph_x_in_line(line, match_start) };
            let x1 = if ends_after { col_end_x } else { screen_point.x + self.glyph_x_in_line(line, match_end) };

            if x1 > x0 {
                let rect = Rect::new(x0, screen_point.y, x1, screen_point.y + line.height());
                cx.fill_rect(rect, color);
            }
        }
    }
}
