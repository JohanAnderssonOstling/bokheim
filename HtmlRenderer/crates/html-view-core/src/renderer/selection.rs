use crate::{FormatState, RendererCore};
use kurbo::Point;

impl RendererCore {
    pub fn selection_range(&self) -> Option<(u32, u32)> {
        // normalize selection anchor/active into a range
        let (Some(a), Some(b)) = (self.selection.selection_anchor, self.selection.selection_active) else {
            return None;
        };
        if a == b {
            return None;
        }
        let (mut start, mut end) = if a < b { (a, b) } else { (b, a) }; // normalize range order
        if self.selection.select_whole_lines {
            let start_line = self.find_line_for_glyph(start)?;
            let end_line = self.find_line_for_glyph(end.saturating_sub(1))?;
            let view = self.document.render_view();
            start = view.line(start_line).map(|line| line.start())?;
            end = view.line(end_line).map(|line| line.end())?;
        }
        Some((start, end))
    }

    pub fn block_ancestor(&self, box_idx: usize) -> usize {
        // walk up to the nearest block box
        let view = self.document.render_view();
        let mut idx = box_idx;
        loop {
            if view.is_block_container_box(idx) {
                return idx;
            }
            let Some(parent) = view.box_parent(idx) else {
                return idx;
            };
            idx = parent as usize;
        }
    }

    pub fn box_tag(&self, box_idx: usize) -> String {
        // get tag name for a box
        self.document.render_view().box_tag(box_idx).unwrap_or("").to_string()
    }

    pub fn tag_is_one_of(tag: &str, candidates: &[&str]) -> bool {
        // case-insensitive tag membership check
        candidates.iter().any(|c| tag.eq_ignore_ascii_case(c))
    }

    pub fn header_prefix(&self, block_idx: usize) -> Option<&'static str> {
        // map header tags to markdown prefixes
        let tag = self.box_tag(block_idx);
        if tag.eq_ignore_ascii_case("h1") {
            Some("# ")
        } else if tag.eq_ignore_ascii_case("h2") {
            Some("## ")
        } else if tag.eq_ignore_ascii_case("h3") {
            Some("### ")
        } else if tag.eq_ignore_ascii_case("h4") {
            Some("#### ")
        } else if tag.eq_ignore_ascii_case("h5") {
            Some("##### ")
        } else if tag.eq_ignore_ascii_case("h6") {
            Some("###### ")
        } else {
            None
        }
    }

    pub fn format_state_for_box(&self, box_idx: usize) -> FormatState {
        // derive formatting from style + tag
        let tag = self.box_tag(box_idx);
        let style = self.document.render_view().box_text_format(box_idx);

        let mut state = FormatState::default();
        state.italic = style.font_style == crate::FontStyle::Italic;
        state.bold = style.font_weight >= 600;
        state.strike = (style.text_decoration & 4) != 0;

        if Self::tag_is_one_of(&tag, &["em", "i"]) {
            state.italic = true;
        }
        if Self::tag_is_one_of(&tag, &["strong", "b"]) {
            state.bold = true;
        }
        if Self::tag_is_one_of(&tag, &["s", "del", "strike"]) {
            state.strike = true;
        }
        if Self::tag_is_one_of(&tag, &["code"]) {
            state.code = true;
            state.bold = false;
            state.italic = false;
            state.strike = false;
        }
        state
    }

    pub fn push_newline(plain: &mut String, markdown: &mut String) {
        // keep plain/markdown buffers in sync on line breaks
        if !plain.ends_with('\n') {
            plain.push('\n');
        }
        if !markdown.ends_with('\n') {
            markdown.push('\n');
        }
    }

    pub fn apply_format_transition(markdown: &mut String, prev: FormatState, next: FormatState) {
        // emit markdown toggles for format changes
        if prev.italic && !next.italic {
            markdown.push('*');
        }
        if prev.bold && !next.bold {
            markdown.push_str("**");
        }
        if prev.strike && !next.strike {
            markdown.push_str("~~");
        }
        if prev.code && !next.code {
            markdown.push('`');
        }
        if !prev.strike && next.strike {
            markdown.push_str("~~");
        }
        if !prev.bold && next.bold {
            markdown.push_str("**");
        }
        if !prev.italic && next.italic {
            markdown.push('*');
        }
        if !prev.code && next.code {
            markdown.push('`');
        }
    }

    pub fn build_selection_text(&self, start: u32, end: u32) -> (Option<String>, Option<String>) {
        // extract selected text as plain and markdown
        let mut plain = String::new();
        let mut markdown = String::new();
        let mut current_format = FormatState::default();
        let mut prev_block: Option<usize> = None;
        let mut at_block_start = true;

        for run in self.document.render_view().text_runs() {
            let glyphs = run.glyphs();
            if glyphs.end <= start || glyphs.start >= end {
                continue;
            }
            let run_start = glyphs.start.max(start);
            let run_end = glyphs.end.min(end);
            let box_idx = run.box_idx();
            let block = self.block_ancestor(box_idx);
            let desired_format = self.format_state_for_box(box_idx);

            if let Some(prev) = prev_block {
                if prev != block {
                    Self::apply_format_transition(&mut markdown, current_format, FormatState::default());
                    current_format = FormatState::default();
                    Self::push_newline(&mut plain, &mut markdown);
                    at_block_start = true;
                }
            }

            if at_block_start {
                if let Some(prefix) = self.header_prefix(block) {
                    markdown.push_str(prefix);
                }
                at_block_start = false;
            }

            if desired_format != current_format {
                Self::apply_format_transition(&mut markdown, current_format, desired_format);
                current_format = desired_format;
            }

            for i in run_start..run_end {
                let view = self.document.render_view();
                let glyph_index = view.glyph_at(i as usize).unwrap_or_default();
                let c = view.glyph_metric(glyph_index).expect("layout glyph IDs are registered").ch();
                if c == '\n' {
                    Self::apply_format_transition(&mut markdown, current_format, FormatState::default());
                    current_format = FormatState::default();
                    Self::push_newline(&mut plain, &mut markdown);
                    at_block_start = true;
                } else {
                    plain.push(c);
                    if current_format.code && c == '`' {
                        markdown.push_str("\\`");
                    } else {
                        markdown.push(c);
                    }
                }
            }
            prev_block = Some(block);
        }

        Self::apply_format_transition(&mut markdown, current_format, FormatState::default());
        if plain.is_empty() {
            return (None, None);
        }
        (Some(plain), Some(markdown))
    }

    pub fn update_selection_text(&mut self) {
        // refresh selection buffers based on current range
        let Some((start, end)) = self.selection_range() else {
            self.selection.selected_text = None;
            self.selection.selected_text_markdown = None;
            return;
        };
        let (plain, markdown) = self.build_selection_text(start, end);
        self.selection.selected_text = plain;
        self.selection.selected_text_markdown = markdown;
    }

    pub fn find_line_for_glyph(&self, glyph_idx: u32) -> Option<usize> {
        // map glyph index to line index
        let lines = self.document.render_view().lines();
        if lines.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = lines.get(mid)?;
            if glyph_idx < line.start() {
                hi = mid;
            } else if glyph_idx >= line.end() {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    pub fn selection_line_bounds(&self, start: u32, end: u32) -> Option<(usize, usize)> {
        // get line span covered by selection
        if end == 0 {
            return None;
        }
        let start_line = self.find_line_for_glyph(start)?;
        let end_line = self.find_line_for_glyph(end.saturating_sub(1))?;
        Some((start_line, end_line))
    }

    pub fn glyph_x_in_line(&self, line: &html_layout::RenderLine, glyph_idx: u32) -> f64 {
        // measure x offset of glyph within line
        if glyph_idx <= line.start() {
            return 0.0;
        }
        let mut x = 0.0;
        let line_idx = self.find_line_for_glyph(line.start());
        let glyphs = line.glyphs();
        for i in glyphs.start..glyphs.end {
            if i >= glyph_idx {
                break;
            }
            let view = self.document.render_view();
            let glyph_index = view.glyph_at(i as usize).unwrap_or_default();
            let metric = view.glyph_metric(glyph_index).expect("layout glyph IDs are registered");
            if let Some(advance) = line_idx.and_then(|line_idx| self.glyph_advance_override(line_idx, i)) {
                x += advance;
            } else {
                x += metric.advance() as f64 + line.letter_spacing();
                if metric.ch() == ' ' {
                    x += line.word_spacing();
                }
            }
        }
        x
    }

    pub fn hit_test_glyph(&self, pos: Point) -> Option<u32> {
        // find nearest glyph index for a screen position
        let lines = self.document.render_view().lines();
        if self.frame.last_line_positions.is_empty() || lines.is_empty() || self.layout.col_count <= 0.0 {
            return None;
        }

        let max_col = self.layout.col_count as i32 - 1;
        let target_col = (((pos.x - self.layout.col_gap) / (self.layout.col_width + self.layout.col_gap)).floor() as i32).clamp(0, max_col); // hit-test only within column

        let mut best_idx: Option<usize> = None;
        let mut best_dist = f64::INFINITY; // track closest line if cursor is between lines
        for (idx, screen) in self.frame.last_line_positions.iter().enumerate() {
            let Some(screen) = screen else {
                continue;
            };
            if screen.col_index != target_col {
                continue;
            }
            let Some(line) = lines.get(idx) else {
                continue;
            };
            let y0 = screen.point.y;
            let y1 = y0 + line.height();
            let dist = if pos.y < y0 {
                y0 - pos.y
            } else if pos.y > y1 {
                pos.y - y1
            } else {
                best_idx = Some(idx);
                break;
            };
            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(idx);
            }
        }

        let line_idx = best_idx?;
        let line = lines.get(line_idx)?;
        let screen = self.frame.last_line_positions[line_idx]?;
        let rel_x = pos.x - screen.point.x;
        if rel_x <= 0.0 {
            return Some(line.start());
        }

        let mut x = 0.0;
        let glyphs = line.glyphs();
        for i in glyphs.start..glyphs.end {
            let view = self.document.render_view();
            let glyph_index = view.glyph_at(i as usize).unwrap_or_default();
            let metric = view.glyph_metric(glyph_index).expect("layout glyph IDs are registered");
            let override_advance = self.glyph_advance_override(line_idx, i);
            let w = override_advance.unwrap_or(metric.advance() as f64 + line.letter_spacing() + if metric.ch() == ' ' { line.word_spacing() } else { 0.0 });
            if rel_x < x + w / 2.0 {
                return Some(i);
            }
            x += w;
        }
        Some(line.end())
    }

    fn glyph_advance_override(&self, line_idx: usize, glyph_idx: u32) -> Option<f64> {
        self.document.render_view().line_glyph_advances(line_idx)?.iter().find(|run| run.range().contains(&glyph_idx)).map(|run| run.advance())
    }

    pub fn hit_test_image(&self, pos: Point) -> Option<u32> {
        if self.frame.last_line_positions.is_empty() || self.layout.col_count <= 0.0 {
            return None;
        }

        let max_col = self.layout.col_count as i32 - 1;
        let target_col = (((pos.x - self.layout.col_gap) / (self.layout.col_width + self.layout.col_gap)).floor() as i32).clamp(0, max_col);

        let view = self.document.render_view();
        for (line_idx, screen) in self.frame.last_line_positions.iter().enumerate() {
            let Some(screen) = screen else {
                continue;
            };
            if screen.col_index != target_col || view.images_for_line(line_idx).is_empty() {
                continue;
            }
            for frag in view.images_for_line(line_idx).iter() {
                let size = frag.size();
                let offset = frag.offset();
                let (width, height) = (size.width, size.height);
                let x0 = screen.point.x + offset.x;
                let y0 = screen.point.y + offset.y;
                let x1 = x0 + width;
                let y1 = y0 + height;
                if pos.x >= x0 && pos.x <= x1 && pos.y >= y0 && pos.y <= y1 {
                    return Some(frag.image_idx());
                }
            }
        }
        None
    }
}
