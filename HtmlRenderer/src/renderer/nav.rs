use super::*;
use floem::reactive::SignalGet;
use std::collections::HashSet;

impl HTMLRenderer {
    fn update_history_availability_signals(&self) {
        let len = self.nav.location_history.len();
        let can_back = len > 0 && self.nav.location_history_index > 0;
        let can_forward = len > 0 && self.nav.location_history_index + 1 < len;
        if self.nav.can_history_back.get_untracked() != can_back {
            self.nav.can_history_back.set(can_back);
        }
        if self.nav.can_history_forward.get_untracked() != can_forward {
            self.nav.can_history_forward.set(can_forward);
        }
    }

    fn find_doc_index_by_uri_or_suffix(&self, candidate: &str) -> Option<usize> {
        self.nav.document_uris.iter().position(|uri| uri == candidate).or_else(|| self.nav.document_uris.iter().position(|uri| uri.ends_with(candidate)))
    }

    fn replace_document_state(&mut self, document: DocumentState, glyph_cache: GlyphCache, sync_doc_title: bool) {
        self.document = document;
        self.glyph_cache = glyph_cache;
        if sync_doc_title {
            self.doc_title_signal.set(self.document.title().map(str::to_owned));
        }
        self.document_toc_signal.set(Self::build_document_toc_entries_for(&self.document));
        self.update_toc_anchor_filter();
        self.line_numbers = Self::build_line_number_cache(&mut self.glyph_cache);
        self.image_pipeline = ImagePipeline::new(Arc::new(self.document.images().to_vec()), self.provider.clone());
    }

    fn load_document_for_index(&self, doc_index: usize) -> (DocumentState, GlyphCache) {
        load::load_document(self.provider.clone(), &self.nav.document_uris[doc_index], self.root_font_size, self.layout.col_width)
    }

    fn replace_with_loaded_document(&mut self, doc_index: usize, sync_doc_title: bool) {
        let (document, glyph_cache) = self.load_document_for_index(doc_index);
        self.nav.current_doc_index = doc_index;
        self.replace_document_state(document, glyph_cache, sync_doc_title);
        self.reset_view_state();
    }

    fn jump_to_forward_offset(&mut self, start_offset_y: f64, pending_nav_anchor_update: bool) {
        self.viewport.direction = PaintDirection::Forward;
        self.viewport.reached_end = false;
        self.viewport.back_anchor_end_line = None;
        self.viewport.start_offset_y = start_offset_y;
        self.viewport.end_offset_y = self.viewport.start_offset_y;
        self.id.request_paint();
        self.nav.pending_nav_anchor_update = pending_nav_anchor_update;
    }

    pub(super) fn handle_link_click(&mut self, glyph_idx: u32) -> bool {
        let Some(href_idx) = self.document.link_href_for_glyph(glyph_idx) else {
            return false;
        };
        let href = self.document.string(href_idx).to_string();
        self.navigate_to_href(&href);
        true
    }

    fn resolve_href_target(&self, href: &str) -> Option<(usize, Option<String>)> {
        if let Some(anchor) = href.strip_prefix('#') {
            let fragment = if anchor.is_empty() { None } else { Some(anchor.to_string()) };
            return Some((self.nav.current_doc_index, fragment));
        }

        let (path_part, fragment) = href.split_once('#').unwrap_or((href, ""));
        let fragment = if fragment.is_empty() { None } else { Some(fragment.to_string()) };
        if path_part.is_empty() {
            return Some((self.nav.current_doc_index, fragment));
        }

        let normalized_path = normalize_href_path(path_part);
        if let Some(idx) = self.find_doc_index_by_uri_or_suffix(normalized_path.as_str()) {
            return Some((idx, fragment));
        }
        let base_uri = self.nav.document_uris.get(self.nav.current_doc_index).map(|s: &String| s.as_str()).unwrap_or("");
        let resolved = self.provider.resolve(base_uri, normalized_path.as_str());
        let doc_index = self.find_doc_index_by_uri_or_suffix(resolved.as_str());
        if let Some(idx) = doc_index {
            return Some((idx, fragment));
        }
        println!("resolve_href_target: no match for href '{}' (path='{}', normalized='{}', base='{}')", href, path_part, normalized_path, base_uri);
        None
    }

    pub fn resolve_href(&self, href: &str) -> Option<(usize, Option<String>)> {
        self.resolve_href_target(href)
    }

    pub fn navigate_to_href(&mut self, href: &str) -> bool {
        self.push_current_location_if_missing();

        let from = self.current_location();
        let Some((doc_index, fragment)) = self.resolve_href_target(href) else {
            return false;
        };
        let to = (doc_index, fragment.clone());
        if doc_index != self.nav.current_doc_index {
            self.load_document_at(doc_index, DocAnchor::Start);
        }
        if let Some(fragment) = fragment.as_deref() {
            if !self.jump_to_id(fragment) {
                return false;
            }
        }
        if from != to {
            self.push_history_location(from);
            self.push_history_location(to);
        }
        true
    }

    pub(super) fn navigate_history_back(&mut self) -> bool {
        self.push_current_location_if_missing();
        if self.nav.location_history.is_empty() || self.nav.location_history_index == 0 {
            self.update_history_availability_signals();
            return false;
        }
        self.nav.location_history_index -= 1;
        let target = self.nav.location_history[self.nav.location_history_index].clone();
        let ok = self.navigate_to_location(target);
        self.update_history_availability_signals();
        ok
    }

    pub(super) fn navigate_history_forward(&mut self) -> bool {
        self.push_current_location_if_missing();
        if self.nav.location_history.is_empty() || self.nav.location_history_index + 1 >= self.nav.location_history.len() {
            self.update_history_availability_signals();
            return false;
        }
        self.nav.location_history_index += 1;
        let target = self.nav.location_history[self.nav.location_history_index].clone();
        let ok = self.navigate_to_location(target);
        self.update_history_availability_signals();
        ok
    }

    fn navigate_to_location(&mut self, target: (usize, Option<String>)) -> bool {
        let (doc_index, anchor) = target;
        if doc_index != self.nav.current_doc_index {
            self.load_document_at(doc_index, DocAnchor::Start);
        }
        if let Some(anchor) = anchor.as_deref() {
            self.jump_to_id(anchor)
        } else {
            self.jump_to_forward_offset(0.0, false);
            true
        }
    }

    fn current_location(&self) -> (usize, Option<String>) {
        (self.nav.current_doc_index, self.current_anchor_id())
    }

    fn push_current_location_if_missing(&mut self) {
        let current = self.current_location();
        if self.nav.location_history.is_empty() {
            self.nav.location_history.push(current);
            self.nav.location_history_index = 0;
            self.update_history_availability_signals();
            return;
        }
        if self.nav.location_history.get(self.nav.location_history_index).map(|entry| entry != &current).unwrap_or(true) {
            self.push_history_location(current);
        } else {
            self.update_history_availability_signals();
        }
    }

    fn push_history_location(&mut self, location: (usize, Option<String>)) {
        if self.nav.location_history.get(self.nav.location_history_index).map(|entry| entry == &location).unwrap_or(false) {
            self.update_history_availability_signals();
            return;
        }
        let keep_len = self.nav.location_history_index + 1;
        if keep_len < self.nav.location_history.len() {
            self.nav.location_history.truncate(keep_len);
        }
        self.nav.location_history.push(location);
        self.nav.location_history_index = self.nav.location_history.len().saturating_sub(1);
        self.update_history_availability_signals();
    }

    fn jump_to_id(&mut self, id: &str) -> bool {
        let Some(id_idx) = self.document.lookup_string(id) else {
            println!("jump_to_id: missing id '{}'", id);
            return false;
        };
        let Some(glyph_idx) = self.document.anchor_glyph(id_idx) else {
            if let Some(pos) = self.document.anchor_positions().get(&id_idx).copied() {
                return self.restore_anchor_position(pos.y);
            }
            println!("jump_to_id: no glyph for id '{}'", id);
            return false;
        };
        self.restore_glyph_position(glyph_idx);
        self.nav.pending_nav_anchor_update = false;
        true
    }

    fn restore_anchor_position(&mut self, target_y: f64) -> bool {
        if self.document.lines().is_empty() {
            return false;
        }
        let line_idx = self.find_start_line_at(target_y, 0.1);
        let Some(line) = self.document.line(line_idx) else {
            return false;
        };
        let line_y = line.point.y;
        let line_start = line.glyphs.start;
        let line_end = line.glyphs.end;
        self.jump_to_forward_offset(line_y, false);
        self.nav.nav_anchor_glyph = if line_start < line_end { Some(line_start) } else { None };
        true
    }
    pub(super) fn find_start_line_at(&self, offset_y: f64, epsilon: f64) -> usize {
        // locate first visible line for a given scroll offset
        let lines = self.document.lines();
        if lines.is_empty() {
            return 0;
        }

        let mut lo = 0;
        let mut hi = lines.len();

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = &lines[mid];
            // Add epsilon margin to account for floating point drift
            if line.point.y + line.height <= offset_y + epsilon {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    pub(super) fn find_start_line(&self) -> usize {
        // locate first visible line at current offset
        self.find_start_line_at(self.viewport.start_offset_y, 0.1)
    }

    pub(super) fn load_nav_state() -> Option<(usize, u32)> {
        // load persisted glyph anchor
        let path = Self::nav_state_path()?;
        let raw = fs::read_to_string(path).ok()?;
        Self::parse_nav_state(raw.trim())
    }

    pub(super) fn parse_nav_state(raw: &str) -> Option<(usize, u32)> {
        use crate::document::cfi;

        let trimmed = raw.trim();

        // Try parsing as CFI first (new format)
        if trimmed.starts_with("epubcfi(") {
            // Parse just the spine position to know which document to load
            // The full CFI will be parsed after the document loads
            let doc_idx = cfi::parse_cfi_spine_only(trimmed)?;
            // Return 0 for glyph_idx - will be resolved after document loads
            return Some((doc_idx, 0));
        }

        // Backwards compatibility: old "doc:glyph" format
        if let Some((doc_part, glyph_part)) = trimmed.split_once(':') {
            let doc_idx = doc_part.parse::<usize>().ok()?;
            let glyph_idx = glyph_part.parse::<u32>().ok()?;
            return Some((doc_idx, glyph_idx));
        }

        // Very old format: just glyph index
        trimmed.parse::<u32>().ok().map(|glyph| (0, glyph))
    }

    /// Restore position from a CFI string after document is loaded
    pub fn restore_position_from_cfi(&mut self, cfi: &str) -> bool {
        use crate::document::cfi;

        if let Some((doc_idx, glyph_idx)) = cfi::parse_cfi(&self.document, cfi) {
            if doc_idx == self.nav.current_doc_index {
                self.restore_glyph_position(glyph_idx);
                return true;
            }
        }
        false
    }

    pub(super) fn current_glyph_position(&self) -> Option<u32> {
        // glyph index that anchors current view
        if self.document.lines().is_empty() {
            return None;
        }
        let line_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        self.document.line(line_idx).map(|line| line.glyphs.start)
    }

    pub(super) fn current_anchor_id(&self) -> Option<String> {
        let current_y = self.viewport.start_offset_y;
        let filter = self.nav.toc_anchor_filter.as_ref();

        if let Some(current_glyph) = self.current_glyph_position() {
            if let Some(id_idx) = self.best_anchor_from_glyphs(current_glyph, filter) {
                return Some(self.document.string(id_idx).to_string());
            }
        }

        if let Some(id_idx) = self.best_anchor_from_positions(current_y, filter) {
            return Some(self.document.string(id_idx).to_string());
        }

        if filter.is_some() {
            return None;
        }

        let glyph_idx = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position())?;
        self.best_anchor_from_glyphs(glyph_idx, None).map(|id_idx| self.document.string(id_idx).to_string())
    }

    fn best_anchor_from_glyphs(&self, glyph_limit: u32, filter: Option<&HashSet<u16>>) -> Option<u16> {
        let mut best: Option<(u16, u32)> = None;
        for (id_idx, glyph_idx) in self.document.anchor_glyphs() {
            if filter.is_some_and(|f| !f.contains(id_idx)) {
                continue;
            }
            let is_better = match best {
                None => true,
                Some((_, best_glyph)) => *glyph_idx > best_glyph,
            };
            if *glyph_idx <= glyph_limit && is_better {
                best = Some((*id_idx, *glyph_idx));
            }
        }
        best.map(|(id_idx, _)| id_idx)
    }

    fn best_anchor_from_positions(&self, current_y: f64, filter: Option<&HashSet<u16>>) -> Option<u16> {
        let mut best: Option<(u16, f64, u32)> = None;
        for (id_idx, pos) in self.document.anchor_positions() {
            if filter.is_some_and(|f| !f.contains(id_idx)) {
                continue;
            }
            let is_better = match best {
                None => true,
                Some((_, best_y, best_order)) => pos.y > best_y || (pos.y == best_y && pos.order > best_order),
            };
            if pos.y <= current_y + 0.1 && is_better {
                best = Some((*id_idx, pos.y, pos.order));
            }
        }
        best.map(|(id_idx, _, _)| id_idx)
    }

    pub(super) fn restore_glyph_position(&mut self, glyph_idx: u32) {
        // jump view to a stored glyph index
        let Some(line_idx) = self.find_line_for_glyph(glyph_idx) else {
            return;
        };
        let Some(line) = self.document.line(line_idx) else {
            return;
        };
        self.jump_to_forward_offset(line.point.y, false);
        self.nav.nav_anchor_glyph = Some(glyph_idx);
    }

    pub(super) fn persist_nav_position(&self) {
        use crate::document::cfi;

        // write glyph anchor to global file as CFI
        let Some(path) = Self::nav_state_path() else {
            return;
        };
        let Some(glyph_idx) = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position()) else {
            return;
        };

        // Generate CFI from current position
        let cfi_string = cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx).unwrap_or_else(|| {
            // Fallback to old format if CFI generation fails
            format!("{}:{}", self.nav.current_doc_index, glyph_idx)
        });

        let _ = fs::write(path, cfi_string);
        self.update_nav_signal();
    }

    pub(super) fn update_nav_signal(&self) {
        use crate::document::cfi;

        let glyph = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position());
        let nav_state = (self.nav.current_doc_index, glyph);
        if self.nav.position_update.get_untracked() != nav_state {
            self.nav.position_update.set(nav_state);
        }

        // Update CFI position signal
        if let Some(glyph_idx) = glyph {
            let cfi_string = cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx);
            if self.nav.cfi_position.get_untracked() != cfi_string {
                self.nav.cfi_position.set(cfi_string);
            }
        } else if self.nav.cfi_position.get_untracked().is_some() {
            self.nav.cfi_position.set(None);
        }

        let anchor = self.current_anchor_id();
        let anchor_state = (self.nav.current_doc_index, anchor);
        if self.nav.nav_anchor_signal.get_untracked() != anchor_state {
            self.nav.nav_anchor_signal.set(anchor_state);
        }
    }

    pub(super) fn update_nav_anchor_from_layout(&mut self) {
        self.nav.nav_anchor_glyph = self.current_glyph_position();
    }

    pub(super) fn nav_state_path() -> Option<PathBuf> {
        // global nav state path
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".html-prototype-nav.txt"))
    }

    pub(super) fn find_end_line_at(&self, offset_y: f64, epsilon: f64) -> Option<usize> {
        // locate last line fully visible before a given offset
        let lines = self.document.lines();
        if lines.is_empty() || lines[0].point.y + lines[0].height > offset_y + epsilon {
            return None;
        }

        let mut lo = 0;
        let mut hi = lines.len() - 1;
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            let line = &lines[mid];
            if line.point.y + line.height <= offset_y + epsilon {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Some(lo)
    }

    pub(super) fn next_page(&mut self) {
        // advance forward by one page
        // Don't navigate if already at end
        if self.viewport.reached_end {
            if self.nav.current_doc_index + 1 < self.nav.document_uris.len() {
                self.load_document_at(self.nav.current_doc_index + 1, DocAnchor::Start);
                return;
            }
            return;
        }

        let mut next_start_offset_y = self.viewport.start_offset_y;
        if let Some(end_idx) = self.frame.current_page_end_line {
            let next_idx = end_idx + 1;
            if let Some(line) = self.document.line(next_idx) {
                next_start_offset_y = line.point.y;
            }
        } else if self.viewport.next_start_offset_y > self.viewport.start_offset_y {
            next_start_offset_y = self.viewport.next_start_offset_y;
        }
        self.jump_to_forward_offset(next_start_offset_y, true);
    }

    pub(super) fn next_line(&mut self) {
        let lines = self.document.lines();
        if lines.is_empty() {
            return;
        }
        let current_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        let Some(current_line) = lines.get(current_idx) else {
            return;
        };
        // Move by visual line band: jump to first line strictly after current line bottom.
        let search_y = current_line.point.y + current_line.height + 0.1;
        let target_idx = self.find_start_line_at(search_y, 0.0);
        if target_idx >= lines.len() {
            return;
        }
        if let Some(target_line) = lines.get(target_idx) {
            self.jump_to_forward_offset(target_line.point.y, true);
        }
    }

    pub(super) fn prev_line(&mut self) {
        let lines = self.document.lines();
        if lines.is_empty() {
            return;
        }
        let current_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        let Some(current_line) = lines.get(current_idx) else {
            return;
        };
        if current_line.point.y <= 0.0 && current_idx == 0 {
            return;
        }
        // Move by visual line band: find last line ending at/before current line top.
        // Use a small epsilon so adjacent lines that touch edges are treated as previous.
        let search_y = current_line.point.y.max(0.0);
        let Some(target_idx) = self.find_end_line_at(search_y, 0.1) else {
            return;
        };
        if let Some(target_line) = lines.get(target_idx) {
            self.jump_to_forward_offset(target_line.point.y, true);
        }
    }

    pub(super) fn prev_page(&mut self) {
        // move backward by one page
        if self.frame.current_page_start_line == Some(0) || self.viewport.start_offset_y <= 0.0 {
            if self.nav.current_doc_index > 0 {
                self.load_document_at(self.nav.current_doc_index - 1, DocAnchor::End);
                return;
            }
            self.jump_to_forward_offset(0.0, false);
            return;
        }

        self.viewport.direction = PaintDirection::Backward;
        self.viewport.back_anchor_end_line = self.frame.current_page_start_line.and_then(|idx| idx.checked_sub(1));
        if let Some(anchor_idx) = self.viewport.back_anchor_end_line {
            if let Some(line) = self.document.line(anchor_idx) {
                self.viewport.end_offset_y = line.point.y + line.height;
            }
        } else {
            self.viewport.end_offset_y = self.viewport.start_offset_y;
        }
        self.id.request_paint();
        self.nav.pending_nav_anchor_update = true;
    }

    pub(super) fn next_document(&mut self) {
        if self.nav.current_doc_index + 1 < self.nav.document_uris.len() {
            self.load_document_at(self.nav.current_doc_index + 1, DocAnchor::Start);
        }
    }

    pub(super) fn prev_document(&mut self) {
        if self.nav.current_doc_index > 0 {
            self.load_document_at(self.nav.current_doc_index - 1, DocAnchor::End);
        }
    }

    pub(super) fn relayout_for_column_width(&mut self) {
        let anchor = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position());
        let document = std::mem::take(&mut self.document);
        let glyph_cache = std::mem::take(&mut self.glyph_cache);
        let mut layout_engine = layout::LayoutEngine::new(self.layout.col_width, self.root_font_size as f64);
        let (mut document, glyph_cache) = layout_engine.layout(document, glyph_cache);
        load::build_anchor_positions(&mut document);
        self.replace_document_state(document, glyph_cache, false);
        self.reset_view_state();
        if let Some(glyph_idx) = anchor {
            self.restore_glyph_position(glyph_idx);
        }
        self.id.request_paint();
    }

    pub(super) fn reload_for_font_size(&mut self) {
        let anchor = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position());
        self.replace_with_loaded_document(self.nav.current_doc_index, true);
        if let Some(glyph_idx) = anchor {
            self.restore_glyph_position(glyph_idx);
        }
        self.id.request_paint();
    }

    pub(super) fn set_root_font_size(&mut self, new_size: f32) {
        let clamped = new_size.clamp(8.0, 64.0);
        let rounded = clamped.round();
        if (rounded - self.root_font_size).abs() < f32::EPSILON {
            return;
        }
        self.root_font_size = rounded;
        self.settings.current_font_size.set(self.root_font_size);
        self.reload_for_font_size();
    }

    pub(super) fn set_col_width(&mut self, new_width: f64) {
        let clamped = new_width.clamp(200.0, 1200.0);
        let rounded = clamped.round();
        self.layout.base_col_width = rounded;
        self.settings.current_column_width.set(self.layout.base_col_width);
        let effective = if self.layout.size.width > 0.0 { rounded.min(self.layout.size.width) } else { rounded };
        if (effective - self.layout.col_width).abs() < 0.1 {
            return;
        }
        self.layout.col_width = effective;
        self.relayout_for_column_width();
    }

    pub(super) fn set_scale(&mut self, new_scale: f64) {
        let clamped = new_scale.clamp(0.5, 3.0);
        if (clamped - self.layout.scale).abs() < f64::EPSILON {
            return;
        }
        self.layout.scale = clamped;
        self.settings.current_scale.set(self.layout.scale);
        self.id.request_paint();
    }

    pub(super) fn set_show_line_numbers(&mut self, enabled: bool) {
        if self.show_line_numbers == enabled {
            return;
        }
        self.show_line_numbers = enabled;
        self.settings.current_line_numbers.set(enabled);
        self.id.request_paint();
    }

    pub(super) fn load_document_at(&mut self, doc_index: usize, anchor: DocAnchor) {
        if doc_index >= self.nav.document_uris.len() {
            return;
        }
        self.replace_with_loaded_document(doc_index, true);

        match anchor {
            DocAnchor::Start => {
                self.viewport.direction = PaintDirection::Forward;
                self.viewport.start_offset_y = 0.0;
                self.viewport.end_offset_y = 0.0;
            }
            DocAnchor::End => {
                self.viewport.direction = PaintDirection::Backward;
                if let Some(last) = self.document.lines().last() {
                    self.viewport.end_offset_y = last.point.y + last.height;
                    self.viewport.back_anchor_end_line = Some(self.document.line_count().saturating_sub(1));
                }
            }
            DocAnchor::Glyph(glyph_idx) => {
                self.restore_glyph_position(glyph_idx);
            }
        }

        self.id.request_paint();
        if matches!(anchor, DocAnchor::Glyph(_)) {
            self.persist_nav_position();
        } else {
            self.nav.pending_nav_anchor_update = true;
        }
    }

    pub(super) fn reset_view_state(&mut self) {
        self.viewport = ViewportState::default();
        self.frame = FrameCache::default();
        self.selection = SelectionState::default();
    }

    pub(super) fn update_toc_anchor_filter(&mut self) {
        let Some(anchor_ids) = self.nav.toc_anchor_strings_by_doc.get(self.nav.current_doc_index) else {
            self.nav.toc_anchor_filter = None;
            return;
        };
        if anchor_ids.is_empty() {
            self.nav.toc_anchor_filter = None;
            return;
        }
        let mut filter: HashSet<u16> = HashSet::new();
        for id in anchor_ids {
            if let Some(id_idx) = self.document.lookup_string(id) {
                filter.insert(id_idx);
            }
        }
        self.nav.toc_anchor_filter = if filter.is_empty() { None } else { Some(filter) };
    }
}

fn normalize_href_path(path: &str) -> String {
    if path.starts_with('/') || path.contains(':') {
        return path.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
