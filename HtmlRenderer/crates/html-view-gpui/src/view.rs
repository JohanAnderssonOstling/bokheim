use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bokheim_app_contract::{ReaderNavigationAction, ReaderNavigationKey, resolve_reader_navigation_shortcut};
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, ImageFormat, InspectorElementId, IntoElement, KeyContext,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point as GpuiPoint, Render, RenderImage, ScrollDelta, ScrollWheelEvent, Style, Window, relative,
};
use html_view_core::{DisplayCacheKey, InteractionPalette, PointerDownOptions, RendererCommand, RendererEvent, RendererHost, RendererSession, TocEntry};
use image::RgbaImage;
use kurbo::{Point, Size};

use crate::paint::{GpuiCommandPainter, PaintCommand, paint_commands};
use crate::text::{GlyphStore, GpuiTextCache, TextShapeCacheStats};

struct HtmlDocumentElement {
    view: Entity<HtmlView>,
    focus: FocusHandle,
    link_hovered: bool,
}

struct HtmlDocumentPrepaint {
    hitbox: Hitbox,
    paint: PreparedPaint,
}

enum HostMessage {
    Repaint,
    Event(RendererEvent),
    ClipboardText(String),
    ClipboardImage { width: usize, height: usize, rgba: Vec<u8> },
    ClipboardSvg(Vec<u8>),
}

struct GpuiHost {
    repaint_pending: Cell<bool>,
    messages: async_channel::Sender<HostMessage>,
    executor: gpui::BackgroundExecutor,
    scheduled_repaint: Arc<AtomicBool>,
}

impl GpuiHost {
    fn new(messages: async_channel::Sender<HostMessage>, executor: gpui::BackgroundExecutor) -> Self {
        Self { repaint_pending: Cell::new(false), messages, executor, scheduled_repaint: Arc::new(AtomicBool::new(false)) }
    }

    fn send(&self, message: HostMessage) {
        let _ = self.messages.try_send(message);
    }
}

impl RendererHost for GpuiHost {
    fn request_repaint(&self) {
        if !self.repaint_pending.replace(true) {
            self.send(HostMessage::Repaint);
        }
    }

    fn request_style(&self) {
        if !self.repaint_pending.replace(true) {
            self.send(HostMessage::Repaint);
        }
    }

    fn schedule(&self, delay: Duration, callback: Box<dyn FnOnce() + Send>) {
        let timer = self.executor.timer(delay);
        self.executor
            .spawn(async move {
                timer.await;
                callback();
            })
            .detach();
    }

    fn schedule_repaint(&self, delay: Duration) {
        if self.scheduled_repaint.swap(true, Ordering::Relaxed) {
            return;
        }
        let timer = self.executor.timer(delay);
        let messages = self.messages.clone();
        let scheduled_repaint = self.scheduled_repaint.clone();
        self.executor
            .spawn(async move {
                timer.await;
                scheduled_repaint.store(false, Ordering::Relaxed);
                let _ = messages.send(HostMessage::Repaint).await;
            })
            .detach();
    }

    fn set_clipboard(&self, text: &str) -> Result<(), String> {
        self.send(HostMessage::ClipboardText(text.to_owned()));
        Ok(())
    }

    fn set_clipboard_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        self.send(HostMessage::ClipboardImage { width, height, rgba });
        Ok(())
    }

    fn set_clipboard_svg(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.send(HostMessage::ClipboardSvg(bytes));
        Ok(())
    }

    fn emit(&self, event: RendererEvent) {
        self.send(HostMessage::Event(event));
    }
}

pub struct HtmlView {
    host: Rc<GpuiHost>,
    renderer: RendererSession<GpuiTextCache>,
    focus: FocusHandle,
    images: HashMap<Vec<u8>, Arc<RenderImage>>,
    document_origin: Cell<GpuiPoint<Pixels>>,
    prepared_paints: VecDeque<CachedPaint>,
    command_buffers: Vec<Vec<PaintCommand>>,
    paint_cache_stats: PaintCacheStats,
    pointer_active: bool,
}

#[derive(Clone)]
struct PreparedPaint {
    base_before_overlay: Arc<Vec<PaintCommand>>,
    overlay: Arc<Vec<PaintCommand>>,
    base_after_overlay: Arc<Vec<PaintCommand>>,
    glyph_store: GlyphStore,
}

struct CachedPaint {
    base_key: DisplayCacheKey,
    overlay_key: DisplayCacheKey,
    paint: PreparedPaint,
}

const PAINT_CACHE_CAPACITY: usize = 3;
const COMMAND_BUFFER_POOL_CAPACITY: usize = PAINT_CACHE_CAPACITY * 3;

fn take_command_buffer(buffers: &mut Vec<Vec<PaintCommand>>, stats: &mut PaintCacheStats) -> Vec<PaintCommand> {
    if let Some(buffer) = buffers.pop() {
        stats.reused_command_buffers += 1;
        buffer
    } else {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintCacheStats {
    pub base_hits: u64,
    pub base_misses: u64,
    pub overlay_hits: u64,
    pub overlay_misses: u64,
    pub evictions: u64,
    pub reused_command_buffers: u64,
    pub retained_entries: usize,
}

impl IntoElement for HtmlDocumentElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HtmlDocumentElement {
    type RequestLayoutState = ();
    type PrepaintState = HtmlDocumentPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::View(self.view.entity_id()))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        window.set_focus_handle(&self.focus, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let paint = self.view.update(cx, |view, _| view.prepare_paint(bounds));
        HtmlDocumentPrepaint { hitbox, paint }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, _cx: &mut App) {
        window.paint_quad(gpui::fill(bounds, gpui::white()));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            paint_commands(prepaint.paint.base_before_overlay.as_ref(), &prepaint.paint.glyph_store, bounds, window);
            paint_commands(prepaint.paint.overlay.as_ref(), &prepaint.paint.glyph_store, bounds, window);
            paint_commands(prepaint.paint.base_after_overlay.as_ref(), &prepaint.paint.glyph_store, bounds, window);
        });

        if self.link_hovered {
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        let mut key_context = KeyContext::default();
        key_context.add("ReaderDocument");
        window.set_key_context(key_context);

        let hitbox = prepaint.hitbox.clone();
        let mouse_down_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            mouse_down_target.update(cx, |view, cx| {
                view.on_mouse_down(event, window, cx);
            });
        });

        let hitbox = prepaint.hitbox.clone();
        let mouse_move_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let pointer_active = mouse_move_target.read(cx).pointer_active;
            if !pointer_active && !hitbox.is_hovered(window) {
                return;
            }
            mouse_move_target.update(cx, |view, cx| {
                view.on_mouse_move(event, window, cx);
            });
        });

        let mouse_up_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !mouse_up_target.read(cx).pointer_active {
                return;
            }
            mouse_up_target.update(cx, |view, cx| {
                view.on_mouse_up(event, window, cx);
            });
        });

        let hitbox = prepaint.hitbox.clone();
        let scroll_target = self.view.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            scroll_target.update(cx, |view, cx| {
                view.on_scroll(event, window, cx);
            });
        });

        let focus = self.focus.clone();
        let key_target = self.view.clone();
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !focus.is_focused(window) {
                return;
            }
            key_target.update(cx, |view, cx| {
                view.on_key_down(event, window, cx);
            });
        });
    }
}

impl EventEmitter<RendererEvent> for HtmlView {}

impl HtmlView {
    pub fn from_provider(
        provider: std::sync::Arc<dyn html_view_core::ResourceProvider>, document_uris: Vec<String>, start_index: usize, nav_state: Option<&str>, config: html_view_core::RendererInitialConfig, window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_renderer(window, cx, move |host, shaper| RendererSession::from_provider_with_nav(host, shaper, provider, document_uris, start_index, nav_state, config))
    }

    fn new_with_renderer(window: &mut Window, cx: &mut Context<Self>, make_renderer: impl FnOnce(Rc<GpuiHost>, GpuiTextCache) -> RendererSession<GpuiTextCache>) -> Self {
        let startup_started = Instant::now();
        let (messages, receiver) = async_channel::unbounded();
        let host = Rc::new(GpuiHost::new(messages, cx.background_executor().clone()));
        println!("HTML_VIEW_STARTUP phase=host_ready elapsed_ms={}", startup_started.elapsed().as_millis());
        let shaper_started = Instant::now();
        let shaper = GpuiTextCache::new(window.text_system().clone());
        println!("HTML_VIEW_STARTUP phase=shaper_ready elapsed_ms={} phase_ms={}", startup_started.elapsed().as_millis(), shaper_started.elapsed().as_millis());
        let renderer_started = Instant::now();
        let mut renderer = make_renderer(host.clone(), shaper);
        println!("HTML_VIEW_STARTUP phase=renderer_ready elapsed_ms={} phase_ms={}", startup_started.elapsed().as_millis(), renderer_started.elapsed().as_millis());
        let snapshot_started = Instant::now();
        renderer.emit_state_snapshot();
        println!("HTML_VIEW_STARTUP phase=snapshot_emitted elapsed_ms={} phase_ms={}", startup_started.elapsed().as_millis(), snapshot_started.elapsed().as_millis());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        cx.spawn(async move |this, cx| {
            while let Ok(message) = receiver.recv().await {
                if this.update(cx, |this, cx| this.handle_host_message(message, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let view = Self {
            host,
            renderer,
            focus,
            images: HashMap::new(),
            document_origin: Cell::new(GpuiPoint::default()),
            prepared_paints: VecDeque::with_capacity(PAINT_CACHE_CAPACITY),
            command_buffers: Vec::with_capacity(COMMAND_BUFFER_POOL_CAPACITY),
            paint_cache_stats: PaintCacheStats::default(),
            pointer_active: false,
        };
        println!("HTML_VIEW_STARTUP phase=view_ready elapsed_ms={}", startup_started.elapsed().as_millis());
        view
    }

    pub fn apply(&mut self, command: RendererCommand, _cx: &mut Context<Self>) {
        self.renderer.apply(command);
    }

    /// Returns whether the renderer is still loading or decoding resources.
    pub fn has_pending_resources(&self) -> bool {
        self.renderer.has_pending_resources()
    }

    pub fn set_interaction_palette(&mut self, palette: InteractionPalette, _cx: &mut Context<Self>) {
        self.renderer.set_interaction_palette(palette);
    }

    pub fn next_page(&mut self, _cx: &mut Context<Self>) {
        self.renderer.next_page();
    }

    pub fn previous_page(&mut self, _cx: &mut Context<Self>) {
        self.renderer.previous_page();
    }

    pub fn toc(&self) -> Option<Vec<TocEntry>> {
        self.renderer.toc().ok().flatten()
    }

    pub fn document_toc(&self) -> Vec<TocEntry> {
        self.renderer.document_toc()
    }

    pub fn resolve_href(&self, href: &str) -> Option<(usize, Option<String>)> {
        self.renderer.resolve_href(href)
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn selection_contains(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.selection_contains_point(self.layout_point(position))
    }

    pub fn table_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.table_at(self.layout_point(position))
    }

    pub fn image_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.image_at(self.layout_point(position))
    }

    pub fn copy_image_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.copy_image_at(self.layout_point(position))
    }

    pub fn table_selection_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.table_selection_at(self.layout_point(position))
    }

    pub fn begin_table_selection_at(&mut self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.begin_table_selection_at(self.layout_point(position))
    }

    pub fn copy_table_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.copy_table_at(self.layout_point(position))
    }

    pub fn copy_table_unstyled_html_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.copy_table_unstyled_html_at(self.layout_point(position))
    }

    pub fn copy_table_styled_html_at(&self, position: GpuiPoint<Pixels>) -> bool {
        self.renderer.copy_table_styled_html_at(self.layout_point(position))
    }

    pub fn copy_table_selection(&self) -> bool {
        self.renderer.copy_table_selection()
    }

    pub fn paint_cache_stats(&self) -> PaintCacheStats {
        PaintCacheStats { retained_entries: self.prepared_paints.len(), ..self.paint_cache_stats }
    }

    pub fn text_shape_cache_stats(&self) -> TextShapeCacheStats {
        self.renderer.glyph_shaper().shape_cache_stats()
    }

    fn recycle_command_arc(&mut self, commands: Arc<Vec<PaintCommand>>) {
        if self.command_buffers.len() >= COMMAND_BUFFER_POOL_CAPACITY {
            return;
        }
        if let Ok(mut commands) = Arc::try_unwrap(commands) {
            commands.clear();
            self.command_buffers.push(commands);
        }
    }

    fn recycle_cached_paint(&mut self, cached: CachedPaint) {
        self.recycle_command_arc(cached.paint.base_before_overlay);
        self.recycle_command_arc(cached.paint.overlay);
        self.recycle_command_arc(cached.paint.base_after_overlay);
    }

    fn prepare_paint(&mut self, bounds: Bounds<Pixels>) -> PreparedPaint {
        let total_started = Instant::now();
        self.document_origin.set(bounds.origin);
        let viewport = Size::new(f32::from(bounds.size.width) as f64, f32::from(bounds.size.height) as f64);
        let renderer_started = Instant::now();
        let frame = self.renderer.prepare_frame(viewport);
        let renderer_ms = renderer_started.elapsed().as_millis();
        let cache_started = Instant::now();
        let keys = frame.cache_keys();
        let glyph_store = frame.glyph_shaper().store();
        let run_store = frame.glyph_shaper().run_store();
        let base = self.prepared_paints.iter().find(|cached| cached.base_key == keys.base).map(|cached| (cached.paint.base_before_overlay.clone(), cached.paint.base_after_overlay.clone()));
        let overlay = self.prepared_paints.iter().find(|cached| cached.overlay_key == keys.overlay).map(|cached| cached.paint.overlay.clone());

        self.paint_cache_stats.base_hits += u64::from(base.is_some());
        self.paint_cache_stats.base_misses += u64::from(base.is_none());
        self.paint_cache_stats.overlay_hits += u64::from(overlay.is_some());
        self.paint_cache_stats.overlay_misses += u64::from(overlay.is_none());

        let base_before_buffer = if base.is_none() { take_command_buffer(&mut self.command_buffers, &mut self.paint_cache_stats) } else { Default::default() };
        let overlay_buffer = if overlay.is_none() { take_command_buffer(&mut self.command_buffers, &mut self.paint_cache_stats) } else { Default::default() };
        let base_after_buffer = if base.is_none() { take_command_buffer(&mut self.command_buffers, &mut self.paint_cache_stats) } else { Default::default() };
        let mut base_before_painter = GpuiCommandPainter::new(&mut self.images, run_store, base_before_buffer);
        let mut overlay_painter = GpuiCommandPainter::commands_only(overlay_buffer);
        let mut base_after_painter = GpuiCommandPainter::commands_only(base_after_buffer);
        let layer_started = Instant::now();
        if base.is_none() || overlay.is_none() {
            frame.paint_layers(&mut base_before_painter, &mut overlay_painter, &mut base_after_painter, base.is_none(), overlay.is_none());
        }
        let layer_ms = layer_started.elapsed().as_millis();
        let (base_before_overlay, base_after_overlay) = base.unwrap_or_else(|| (Arc::new(base_before_painter.into_commands()), Arc::new(base_after_painter.into_commands())));
        let overlay = overlay.unwrap_or_else(|| Arc::new(overlay_painter.into_commands()));
        let command_count = base_before_overlay.len() + overlay.len() + base_after_overlay.len();
        let paint = PreparedPaint { base_before_overlay, overlay, base_after_overlay, glyph_store };
        if let Some(index) = self.prepared_paints.iter().position(|cached| cached.base_key == keys.base && cached.overlay_key == keys.overlay)
            && let Some(replaced) = self.prepared_paints.remove(index)
        {
            self.recycle_cached_paint(replaced);
        }
        self.prepared_paints.push_back(CachedPaint { base_key: keys.base, overlay_key: keys.overlay, paint: paint.clone() });
        while self.prepared_paints.len() > PAINT_CACHE_CAPACITY {
            if let Some(evicted) = self.prepared_paints.pop_front() {
                self.recycle_cached_paint(evicted);
            }
            self.paint_cache_stats.evictions += 1;
        }
        let total_ms = total_started.elapsed().as_millis();
        if total_ms >= 10 {
            println!(
                "HTML_GPUI_PREPAINT total_ms={total_ms} renderer_prepare_ms={renderer_ms} command_layers_ms={layer_ms} cache_and_bookkeeping_ms={} commands={command_count}",
                cache_started.elapsed().as_millis().saturating_sub(layer_ms)
            );
        }
        paint
    }

    fn handle_host_message(&mut self, message: HostMessage, cx: &mut Context<Self>) {
        match message {
            HostMessage::Repaint => {
                self.host.repaint_pending.set(false);
                cx.notify();
            }
            HostMessage::Event(event) => cx.emit(event),
            HostMessage::ClipboardText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            HostMessage::ClipboardImage { width, height, rgba } => {
                let Some(buffer) = RgbaImage::from_raw(width as u32, height as u32, rgba) else {
                    return;
                };
                let mut encoded = std::io::Cursor::new(Vec::new());
                if image::DynamicImage::ImageRgba8(buffer).write_to(&mut encoded, image::ImageFormat::Png).is_ok() {
                    let image = gpui::Image::from_bytes(ImageFormat::Png, encoded.into_inner());
                    cx.write_to_clipboard(ClipboardItem::new_image(&image));
                }
            }
            HostMessage::ClipboardSvg(bytes) => {
                let image = gpui::Image::from_bytes(ImageFormat::Svg, bytes);
                cx.write_to_clipboard(ClipboardItem::new_image(&image));
            }
        }
    }

    fn layout_point(&self, position: GpuiPoint<Pixels>) -> Point {
        let origin = self.document_origin.get();
        self.renderer.to_layout_point(Point::new(f32::from(position.x - origin.x) as f64, f32::from(position.y - origin.y) as f64))
    }

    fn stop_event(cx: &mut Context<Self>) {
        cx.stop_propagation();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        self.focus.focus(window, cx);
        let position = self.layout_point(event.position);
        let options = PointerDownOptions { semantic_selection: event.modifiers.control, copy_image: false };
        self.pointer_active = self.renderer.pointer_down(position, options);
        if self.pointer_active {
            Self::stop_event(cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let position = self.layout_point(event.position);
        let outcome = self.renderer.pointer_move(position, event.modifiers.control);
        if outcome.link_hover_changed {
            cx.notify();
        }
        if event.dragging() && outcome.handled {
            Self::stop_event(cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.pointer_active {
            return;
        }
        self.pointer_active = false;
        self.renderer.pointer_up();
        Self::stop_event(cx);
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let y = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y),
            ScrollDelta::Lines(delta) => delta.y,
        };
        if !self.renderer.scroll_vertical(y as f64) {
            return;
        }
        Self::stop_event(cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let navigation = ReaderNavigationKey::from_key_name(key).and_then(|key| resolve_reader_navigation_shortcut(key, modifiers.control || modifiers.platform, modifiers.alt, modifiers.shift));
        let handled = if let Some(action) = navigation {
            match action {
                ReaderNavigationAction::PreviousLine => {
                    self.renderer.previous_line();
                    true
                }
                ReaderNavigationAction::NextLine => {
                    self.renderer.next_line();
                    true
                }
                ReaderNavigationAction::PreviousPage => {
                    self.renderer.previous_page();
                    true
                }
                ReaderNavigationAction::NextPage => {
                    self.renderer.next_page();
                    true
                }
                ReaderNavigationAction::PreviousSection => {
                    self.renderer.previous_document();
                    true
                }
                ReaderNavigationAction::NextSection => {
                    self.renderer.next_document();
                    true
                }
                ReaderNavigationAction::HistoryBack => self.renderer.navigate_history_back(),
                ReaderNavigationAction::HistoryForward => self.renderer.navigate_history_forward(),
            }
        } else if modifiers.alt && !modifiers.control {
            match key {
                "=" | "+" => {
                    self.renderer.change_root_font_size(1.0);
                    true
                }
                "-" => {
                    self.renderer.change_root_font_size(-1.0);
                    true
                }
                _ => false,
            }
        } else if modifiers.control || modifiers.platform {
            match key {
                "c" => self.renderer.copy_selection(),
                _ => false,
            }
        } else if key == "escape" {
            self.renderer.clear_table_selection()
        } else {
            false
        };
        if handled {
            cx.notify();
            Self::stop_event(cx);
        }
    }
}

impl Render for HtmlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        HtmlDocumentElement { view: cx.entity(), focus: self.focus.clone(), link_hovered: self.renderer.link_cursor_active() }
    }
}
