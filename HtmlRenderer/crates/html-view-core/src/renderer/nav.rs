use std::collections::HashSet;
use std::env;
use std::fs;
use std::mem;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::{
    CachedDocument, DocAnchor, FrameCache, GlyphId, GlyphShaper, ImagePipeline, LaidOutDocument, LineNumberCache, PaintDirection, RendererCore, RendererEvent, RendererHost, ResourceProvider, SelectionState, TocEntry, ViewportState, load,
};

impl RendererCore {
    pub fn from_source_path_with_nav(host: Rc<dyn RendererHost>, glyph_shaper: &mut impl GlyphShaper, path: PathBuf, nav_state: Option<&str>) -> Self {
        let (provider, document_uris, start_index) = load::collect_documents(path);
        Self::from_provider_with_nav(host, glyph_shaper, provider, document_uris, start_index, nav_state)
    }

    pub fn from_epub_bytes_with_nav(host: Rc<dyn RendererHost>, glyph_shaper: &mut impl GlyphShaper, bytes: Vec<u8>, nav_state: Option<&str>) -> Self {
        let (provider, document_uris, start_index) = load::collect_documents_from_epub_bytes(bytes);
        Self::from_provider_with_nav(host, glyph_shaper, provider, document_uris, start_index, nav_state)
    }

    pub fn from_provider_with_nav(host: Rc<dyn RendererHost>, glyph_shaper: &mut impl GlyphShaper, provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, start_index: usize, nav_state: Option<&str>) -> Self {
        assert!(!document_uris.is_empty(), "No HTML documents found for source");
        let nav_state = nav_state.map(str::to_owned).or_else(|| Self::nav_state_path().and_then(|path| fs::read_to_string(path).ok()));
        let (doc_index, glyph_idx) = nav_state.as_deref().and_then(Self::parse_nav_state).unwrap_or((start_index, 0));
        let doc_index = doc_index.min(document_uris.len().saturating_sub(1));
        let loaded = load::load_document(provider.clone(), &document_uris[doc_index], 20.0, 600.0, glyph_shaper);
        let mut renderer = Self::new(host, loaded, glyph_shaper, provider, document_uris, doc_index);

        if nav_state.as_deref().is_some_and(|state| state.trim().starts_with("epubcfi(") && renderer.restore_position_from_cfi(state.trim())) {
            renderer.update_nav_signal();
            return renderer;
        }
        if glyph_idx > 0 {
            renderer.restore_glyph_position(glyph_idx);
        }
        if renderer.nav.nav_anchor_glyph.is_none() {
            renderer.nav.nav_anchor_glyph = renderer.current_glyph_position();
        }
        renderer.update_nav_signal();
        renderer
    }

    pub fn toc(&self) -> std::io::Result<Option<Vec<TocEntry>>> {
        self.provider.toc()
    }

    pub fn build_document_toc_entries_for(document: &LaidOutDocument) -> Vec<TocEntry> {
        fn convert_nodes(view: html_layout::RenderView<'_>, nodes: &[crate::DocumentTocNode]) -> Vec<TocEntry> {
            nodes.iter().map(|node| TocEntry { title: view.string(node.title).to_string(), link: view.string(node.href).to_string(), children: convert_nodes(view, node.children.as_slice()) }).collect()
        }

        let view = document.render_view();
        convert_nodes(view, view.document_toc_entries())
    }

    pub(crate) fn build_line_number_cache(glyph_shaper: &mut impl GlyphShaper, mut glyph_metrics: crate::GlyphRegistry<'_>) -> LineNumberCache {
        let mut digits: [GlyphId; 10] = [0; 10];
        for (digit, slot) in digits.iter_mut().enumerate() {
            *slot = glyph_shaper.shape_glyph(&mut glyph_metrics, char::from(b'0' + digit as u8), 12.0, 400, crate::FontStyle::Normal, 0x000000FF, None, None).expect("line-number digit glyph shaping should succeed");
        }
        let space = glyph_shaper.shape_glyph(&mut glyph_metrics, ' ', 12.0, 400, crate::FontStyle::Normal, 0x000000FF, None, None).expect("line-number space glyph shaping should succeed");
        LineNumberCache { digits, space, gap: 8.0 }
    }

    fn update_history_availability_signals(&mut self) {
        let len = self.nav.location_history.len();
        let can_back = len > 0 && self.nav.location_history_index > 0;
        let can_forward = len > 0 && self.nav.location_history_index + 1 < len;
        if self.nav.last_history_availability != Some((can_back, can_forward)) {
            self.nav.last_history_availability = Some((can_back, can_forward));
            self.host.emit(RendererEvent::HistoryAvailability { back: can_back, forward: can_forward });
        }
    }

    fn find_doc_index_by_uri_or_suffix(&self, candidate: &str) -> Option<usize> {
        self.nav.document_uris.iter().position(|uri| uri == candidate).or_else(|| self.nav.document_uris.iter().position(|uri| uri.ends_with(candidate)))
    }

    fn replace_document_state(&mut self, loaded: load::LoadedRenderDocument, sync_doc_title: bool, glyph_shaper: &mut impl GlyphShaper) {
        self.document = loaded.document;
        self.pipeline_session = loaded.session;
        self.pipeline_inputs = loaded.inputs;
        self.root_font_size = self.pipeline_inputs.style_environment.root_font_size as f32;
        if sync_doc_title {
            self.host.emit(RendererEvent::TitleChanged(self.document.render_view().title().map(str::to_owned)));
        }
        self.host.emit(RendererEvent::TocChanged(Self::build_document_toc_entries_for(&self.document)));
        self.update_toc_anchor_filter();
        let image_resources = self.document.render_view().images().to_vec();
        self.line_numbers = Self::build_line_number_cache(glyph_shaper, self.document.auxiliary_glyph_registry());
        self.image_pipeline = ImagePipeline::new(Arc::new(image_resources), self.provider.clone());
    }

    fn cache_current_document(&mut self) {
        if self.document_cache_capacity == 0 {
            return;
        }

        let current_index = self.nav.current_doc_index;
        let cached = CachedDocument { session: mem::replace(&mut self.pipeline_session, html_pipeline::PipelineSession::new(self.provider.clone())), inputs: self.pipeline_inputs.clone(), document: self.document.clone() };
        self.insert_cached_document(current_index, cached);
    }

    fn cache_order_remove(&mut self, doc_index: usize) {
        self.document_cache_order.retain(|cached_index| *cached_index != doc_index);
    }

    fn insert_cached_document(&mut self, doc_index: usize, cached: CachedDocument) {
        if self.document_cache_capacity == 0 {
            return;
        }

        if self.document_cache.contains_key(&doc_index) {
            self.document_cache_order.retain(|cached_index| *cached_index != doc_index);
        }

        self.document_cache.insert(doc_index, cached);
        self.document_cache_order.push_back(doc_index);

        while self.document_cache_order.len() > self.document_cache_capacity {
            if let Some(evicted) = self.document_cache_order.pop_front() {
                self.document_cache.remove(&evicted);
            }
        }
    }

    fn take_cached_document(&mut self, doc_index: usize) -> Option<CachedDocument> {
        self.cache_order_remove(doc_index);
        self.document_cache.remove(&doc_index)
    }

    fn rebased_inputs_for_doc_index(&self, doc_index: usize, source_inputs: &html_pipeline::PipelineInputs) -> html_pipeline::PipelineInputs {
        let mut next_inputs = source_inputs.clone();
        if let Some(uri) = self.nav.document_uris.get(doc_index) {
            next_inputs.base_uri = uri.clone();
        }
        next_inputs.style_environment = self.pipeline_inputs.style_environment;
        next_inputs.layout = html_pipeline::LayoutConstraints { viewport_width: self.pipeline_inputs.layout.viewport_width, line_height: self.pipeline_inputs.style_environment.root_font_size as f64 };
        next_inputs.resource_revision = self.pipeline_inputs.resource_revision;
        next_inputs.stylesheet_revision = self.pipeline_inputs.stylesheet_revision;
        next_inputs.font_environment = self.pipeline_inputs.font_environment;
        next_inputs.image_metrics_revision = self.pipeline_inputs.image_metrics_revision;
        next_inputs.image_metrics = self.pipeline_inputs.image_metrics.clone();
        next_inputs.paint = self.pipeline_inputs.paint;
        next_inputs
    }

    fn load_document_for_index(&mut self, glyph_shaper: &mut impl GlyphShaper, doc_index: usize) -> load::LoadedRenderDocument {
        let uri = self.nav.document_uris[doc_index].clone();
        load::load_document(self.provider.clone(), &uri, self.root_font_size, self.pipeline_inputs.layout.viewport_width, glyph_shaper)
    }

    fn fragment_part(fragment: &str) -> Option<String> {
        (!fragment.is_empty()).then(|| fragment.to_string())
    }

    fn replace_with_loaded_document(&mut self, glyph_shaper: &mut impl GlyphShaper, doc_index: usize, sync_doc_title: bool) {
        let document = self.load_document_for_index(glyph_shaper, doc_index);
        self.nav.current_doc_index = doc_index;
        self.replace_document_state(document, sync_doc_title, glyph_shaper);
        self.reset_view_state();
    }

    fn jump_to_forward_offset(&mut self, start_offset_y: f64, pending_nav_anchor_update: bool) {
        self.viewport.direction = PaintDirection::Forward;
        self.viewport.reached_end = false;
        self.viewport.back_anchor_end_line = None;
        self.viewport.start_offset_y = start_offset_y;
        self.viewport.end_offset_y = self.viewport.start_offset_y;
        self.host.request_repaint();
        self.nav.pending_nav_anchor_update = pending_nav_anchor_update;
    }

    pub fn handle_link_click(&mut self, glyph_shaper: &mut impl GlyphShaper, glyph_idx: u32) -> bool {
        let view = self.document.render_view();
        let Some(href_idx) = view.link_href_for_glyph(glyph_idx) else {
            return false;
        };
        let href = view.string(href_idx).to_string();
        self.navigate_to_href(glyph_shaper, &href);
        true
    }

    fn resolve_doc_index_for_path(&self, normalized_path: &str) -> Option<usize> {
        self.find_doc_index_by_uri_or_suffix(normalized_path).or_else(|| {
            let base_uri = self.nav.document_uris.get(self.nav.current_doc_index).map(|s: &String| s.as_str()).unwrap_or("");
            let resolved = self.provider.resolve(base_uri, normalized_path);
            self.find_doc_index_by_uri_or_suffix(resolved.as_str())
        })
    }

    fn resolve_href_target(&self, href: &str) -> Option<(usize, Option<String>)> {
        if let Some(anchor) = href.strip_prefix('#') {
            return Some((self.nav.current_doc_index, Self::fragment_part(anchor)));
        }

        let (path_part, fragment) = href.split_once('#').unwrap_or((href, ""));
        let fragment = Self::fragment_part(fragment);
        if path_part.is_empty() {
            return Some((self.nav.current_doc_index, fragment));
        }

        let normalized_path = normalize_href_path(path_part);
        let Some(idx) = self.resolve_doc_index_for_path(normalized_path.as_str()) else {
            let base_uri = self.nav.document_uris.get(self.nav.current_doc_index).map(|s: &String| s.as_str()).unwrap_or("");
            println!("resolve_href_target: no match for href '{}' (path='{}', normalized='{}', base='{}')", href, path_part, normalized_path, base_uri);
            return None;
        };
        Some((idx, fragment))
    }

    pub fn resolve_href(&self, href: &str) -> Option<(usize, Option<String>)> {
        self.resolve_href_target(href)
    }

    pub fn navigate_to_href(&mut self, glyph_shaper: &mut impl GlyphShaper, href: &str) -> bool {
        self.push_current_location_if_missing();

        let from = self.current_location();
        let Some((doc_index, fragment)) = self.resolve_href_target(href) else {
            return false;
        };
        let to = (doc_index, fragment.clone());
        if doc_index != self.nav.current_doc_index {
            self.load_document_at(glyph_shaper, doc_index, DocAnchor::Start);
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

    fn navigate_history_step(&mut self, glyph_shaper: &mut impl GlyphShaper, step: isize) -> bool {
        let len = self.nav.location_history.len() as isize;
        let next_index = self.nav.location_history_index as isize + step;
        if next_index < 0 || next_index >= len {
            self.update_history_availability_signals();
            return false;
        }

        self.nav.location_history_index = next_index as usize;
        let target = self.nav.location_history[self.nav.location_history_index].clone();
        let ok = self.navigate_to_location(glyph_shaper, target);
        self.update_history_availability_signals();
        ok
    }

    pub fn navigate_history_back(&mut self, glyph_shaper: &mut impl GlyphShaper) -> bool {
        self.navigate_history_step(glyph_shaper, -1)
    }

    pub fn navigate_history_forward(&mut self, glyph_shaper: &mut impl GlyphShaper) -> bool {
        self.navigate_history_step(glyph_shaper, 1)
    }

    fn navigate_to_location(&mut self, glyph_shaper: &mut impl GlyphShaper, target: (usize, Option<String>)) -> bool {
        let (doc_index, anchor) = target;
        if doc_index != self.nav.current_doc_index {
            self.load_document_at(glyph_shaper, doc_index, DocAnchor::Start);
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
        if self.nav.location_history.get(self.nav.location_history_index) != Some(&current) {
            self.push_history_location(current);
        } else {
            self.update_history_availability_signals();
        }
    }

    fn push_history_location(&mut self, location: (usize, Option<String>)) {
        if self.nav.location_history.get(self.nav.location_history_index) == Some(&location) {
            self.update_history_availability_signals();
            return;
        }
        let keep_len = self.nav.location_history_index + 1;
        self.nav.location_history.truncate(keep_len);
        self.nav.location_history.push(location);
        self.nav.location_history_index = self.nav.location_history.len().saturating_sub(1);
        self.update_history_availability_signals();
    }

    fn jump_to_id(&mut self, id: &str) -> bool {
        let view = self.document.render_view();
        let Some(id_idx) = view.lookup_string(id) else {
            println!("jump_to_id: missing id '{}'", id);
            return false;
        };
        let Some(glyph_idx) = view.anchor_glyph(id_idx) else {
            if let Some(pos) = view.anchor_position(id_idx) {
                return self.restore_anchor_position(pos.y());
            }
            println!("jump_to_id: no glyph for id '{}'", id);
            return false;
        };
        self.restore_glyph_position(glyph_idx);
        self.nav.pending_nav_anchor_update = false;
        true
    }

    fn restore_anchor_position(&mut self, target_y: f64) -> bool {
        if self.document.render_view().lines().is_empty() {
            return false;
        }
        let line_idx = self.find_start_line_at(target_y, 0.1);
        let Some(line) = self.document.render_view().line(line_idx) else {
            return false;
        };
        let line_y = line.point().y;
        let line_start = line.start();
        let line_end = line.end();
        self.jump_to_forward_offset(line_y, false);
        self.nav.nav_anchor_glyph = if line_start < line_end { Some(line_start) } else { None };
        true
    }
    pub fn find_start_line_at(&self, offset_y: f64, epsilon: f64) -> usize {
        let lines = self.document.render_view().lines();
        if lines.is_empty() {
            return 0;
        }

        let mut lo = 0;
        let mut hi = lines.len();

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let line = lines.get(mid).expect("binary-search index must be within line count");
            if line.point().y + line.height() <= offset_y + epsilon {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    pub fn find_start_line(&self) -> usize {
        self.find_start_line_at(self.viewport.start_offset_y, 0.1)
    }

    pub fn parse_nav_state(raw: &str) -> Option<(usize, u32)> {
        use html_layout::cfi;

        let trimmed = raw.trim();

        if trimmed.starts_with("epubcfi(") {
            let doc_idx = cfi::parse_cfi_spine_only(trimmed)?;
            return Some((doc_idx, 0));
        }

        if let Some((doc_part, glyph_part)) = trimmed.split_once(':') {
            let doc_idx = doc_part.parse::<usize>().ok()?;
            let glyph_idx = glyph_part.parse::<u32>().ok()?;
            return Some((doc_idx, glyph_idx));
        }

        trimmed.parse::<u32>().ok().map(|glyph| (0, glyph))
    }

    pub fn restore_position_from_cfi(&mut self, cfi: &str) -> bool {
        use html_layout::cfi;

        if let Some((doc_idx, glyph_idx)) = cfi::parse_cfi(&self.document, cfi) {
            if doc_idx == self.nav.current_doc_index {
                self.restore_glyph_position(glyph_idx);
                return true;
            }
        }
        false
    }

    pub fn current_cfi(&self) -> Option<String> {
        let glyph_idx = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position())?;
        html_layout::cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx)
    }

    pub fn current_glyph_position(&self) -> Option<u32> {
        if self.document.render_view().lines().is_empty() {
            return None;
        }
        let line_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        self.document.render_view().line(line_idx).map(|line| line.start())
    }

    pub fn current_anchor_id(&self) -> Option<String> {
        let current_y = self.viewport.start_offset_y;
        let filter = self.nav.toc_anchor_filter.as_ref();

        if let Some(current_glyph) = self.current_glyph_position() {
            if let Some(id_idx) = self.best_anchor_from_glyphs(current_glyph, filter) {
                return Some(self.document.render_view().string(id_idx).to_string());
            }
        }

        if let Some(id_idx) = self.best_anchor_from_positions(current_y, filter) {
            return Some(self.document.render_view().string(id_idx).to_string());
        }

        if filter.is_some() {
            return None;
        }

        let glyph_idx = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position())?;
        self.best_anchor_from_glyphs(glyph_idx, None).map(|id_idx| self.document.render_view().string(id_idx).to_string())
    }

    fn best_anchor_from_glyphs(&self, glyph_limit: u32, filter: Option<&HashSet<u16>>) -> Option<u16> {
        let mut best: Option<(u16, u32)> = None;
        for (id_idx, glyph_idx) in self.document.render_view().anchor_glyphs() {
            if filter.is_some_and(|f| !f.contains(&id_idx)) {
                continue;
            }
            let is_better = match best {
                None => true,
                Some((_, best_glyph)) => glyph_idx > best_glyph,
            };
            if glyph_idx <= glyph_limit && is_better {
                best = Some((id_idx, glyph_idx));
            }
        }
        best.map(|(id_idx, _)| id_idx)
    }

    fn best_anchor_from_positions(&self, current_y: f64, filter: Option<&HashSet<u16>>) -> Option<u16> {
        let mut best: Option<(u16, f64, u32)> = None;
        for (id_idx, pos) in self.document.render_view().anchor_positions().iter() {
            if filter.is_some_and(|f| !f.contains(&id_idx)) {
                continue;
            }
            let is_better = match best {
                None => true,
                Some((_, best_y, best_order)) => pos.y() > best_y || (pos.y() == best_y && pos.order() > best_order),
            };
            if pos.y() <= current_y + 0.1 && is_better {
                best = Some((id_idx, pos.y(), pos.order()));
            }
        }
        best.map(|(id_idx, _, _)| id_idx)
    }

    pub fn restore_glyph_position(&mut self, glyph_idx: u32) {
        let Some(line_idx) = self.find_line_for_glyph(glyph_idx) else {
            return;
        };
        let Some(line) = self.document.render_view().line(line_idx) else {
            return;
        };
        self.jump_to_forward_offset(line.point().y, false);
        self.nav.nav_anchor_glyph = Some(glyph_idx);
    }

    pub(crate) fn persist_nav_position(&mut self) {
        use html_layout::cfi;

        let Some(path) = Self::nav_state_path() else {
            return;
        };
        let Some(glyph_idx) = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position()) else {
            return;
        };

        let cfi_string = cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx).unwrap_or_else(|| format!("{}:{}", self.nav.current_doc_index, glyph_idx));

        let _ = fs::write(path, cfi_string);
        self.update_nav_signal();
    }

    pub(crate) fn update_nav_signal(&mut self) {
        use html_layout::cfi;

        let glyph = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position());
        let nav_state = (self.nav.current_doc_index, glyph);
        if self.nav.last_position_update != Some(nav_state) {
            self.nav.last_position_update = Some(nav_state);
            self.host.emit(RendererEvent::PositionChanged { doc: nav_state.0, glyph: nav_state.1 });
        }

        let cfi_string = glyph.and_then(|glyph_idx| cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx));
        if self.nav.last_cfi_position.as_ref() != Some(&cfi_string) {
            self.nav.last_cfi_position = Some(cfi_string.clone());
            self.host.emit(RendererEvent::CfiChanged(cfi_string));
        }

        let anchor = self.current_anchor_id();
        let anchor_state = (self.nav.current_doc_index, anchor);
        if self.nav.last_nav_anchor.as_ref() != Some(&anchor_state) {
            self.nav.last_nav_anchor = Some(anchor_state.clone());
            self.host.emit(RendererEvent::NavAnchorChanged { doc: anchor_state.0, anchor: anchor_state.1 });
        }
    }

    pub(crate) fn update_nav_anchor_from_layout(&mut self) {
        self.nav.nav_anchor_glyph = self.current_glyph_position();
    }

    pub fn nav_state_path() -> Option<PathBuf> {
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".html-prototype-nav.txt"))
    }

    pub fn find_end_line_at(&self, offset_y: f64, epsilon: f64) -> Option<usize> {
        let lines = self.document.render_view().lines();
        let Some(first_line) = lines.first() else {
            return None;
        };
        if first_line.point().y + first_line.height() > offset_y + epsilon {
            return None;
        }

        let mut lo = 0;
        let mut hi = lines.len() - 1;
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            let line = lines.get(mid).expect("binary-search index must be within line count");
            if line.point().y + line.height() <= offset_y + epsilon {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Some(lo)
    }

    pub fn next_page(&mut self, glyph_shaper: &mut impl GlyphShaper) {
        if self.viewport.reached_end {
            if self.nav.current_doc_index + 1 < self.nav.document_uris.len() {
                self.load_document_at(glyph_shaper, self.nav.current_doc_index + 1, DocAnchor::Start);
                return;
            }
            return;
        }

        let mut next_start_offset_y = self.viewport.start_offset_y;
        if let Some(end_idx) = self.frame.current_page_end_line {
            let next_idx = end_idx + 1;
            if let Some(line) = self.document.render_view().line(next_idx) {
                next_start_offset_y = line.point().y;
            }
        } else if self.viewport.next_start_offset_y > self.viewport.start_offset_y {
            next_start_offset_y = self.viewport.next_start_offset_y;
        }
        self.jump_to_forward_offset(next_start_offset_y, true);
    }

    pub fn next_line(&mut self) {
        let lines = self.document.render_view().lines();
        if lines.is_empty() {
            return;
        }
        let current_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        let Some(current_line) = lines.get(current_idx) else {
            return;
        };
        let search_y = current_line.point().y + current_line.height() + 0.1;
        let target_idx = self.find_start_line_at(search_y, 0.0);
        if target_idx >= lines.len() {
            return;
        }
        if let Some(target_line) = lines.get(target_idx) {
            self.jump_to_forward_offset(target_line.point().y, true);
        }
    }

    pub fn prev_line(&mut self) {
        let lines = self.document.render_view().lines();
        if lines.is_empty() {
            return;
        }
        let current_idx = self.frame.current_page_start_line.unwrap_or_else(|| self.find_start_line());
        let Some(current_line) = lines.get(current_idx) else {
            return;
        };
        if current_line.point().y <= 0.0 && current_idx == 0 {
            return;
        }
        let search_y = current_line.point().y.max(0.0);
        let Some(target_idx) = self.find_end_line_at(search_y, 0.1) else {
            return;
        };
        if let Some(target_line) = lines.get(target_idx) {
            self.jump_to_forward_offset(target_line.point().y, true);
        }
    }

    pub fn prev_page(&mut self, glyph_shaper: &mut impl GlyphShaper) {
        if self.frame.current_page_start_line == Some(0) || self.viewport.start_offset_y <= 0.0 {
            if self.nav.current_doc_index > 0 {
                self.load_document_at(glyph_shaper, self.nav.current_doc_index - 1, DocAnchor::End);
                return;
            }
            self.jump_to_forward_offset(0.0, false);
            return;
        }

        self.viewport.direction = PaintDirection::Backward;
        self.viewport.back_anchor_end_line = self.frame.current_page_start_line.and_then(|idx| idx.checked_sub(1));
        if let Some(anchor_idx) = self.viewport.back_anchor_end_line {
            if let Some(line) = self.document.render_view().line(anchor_idx) {
                self.viewport.end_offset_y = line.point().y + line.height();
            }
        } else {
            self.viewport.end_offset_y = self.viewport.start_offset_y;
        }
        self.host.request_repaint();
        self.nav.pending_nav_anchor_update = true;
    }

    pub fn next_document(&mut self, glyph_shaper: &mut impl GlyphShaper) {
        if self.nav.current_doc_index + 1 < self.nav.document_uris.len() {
            self.load_document_at(glyph_shaper, self.nav.current_doc_index + 1, DocAnchor::Start);
        }
    }

    pub fn prev_document(&mut self, glyph_shaper: &mut impl GlyphShaper) {
        if self.nav.current_doc_index > 0 {
            self.load_document_at(glyph_shaper, self.nav.current_doc_index - 1, DocAnchor::End);
        }
    }

    pub fn relayout_for_column_width(&mut self, glyph_shaper: &mut impl GlyphShaper) {
        let next = self.next_pipeline_inputs_for_layout(self.layout.col_width);
        let _ = self.apply_pipeline_inputs(next, glyph_shaper);
    }

    pub fn set_root_font_size(&mut self, glyph_shaper: &mut impl GlyphShaper, new_size: f32) {
        let clamped = new_size.clamp(8.0, 64.0);
        let rounded = clamped.round();
        if (rounded - self.root_font_size).abs() < f32::EPSILON {
            return;
        }
        self.root_font_size = rounded;
        self.host.emit(RendererEvent::FontSizeChanged(self.root_font_size));
        let next = self.next_pipeline_inputs_for_style(self.root_font_size);
        self.apply_pipeline_inputs(next, glyph_shaper);
    }

    pub fn set_col_width(&mut self, glyph_shaper: &mut impl GlyphShaper, new_width: f64) {
        let clamped = new_width.clamp(200.0, 1200.0);
        let rounded = clamped.round();
        let base_changed = (rounded - self.layout.base_col_width).abs() >= f64::EPSILON;
        let effective = if self.layout.size.width > 0.0 { rounded.min(self.layout.size.width) } else { rounded };
        let effective_changed = (effective - self.layout.col_width).abs() >= 0.1;
        if !base_changed && !effective_changed {
            return;
        }
        if base_changed {
            self.layout.base_col_width = rounded;
            self.host.emit(RendererEvent::ColumnWidthChanged(self.layout.base_col_width));
        }
        if !effective_changed {
            return;
        }
        self.layout.col_width = effective;
        self.relayout_for_column_width(glyph_shaper);
    }

    pub fn set_scale(&mut self, new_scale: f64) {
        let clamped = new_scale.clamp(0.5, 3.0);
        if (clamped - self.layout.scale).abs() < f64::EPSILON {
            return;
        }
        self.layout.scale = clamped;
        self.host.emit(RendererEvent::ScaleChanged(self.layout.scale));
        self.host.request_repaint();
    }

    pub fn set_show_line_numbers(&mut self, enabled: bool) {
        if self.show_line_numbers == enabled {
            return;
        }
        self.show_line_numbers = enabled;
        self.host.emit(RendererEvent::LineNumbersChanged(enabled));
        self.host.request_repaint();
    }

    pub fn load_document_at(&mut self, glyph_shaper: &mut impl GlyphShaper, doc_index: usize, anchor: DocAnchor) {
        if doc_index >= self.nav.document_uris.len() {
            return;
        }

        if doc_index == self.nav.current_doc_index {
            match anchor {
                DocAnchor::Start => {
                    self.viewport.direction = PaintDirection::Forward;
                    self.viewport.start_offset_y = 0.0;
                    self.viewport.end_offset_y = 0.0;
                }
                DocAnchor::End => {
                    self.viewport.direction = PaintDirection::Backward;
                    if let Some(last) = self.document.render_view().lines().last() {
                        self.viewport.end_offset_y = last.point().y + last.height();
                        self.viewport.reached_end = true;
                        self.viewport.back_anchor_end_line = Some(self.document.render_view().line_count().saturating_sub(1));
                    }
                }
                DocAnchor::Glyph(glyph_idx) => {
                    self.restore_glyph_position(glyph_idx);
                }
            }
            self.host.request_repaint();
            if matches!(anchor, DocAnchor::Glyph(_)) {
                self.persist_nav_position();
            } else {
                self.nav.pending_nav_anchor_update = true;
            }
            return;
        }

        self.cache_current_document();

        if let Some(cached) = self.take_cached_document(doc_index) {
            let requested_inputs = self.rebased_inputs_for_doc_index(doc_index, &cached.inputs);
            if cached.inputs == requested_inputs {
                self.nav.current_doc_index = doc_index;
                self.replace_document_state(load::LoadedRenderDocument { session: cached.session, inputs: cached.inputs, document: cached.document, source: requested_inputs.source }, true, glyph_shaper);
            } else {
                self.nav.current_doc_index = doc_index;
                let requested_source = requested_inputs.source.clone();
                self.replace_document_state(load::LoadedRenderDocument { session: cached.session, inputs: cached.inputs, document: cached.document, source: requested_source }, false, glyph_shaper);
                if !self.apply_pipeline_inputs(requested_inputs, glyph_shaper) {
                    let fallback = self.load_document_for_index(glyph_shaper, doc_index);
                    self.nav.current_doc_index = doc_index;
                    self.replace_document_state(fallback, true, glyph_shaper);
                }
            }
            self.reset_view_state();
        } else {
            let document = self.load_document_for_index(glyph_shaper, doc_index);
            self.nav.current_doc_index = doc_index;
            self.replace_document_state(document, true, glyph_shaper);
            self.reset_view_state();
        }

        match anchor {
            DocAnchor::Start => {
                self.viewport.direction = PaintDirection::Forward;
                self.viewport.start_offset_y = 0.0;
                self.viewport.end_offset_y = 0.0;
            }
            DocAnchor::End => {
                self.viewport.direction = PaintDirection::Backward;
                if let Some(last) = self.document.render_view().lines().last() {
                    self.viewport.end_offset_y = last.point().y + last.height();
                    self.viewport.back_anchor_end_line = Some(self.document.render_view().line_count().saturating_sub(1));
                }
            }
            DocAnchor::Glyph(glyph_idx) => {
                self.restore_glyph_position(glyph_idx);
            }
        }

        self.host.request_repaint();
        if matches!(anchor, DocAnchor::Glyph(_)) {
            self.persist_nav_position();
        } else {
            self.nav.pending_nav_anchor_update = true;
        }
    }

    pub fn reset_view_state(&mut self) {
        self.viewport = ViewportState::default();
        self.frame = FrameCache::default();
        self.selection = SelectionState::default();
    }

    pub fn update_toc_anchor_filter(&mut self) {
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
            if let Some(id_idx) = self.document.render_view().lookup_string(id) {
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use kurbo::{Point, Size};

    use crate::{FileSystemProvider, FontStyle, GlyphId, GlyphMetric, GlyphShaper, RecordingPainter, RendererCore, RendererEvent, RendererHost, load};

    #[derive(Default)]
    struct TestShaper {
        glyphs: HashMap<(char, u32), GlyphId>,
    }

    impl GlyphShaper for TestShaper {
        fn reset(&mut self) {
            self.glyphs.clear();
        }

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut crate::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: FontStyle, _color: u32, _font_family: Option<crate::StyleStringId>, _family: Option<&str>,
        ) -> Result<GlyphId, crate::layout::ShapeError> {
            let key = (ch, font_size.to_bits());
            if let Some(&glyph) = self.glyphs.get(&key) {
                return Ok(glyph);
            }
            let metric = GlyphMetric::try_new(ch, font_size * 0.5, font_size * 0.75, font_size * 0.25, font_size * 0.75).map_err(crate::layout::ShapeError::rejected_metric)?;
            let glyph = glyph_metrics.register(metric)?;
            self.glyphs.insert(key, glyph);
            Ok(glyph)
        }
    }

    #[derive(Default)]
    struct TestHost {
        events: RefCell<Vec<RendererEvent>>,
        repaint_requests: Cell<usize>,
        clipboard_text: RefCell<Option<String>>,
    }

    impl RendererHost for TestHost {
        fn request_repaint(&self) {
            self.repaint_requests.set(self.repaint_requests.get() + 1);
        }
        fn request_style(&self) {}
        fn schedule(&self, _delay: Duration, callback: Box<dyn FnOnce() + Send>) {
            callback();
        }
        fn schedule_repaint(&self, _delay: Duration) {}
        fn set_clipboard(&self, text: &str) -> Result<(), String> {
            *self.clipboard_text.borrow_mut() = Some(text.to_owned());
            Ok(())
        }
        fn set_clipboard_image(&self, _width: usize, _height: usize, _rgba: Vec<u8>) -> Result<(), String> {
            Ok(())
        }
        fn emit(&self, event: RendererEvent) {
            self.events.borrow_mut().push(event);
        }
    }

    #[derive(Default)]
    struct CountingProvider {
        documents: HashMap<String, String>,
        read_count: Arc<AtomicUsize>,
    }

    impl CountingProvider {
        fn new(documents: Vec<(&str, &str)>) -> (Arc<Self>, Arc<AtomicUsize>) {
            let read_count = Arc::new(AtomicUsize::new(0));
            let mut mapped = HashMap::new();
            for (uri, body) in documents {
                mapped.insert(uri.to_owned(), body.to_owned());
            }
            (Arc::new(Self { documents: mapped, read_count: read_count.clone() }), read_count)
        }
    }

    impl crate::ResourceProvider for CountingProvider {
        fn read_bytes(&self, uri: &str) -> std::io::Result<Vec<u8>> {
            self.read_count.fetch_add(1, Ordering::Relaxed);
            self.documents.get(uri).cloned().map(|value| value.into_bytes()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, uri.to_owned()))
        }

        fn exists(&self, uri: &str) -> bool {
            self.documents.contains_key(uri)
        }

        fn resolve(&self, _base: &str, href: &str) -> String {
            href.to_owned()
        }

        fn list_html_candidates(&self, _root: &str) -> std::io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn core_loads_and_navigates_between_documents_without_a_window_backend() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let fixture = std::env::temp_dir().join(format!("html-view-core-nav-{nonce}"));
        std::fs::create_dir_all(&fixture).unwrap();
        let first = fixture.join("one.html");
        let second = fixture.join("two.html");
        std::fs::write(&first, "<html><body><a href=\"two.html#target\">Next</a></body></html>").unwrap();
        std::fs::write(&second, "<html><body><h1 id=\"target\">Target</h1></body></html>").unwrap();

        let provider: Arc<dyn crate::ResourceProvider> = Arc::new(FileSystemProvider::new());
        let uris = vec![first.to_string_lossy().into_owned(), second.to_string_lossy().into_owned()];
        let mut shaper = TestShaper::default();
        let host = Rc::new(TestHost::default());
        let mut core = RendererCore::from_provider_with_nav(host.clone(), &mut shaper, provider, uris, 0, Some("0:0"));

        core.configure_layout_for_viewport(&mut shaper, Size::new(600.0, 800.0));
        core.paint_forward(&mut RecordingPainter::default());
        let view = core.document.render_view();
        let linked_glyph = (0..view.glyph_count() as u32).find(|glyph| view.link_href_for_glyph(*glyph).is_some()).expect("fixture should contain a linked glyph");
        let line_idx = core.find_line_for_glyph(linked_glyph).expect("linked glyph should be laid out");
        let screen = core.frame.last_line_positions[line_idx].expect("linked line should be painted");
        let line = core.document.render_view().line(line_idx).expect("linked line should be present");
        let position = Point::new(screen.point.x + core.glyph_x_in_line(&line, linked_glyph) + 0.1, screen.point.y + line.height() / 2.0);
        assert!(core.link_at(position));
        assert!(core.begin_selection_at(position, false));
        assert_eq!(core.finish_selection(), Some(linked_glyph));

        assert!(core.handle_link_click(&mut shaper, linked_glyph));
        assert_eq!(core.nav.current_doc_index, 1);
        assert!(core.current_anchor_id().as_deref() == Some("target") || core.nav.nav_anchor_glyph.is_some());
        assert!(core.navigate_history_back(&mut shaper));
        assert_eq!(core.nav.current_doc_index, 0);
        assert!(core.navigate_history_forward(&mut shaper));
        assert_eq!(core.nav.current_doc_index, 1);
        assert!(host.events.borrow().iter().any(|event| matches!(event, RendererEvent::TitleChanged(_))));

        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn commands_preserve_search_selection_and_settings_without_a_window_backend() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let fixture = std::env::temp_dir().join(format!("html-view-core-commands-{nonce}"));
        std::fs::create_dir_all(&fixture).unwrap();
        let document_path = fixture.join("index.html");
        std::fs::write(&document_path, "<html><head><title>Commands</title></head><body><p><strong>Needle</strong> and needle.</p></body></html>").unwrap();

        let provider: Arc<dyn crate::ResourceProvider> = Arc::new(FileSystemProvider::new());
        let uris = vec![document_path.to_string_lossy().into_owned()];
        let mut shaper = TestShaper::default();
        let loaded = load::load_document(provider.clone(), &uris[0], 16.0, 600.0, &mut shaper);
        let host = Rc::new(TestHost::default());
        let mut core = RendererCore::new(host.clone(), loaded, &mut shaper, provider, uris, 0);

        core.apply(&mut shaper, crate::RendererCommand::SetColumnWidth(480.0));
        core.apply(&mut shaper, crate::RendererCommand::SetFontSize(18.0));
        core.apply(&mut shaper, crate::RendererCommand::SetScale(1.5));
        core.apply(&mut shaper, crate::RendererCommand::SetLineNumbers(false));
        assert_eq!(core.layout.base_col_width, 480.0);
        assert_eq!(core.root_font_size, 18.0);
        assert_eq!(core.layout.scale, 1.5);
        assert!(!core.show_line_numbers);
        let width_event_count = host.events.borrow().iter().filter(|event| matches!(event, RendererEvent::ColumnWidthChanged(_))).count();
        let repaint_count = host.repaint_requests.get();
        core.apply(&mut shaper, crate::RendererCommand::SetColumnWidth(480.0));
        assert_eq!(host.events.borrow().iter().filter(|event| matches!(event, RendererEvent::ColumnWidthChanged(_))).count(), width_event_count,);
        assert_eq!(host.repaint_requests.get(), repaint_count);

        core.apply(&mut shaper, crate::RendererCommand::SetSearchActive(true));
        core.apply(&mut shaper, crate::RendererCommand::SetSearchQuery("needle".to_string()));
        assert_eq!(core.highlight.matches.len(), 2);
        assert_eq!(core.highlight.current_match, 0);
        core.apply(&mut shaper, crate::RendererCommand::NavigateSearch(1));
        assert_eq!(core.highlight.current_match, 1);

        let (start, end) = core.highlight.matches[0];
        core.selection.selection_anchor = Some(start);
        core.selection.selection_active = Some(end);
        core.update_selection_text();
        assert_eq!(core.selection.selected_text.as_deref(), Some("Needle"));
        assert_eq!(core.selection.selected_text_markdown.as_deref(), Some("**Needle**"));
        assert_eq!(core.copy_selection_to_clipboard(), Ok(true));
        assert_eq!(host.clipboard_text.borrow().as_deref(), Some("**Needle**"));

        core.configure_layout_for_viewport(&mut shaper, Size::new(1_600.0, 800.0));
        assert_eq!(core.layout.col_count, 2.0);
        assert!(core.layout.col_gap >= 0.0);

        let events = host.events.borrow();
        assert!(events.iter().any(|event| matches!(event, RendererEvent::ColumnWidthChanged(width) if *width == 480.0)));
        assert!(events.iter().any(|event| matches!(event, RendererEvent::FontSizeChanged(size) if *size == 18.0)));
        assert!(events.iter().any(|event| matches!(event, RendererEvent::ScaleChanged(scale) if *scale == 1.5)));
        assert!(events.iter().any(|event| matches!(event, RendererEvent::LineNumbersChanged(false))));
        assert!(events.iter().any(|event| matches!(event, RendererEvent::SearchActiveChanged(true))));
        assert!(events.iter().any(|event| matches!(event, RendererEvent::MatchInfo { current: 2, total: 2 })));
        assert!(host.repaint_requests.get() > 0);
        drop(events);

        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn navigation_reuses_cached_documents_between_adjacent_spine_entries() {
        let (provider, read_count) = CountingProvider::new(vec![("doc0.html", "<html><body><a href=\"doc1.html\">next</a></body></html>"), ("doc1.html", "<html><body><a href=\"doc0.html\">prev</a></body></html>")]);

        let host = Rc::new(TestHost::default());
        let uris = vec!["doc0.html".to_owned(), "doc1.html".to_owned()];
        let mut shaper = TestShaper::default();
        let mut core = RendererCore::from_provider_with_nav(host, &mut shaper, provider, uris, 0, Some("0:0"));

        assert_eq!(read_count.load(Ordering::Relaxed), 1);

        core.next_document(&mut shaper);
        assert_eq!(core.nav.current_doc_index, 1);
        assert_eq!(read_count.load(Ordering::Relaxed), 2);

        core.prev_document(&mut shaper);
        assert_eq!(core.nav.current_doc_index, 0);
        assert_eq!(read_count.load(Ordering::Relaxed), 2);

        core.next_document(&mut shaper);
        assert_eq!(core.nav.current_doc_index, 1);
        assert_eq!(read_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn navigation_cache_uses_eviction_when_capacity_is_exceeded() {
        let (provider, read_count) = CountingProvider::new(vec![
            ("doc0.html", "<html><body><p>doc0</p></body></html>"),
            ("doc1.html", "<html><body><p>doc1</p></body></html>"),
            ("doc2.html", "<html><body><p>doc2</p></body></html>"),
            ("doc3.html", "<html><body><p>doc3</p></body></html>"),
            ("doc4.html", "<html><body><p>doc4</p></body></html>"),
        ]);

        let host = Rc::new(TestHost::default());
        let uris = vec!["doc0.html".to_owned(), "doc1.html".to_owned(), "doc2.html".to_owned(), "doc3.html".to_owned(), "doc4.html".to_owned()];
        let mut shaper = TestShaper::default();
        let mut core = RendererCore::from_provider_with_nav(host, &mut shaper, provider, uris, 0, Some("0:0"));

        assert_eq!(read_count.load(Ordering::Relaxed), 1);

        for _ in 0..4 {
            core.next_document(&mut shaper);
        }
        assert_eq!(core.nav.current_doc_index, 4);
        assert_eq!(read_count.load(Ordering::Relaxed), 5);

        while core.nav.current_doc_index > 0 {
            core.prev_document(&mut shaper);
        }
        assert_eq!(core.nav.current_doc_index, 0);

        if !core.nav.location_history.is_empty() {
            assert_eq!(core.nav.location_history_index, core.nav.location_history.len() - 1);
        }
        assert_eq!(read_count.load(Ordering::Relaxed), 7);
    }
}
