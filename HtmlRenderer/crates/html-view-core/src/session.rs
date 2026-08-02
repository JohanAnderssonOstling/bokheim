use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use kurbo::{Point, Size};
use reader_style::ReaderInteractionStyle;

use crate::{GlyphShaper, LaidOutDocument, Painter, RendererCommand, RendererCore, RendererHost, RendererOperation, ResourceProvider, TocEntry};

/// The high-level renderer API used by UI backends.
///
/// A session owns both the renderer state and its glyph shaper so callers do
/// not have to coordinate them for navigation, relayout, or link activation.
pub struct RendererSession<S> {
    core: RendererCore,
    glyph_shaper: S,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerDownOptions {
    pub select_whole_lines: bool,
    pub copy_image: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerMoveOutcome {
    pub handled: bool,
    pub over_link: bool,
    pub link_hover_changed: bool,
}

impl<S: GlyphShaper> RendererSession<S> {
    pub(crate) fn from_core(core: RendererCore, glyph_shaper: S) -> Self {
        Self { core, glyph_shaper }
    }

    pub fn from_source_path(host: Rc<dyn RendererHost>, glyph_shaper: S, path: PathBuf) -> Self {
        Self::from_source_path_with_nav(host, glyph_shaper, path, None)
    }

    pub fn from_source_path_with_nav(host: Rc<dyn RendererHost>, mut glyph_shaper: S, path: PathBuf, nav_state: Option<&str>) -> Self {
        let core = RendererCore::from_source_path_with_nav(host, &mut glyph_shaper, path, nav_state);
        Self::from_core(core, glyph_shaper)
    }

    pub fn from_epub_bytes(host: Rc<dyn RendererHost>, glyph_shaper: S, bytes: Vec<u8>) -> Self {
        Self::from_epub_bytes_with_nav(host, glyph_shaper, bytes, None)
    }

    pub fn from_epub_bytes_with_nav(host: Rc<dyn RendererHost>, mut glyph_shaper: S, bytes: Vec<u8>, nav_state: Option<&str>) -> Self {
        let core = RendererCore::from_epub_bytes_with_nav(host, &mut glyph_shaper, bytes, nav_state);
        Self::from_core(core, glyph_shaper)
    }

    pub fn from_provider_with_nav(host: Rc<dyn RendererHost>, mut glyph_shaper: S, provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, start_index: usize, nav_state: Option<&str>) -> Self {
        let core = RendererCore::from_provider_with_nav(host, &mut glyph_shaper, provider, document_uris, start_index, nav_state);
        Self::from_core(core, glyph_shaper)
    }

    pub fn document(&self) -> &LaidOutDocument {
        self.core.document()
    }

    pub fn glyph_shaper(&self) -> &S {
        &self.glyph_shaper
    }

    pub fn emit_state_snapshot(&mut self) {
        self.core.emit_state_snapshot();
    }

    pub fn apply(&mut self, command: RendererCommand) {
        self.core.apply(&mut self.glyph_shaper, command);
    }

    pub fn toc(&self) -> std::io::Result<Option<Vec<TocEntry>>> {
        self.core.toc()
    }

    pub fn document_toc(&self) -> Vec<TocEntry> {
        self.core.document_toc()
    }

    pub fn set_toc_anchor_strings_by_doc(&mut self, anchors_by_doc: Vec<Vec<String>>) {
        self.core.set_toc_anchor_strings_by_doc(anchors_by_doc);
    }

    pub fn current_cfi(&self) -> Option<String> {
        self.core.current_cfi()
    }

    pub fn resolve_href(&self, href: &str) -> Option<(usize, Option<String>)> {
        self.core.resolve_href(href)
    }

    pub fn navigate_to_href(&mut self, href: &str) -> bool {
        self.core.navigate_to_href(&mut self.glyph_shaper, href)
    }

    pub fn navigate_history_back(&mut self) -> bool {
        self.core.navigate_history_back(&mut self.glyph_shaper)
    }

    pub fn navigate_history_forward(&mut self) -> bool {
        self.core.navigate_history_forward(&mut self.glyph_shaper)
    }

    pub fn set_position(&mut self, document: usize, glyph: Option<u32>) {
        self.core.set_position(&mut self.glyph_shaper, document, glyph);
    }

    pub fn set_cfi_position(&mut self, cfi: &str) -> bool {
        self.core.set_cfi_position(cfi)
    }

    pub fn restore_position_from_cfi(&mut self, cfi: &str) -> bool {
        self.set_cfi_position(cfi)
    }

    pub fn next_page(&mut self) {
        self.core.next_page(&mut self.glyph_shaper);
    }

    pub fn previous_page(&mut self) {
        self.core.prev_page(&mut self.glyph_shaper);
    }

    pub fn next_line(&mut self) {
        self.core.next_line();
    }

    pub fn previous_line(&mut self) {
        self.core.prev_line();
    }

    pub fn scroll_vertical(&mut self, delta_y: f64) -> bool {
        if delta_y < 0.0 {
            self.previous_line();
            true
        } else if delta_y > 0.0 {
            self.next_line();
            true
        } else {
            false
        }
    }

    pub fn next_document(&mut self) {
        self.core.next_document(&mut self.glyph_shaper);
    }

    pub fn previous_document(&mut self) {
        self.core.prev_document(&mut self.glyph_shaper);
    }

    pub fn root_font_size(&self) -> f32 {
        self.core.root_font_size()
    }

    pub fn set_root_font_size(&mut self, size: f32) {
        self.core.set_root_font_size(&mut self.glyph_shaper, size);
    }

    pub fn change_root_font_size(&mut self, delta: f32) {
        self.set_root_font_size(self.root_font_size() + delta);
    }

    pub fn column_width(&self) -> f64 {
        self.core.preferred_column_width()
    }

    pub fn set_column_width(&mut self, width: f64) {
        self.core.set_col_width(&mut self.glyph_shaper, width);
    }

    pub fn change_column_width(&mut self, delta: f64) {
        self.set_column_width(self.column_width() + delta);
    }

    pub fn scale(&self) -> f64 {
        self.core.scale()
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.core.set_scale(scale);
    }

    pub fn change_scale(&mut self, delta: f64) {
        self.set_scale(self.scale() + delta);
    }

    pub fn line_numbers_enabled(&self) -> bool {
        self.core.line_numbers_enabled()
    }

    pub fn set_line_numbers_enabled(&mut self, enabled: bool) {
        self.core.set_show_line_numbers(enabled);
    }

    pub fn toggle_line_numbers(&mut self) {
        self.set_line_numbers_enabled(!self.line_numbers_enabled());
    }

    pub fn search_active(&self) -> bool {
        self.core.search_active()
    }

    pub fn set_search_active(&mut self, active: bool) {
        self.core.set_search_active(active);
    }

    pub fn toggle_search(&mut self) {
        self.set_search_active(!self.search_active());
    }

    pub fn set_search_query(&mut self, query: &str) {
        self.core.set_search_query(query);
    }

    pub fn next_match(&mut self) {
        self.core.next_match();
    }

    pub fn previous_match(&mut self) {
        self.core.prev_match();
    }

    pub fn copy_selection(&self) -> bool {
        match self.core.copy_selection_to_clipboard() {
            Ok(handled) => handled,
            Err(message) => {
                self.core.emit_operation_failed(RendererOperation::CopySelection, message);
                true
            }
        }
    }

    /// Starts the complete primary-pointer interaction. Image copying takes
    /// precedence over text selection when `copy_image` is true.
    pub fn pointer_down(&mut self, position: Point, options: PointerDownOptions) -> bool {
        if options.copy_image
            && let Some(image) = self.core.hit_test_image(position)
        {
            if let Err(message) = self.core.copy_image_to_clipboard(image) {
                self.core.emit_operation_failed(RendererOperation::CopyImage, message);
            }
            return true;
        }
        self.core.begin_selection_at(position, options.select_whole_lines)
    }

    /// Updates both link hover state and an active selection.
    pub fn pointer_move(&mut self, position: Point, select_whole_lines: bool) -> PointerMoveOutcome {
        let was_over_link = self.core.link_cursor_active();
        let over_link = self.core.link_at(position);
        self.core.set_link_cursor_active(over_link);
        PointerMoveOutcome { handled: self.core.update_selection_at(position, select_whole_lines), over_link, link_hover_changed: was_over_link != over_link }
    }

    /// Finishes selection and activates a link if the interaction remained a click.
    pub fn pointer_up(&mut self) -> bool {
        let Some(glyph) = self.core.finish_selection() else {
            return false;
        };
        self.core.handle_link_click(&mut self.glyph_shaper, glyph)
    }

    pub fn pointer_leave(&mut self) {
        self.core.set_link_cursor_active(false);
    }

    pub fn link_cursor_active(&self) -> bool {
        self.core.link_cursor_active()
    }

    pub fn origin(&self) -> Point {
        self.core.origin()
    }

    pub fn set_origin(&mut self, origin: Point) {
        self.core.set_origin(origin);
    }

    pub fn set_interaction_style(&mut self, style: ReaderInteractionStyle) {
        self.core.set_interaction_style(style);
    }

    pub fn to_layout_point(&self, position: Point) -> Point {
        self.core.to_layout_point(position)
    }

    /// Prepares layout and returns a frame that can be painted exactly once.
    pub fn prepare_frame(&mut self, viewport_size: Size) -> PreparedFrame<'_, S> {
        self.core.prepare_frame(&mut self.glyph_shaper, viewport_size);
        PreparedFrame { core: &mut self.core, glyph_shaper: &self.glyph_shaper }
    }
}

pub struct PreparedFrame<'a, S> {
    core: &'a mut RendererCore,
    glyph_shaper: &'a S,
}

impl<'a, S> PreparedFrame<'a, S> {
    pub fn glyph_shaper(&self) -> &S {
        self.glyph_shaper
    }

    pub fn paint(self, painter: &mut impl Painter) {
        self.core.paint_frame(painter);
    }

    pub fn into_parts(self) -> (&'a S, FramePainter<'a>) {
        (self.glyph_shaper, FramePainter { core: self.core })
    }
}

/// The paint-only half of a prepared frame. This lets a backend construct a
/// painter that borrows the glyph shaper without exposing renderer internals.
pub struct FramePainter<'a> {
    core: &'a mut RendererCore,
}

impl FramePainter<'_> {
    pub fn paint(self, painter: &mut impl Painter) {
        self.core.paint_frame(painter);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::{FileSystemProvider, FontStyle, GlyphId, GlyphMetric, RecordingPainter, RendererEvent};

    use super::*;

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
        repaints: Cell<usize>,
        fail_clipboard: Cell<bool>,
    }

    impl RendererHost for TestHost {
        fn request_repaint(&self) {
            self.repaints.set(self.repaints.get() + 1);
        }

        fn request_style(&self) {}

        fn schedule(&self, _delay: Duration, callback: Box<dyn FnOnce() + Send>) {
            callback();
        }

        fn schedule_repaint(&self, _delay: Duration) {}

        fn set_clipboard(&self, _text: &str) -> Result<(), String> {
            if self.fail_clipboard.get() { Err("test clipboard failure".to_owned()) } else { Ok(()) }
        }

        fn set_clipboard_image(&self, _width: usize, _height: usize, _rgba: Vec<u8>) -> Result<(), String> {
            Ok(())
        }

        fn emit(&self, event: RendererEvent) {
            self.events.borrow_mut().push(event);
        }
    }

    #[test]
    fn session_owns_navigation_shaping_and_pointer_choreography() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let fixture = std::env::temp_dir().join(format!("html-view-session-{nonce}"));
        std::fs::create_dir_all(&fixture).unwrap();
        let first = fixture.join("one.html");
        let second = fixture.join("two.html");
        std::fs::write(&first, "<html><body><a href=\"two.html#target\">Next</a></body></html>").unwrap();
        std::fs::write(&second, "<html><body><h1 id=\"target\">Target</h1></body></html>").unwrap();

        let provider: Arc<dyn ResourceProvider> = Arc::new(FileSystemProvider::new());
        let uris = vec![first.to_string_lossy().into_owned(), second.to_string_lossy().into_owned()];
        let host = Rc::new(TestHost::default());
        let mut renderer = RendererSession::from_provider_with_nav(host.clone(), TestShaper::default(), provider, uris, 0, Some("0:0"));

        renderer.prepare_frame(Size::new(600.0, 800.0)).paint(&mut RecordingPainter::default());
        let view = renderer.core.document.render_view();
        let linked_glyph = (0..view.glyph_count() as u32).find(|glyph| view.link_href_for_glyph(*glyph).is_some()).expect("fixture should contain a linked glyph");
        let line_idx = renderer.core.find_line_for_glyph(linked_glyph).unwrap();
        let screen = renderer.core.frame.last_line_positions[line_idx].unwrap();
        let line = renderer.core.document.render_view().line(line_idx).expect("linked line should be present");
        let position = Point::new(screen.point.x + renderer.core.glyph_x_in_line(&line, linked_glyph) + 0.1, screen.point.y + line.height() / 2.0);

        let hover = renderer.pointer_move(position, false);
        assert!(hover.over_link);
        assert!(hover.link_hover_changed);
        assert!(renderer.pointer_down(position, PointerDownOptions::default()));
        renderer.core.selection.selected_text_markdown = Some("Next".to_owned());
        host.fail_clipboard.set(true);
        assert!(renderer.copy_selection());
        assert!(host.events.borrow().iter().any(|event| matches!(
            event,
            RendererEvent::OperationFailed { operation: RendererOperation::CopySelection, message }
                if message == "test clipboard failure"
        )));
        host.fail_clipboard.set(false);
        assert!(renderer.pointer_up());
        assert_eq!(renderer.core.nav.current_doc_index, 1);

        let font_size = renderer.root_font_size();
        let column_width = renderer.column_width();
        let scale = renderer.scale();
        renderer.change_root_font_size(2.0);
        renderer.change_column_width(-20.0);
        renderer.change_scale(0.1);
        renderer.toggle_line_numbers();
        assert_eq!(renderer.root_font_size(), font_size + 2.0);
        assert_eq!(renderer.column_width(), column_width - 20.0);
        assert_eq!(renderer.scale(), scale + 0.1);
        assert!(!renderer.line_numbers_enabled());
        assert!(!renderer.scroll_vertical(0.0));

        std::fs::remove_dir_all(fixture).unwrap();
    }
}
