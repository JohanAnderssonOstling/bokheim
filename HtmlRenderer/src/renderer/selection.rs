use super::*;

impl HTMLRenderer {
    pub(super) fn word_for_glyph(&self, glyph_idx: u32) -> Option<String> {
        let line_idx = self.find_line_for_glyph(glyph_idx)?;
        let line = self.document.line(line_idx)?;
        if glyph_idx < line.glyphs.start || glyph_idx >= line.glyphs.end {
            return None;
        }
        let glyph_index = self.document.glyph_at(glyph_idx as usize)?;
        let ch = self.glyph_cache.get_char(glyph_index);
        if ch.is_whitespace() {
            return Some("(whitespace)".to_string());
        }

        let mut start = glyph_idx;
        while start > line.glyphs.start {
            let prev_idx = start - 1;
            let prev_glyph = self.document.glyph_at(prev_idx as usize)?;
            if self.glyph_cache.get_char(prev_glyph).is_whitespace() {
                break;
            }
            start = prev_idx;
        }

        let mut end = glyph_idx + 1;
        while end < line.glyphs.end {
            let next_glyph = self.document.glyph_at(end as usize)?;
            if self.glyph_cache.get_char(next_glyph).is_whitespace() {
                break;
            }
            end += 1;
        }

        let mut word = String::new();
        for idx in start..end {
            let glyph_index = self.document.glyph_at(idx as usize)?;
            word.push(self.glyph_cache.get_char(glyph_index));
        }
        Some(word)
    }

    pub(super) fn selection_range(&self) -> Option<(u32, u32)> {
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
            start = self.document.line(start_line)?.glyphs.start;
            end = self.document.line(end_line)?.glyphs.end;
        }
        Some((start, end))
    }

    pub(super) fn block_ancestor(&self, box_idx: usize) -> usize {
        // walk up to the nearest block box
        let mut idx = box_idx;
        loop {
            if matches!(self.document.box_layout_mode(idx), Some(BoxType::Block(_) | BoxType::Table(_) | BoxType::TableCell(_))) {
                return idx;
            }
            let Some(parent) = self.document.get_parent(idx) else {
                return idx;
            };
            idx = parent as usize;
        }
    }

    pub(super) fn box_tag(&self, box_idx: usize) -> &str {
        // get tag name for a box
        self.document.get_tag(box_idx)
    }

    pub(super) fn tag_is_one_of(tag: &str, candidates: &[&str]) -> bool {
        // case-insensitive tag membership check
        candidates.iter().any(|c| tag.eq_ignore_ascii_case(c))
    }

    pub(super) fn header_prefix(&self, block_idx: usize) -> Option<&'static str> {
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

    pub(super) fn format_state_for_box(&self, box_idx: usize) -> FormatState {
        // derive formatting from style + tag
        let tag = self.box_tag(box_idx);
        let style = crate::layout::get_style(&self.document, box_idx);

        let mut state = FormatState::default();
        state.italic = style.font_style() == crate::document::FontStyle::Italic;
        state.bold = style.font_weight() >= 600;
        state.strike = (style.text_decoration() & 4) != 0;

        if Self::tag_is_one_of(tag, &["em", "i"]) {
            state.italic = true;
        }
        if Self::tag_is_one_of(tag, &["strong", "b"]) {
            state.bold = true;
        }
        if Self::tag_is_one_of(tag, &["s", "del", "strike"]) {
            state.strike = true;
        }
        if Self::tag_is_one_of(tag, &["code"]) {
            state.code = true;
            state.bold = false;
            state.italic = false;
            state.strike = false;
        }
        state
    }

    pub(super) fn push_newline(plain: &mut String, markdown: &mut String) {
        // keep plain/markdown buffers in sync on line breaks
        if !plain.ends_with('\n') {
            plain.push('\n');
        }
        if !markdown.ends_with('\n') {
            markdown.push('\n');
        }
    }

    pub(super) fn apply_format_transition(markdown: &mut String, prev: FormatState, next: FormatState) {
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

    pub(super) fn build_selection_text(&self, start: u32, end: u32) -> (Option<String>, Option<String>) {
        // extract selected text as plain and markdown
        let mut plain = String::new();
        let mut markdown = String::new();
        let mut current_format = FormatState::default();
        let mut prev_block: Option<usize> = None;
        let mut at_block_start = true;

        for run in self.document.inline_runs() {
            let InlineKind::Text { glyphs } = &run.kind else {
                continue;
            };
            if glyphs.end <= start || glyphs.start >= end {
                continue;
            }
            let run_start = glyphs.start.max(start);
            let run_end = glyphs.end.min(end);
            let box_idx = run.box_idx as usize;
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
                let glyph_index = self.document.glyph_at(i as usize).unwrap_or_default();
                let c = self.glyph_cache.get_char(glyph_index);
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

    pub(super) fn update_selection_text(&mut self) {
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

    pub(super) fn find_line_for_glyph(&self, glyph_idx: u32) -> Option<usize> {
        // map glyph index to line index
        let lines = self.document.lines();
        if lines.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = lines.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = &lines[mid];
            if glyph_idx < line.glyphs.start {
                hi = mid;
            } else if glyph_idx >= line.glyphs.end {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    pub(super) fn selection_line_bounds(&self, start: u32, end: u32) -> Option<(usize, usize)> {
        // get line span covered by selection
        if end == 0 {
            return None;
        }
        let start_line = self.find_line_for_glyph(start)?;
        let end_line = self.find_line_for_glyph(end.saturating_sub(1))?;
        Some((start_line, end_line))
    }

    pub(super) fn glyph_x_in_line(&self, line: &Line, glyph_idx: u32) -> f64 {
        // measure x offset of glyph within line
        if glyph_idx <= line.glyphs.start {
            return 0.0;
        }
        let mut x = 0.0;
        for i in line.glyphs.start..line.glyphs.end {
            if i >= glyph_idx {
                break;
            }
            let glyph_index = self.document.glyph_at(i as usize).unwrap_or_default();
            let w = self.glyph_cache.get(glyph_index).size().width;
            x += w + line.letter_spacing;
            if self.glyph_cache.get_char(glyph_index) == ' ' {
                x += line.word_spacing;
            }
        }
        x
    }

    pub(super) fn hit_test_glyph(&self, pos: Point) -> Option<u32> {
        // find nearest glyph index for a screen position
        let lines = self.document.lines();
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
            let line = &lines[idx];
            let y0 = screen.point.y;
            let y1 = y0 + line.height;
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
        let line = &lines[line_idx];
        let screen = self.frame.last_line_positions[line_idx]?;
        let rel_x = pos.x - screen.point.x;
        if rel_x <= 0.0 {
            return Some(line.glyphs.start);
        }

        let mut x = 0.0;
        for i in line.glyphs.start..line.glyphs.end {
            let glyph_index = self.document.glyph_at(i as usize).unwrap_or_default();
            let w = self.glyph_cache.get(glyph_index).size().width;
            if rel_x < x + w / 2.0 {
                return Some(i);
            }
            x += w + line.letter_spacing;
            if self.glyph_cache.get_char(glyph_index) == ' ' {
                x += line.word_spacing;
            }
        }
        Some(line.glyphs.end)
    }

    pub(super) fn hit_test_image(&self, pos: Point) -> Option<u32> {
        if self.frame.last_line_positions.is_empty() || self.layout.col_count <= 0.0 {
            return None;
        }

        let max_col = self.layout.col_count as i32 - 1;
        let target_col = (((pos.x - self.layout.col_gap) / (self.layout.col_width + self.layout.col_gap)).floor() as i32).clamp(0, max_col);

        for (line_idx, screen) in self.frame.last_line_positions.iter().enumerate() {
            let Some(screen) = screen else {
                continue;
            };
            if screen.col_index != target_col || line_idx >= self.document.image_fragments_by_line().len() {
                continue;
            }
            for &frag_idx in &self.document.image_fragments_by_line()[line_idx] {
                let frag = &self.document.image_fragments()[frag_idx];
                let (width, height) = self.scaled_image_size(frag);
                let x0 = screen.point.x + frag.offset.x;
                let y0 = screen.point.y + frag.offset.y;
                let x1 = x0 + width;
                let y1 = y0 + height;
                if pos.x >= x0 && pos.x <= x1 && pos.y >= y0 && pos.y <= y1 {
                    return Some(frag.image_idx);
                }
            }
        }
        None
    }
}
