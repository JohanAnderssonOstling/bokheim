use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, ImageFormat, InspectorElementId, IntoElement, KeyContext,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point as GpuiPoint, Render, RenderImage, ScrollDelta, ScrollWheelEvent, Style, Window, relative,
};
use html_view_core::{PointerDownOptions, RendererCommand, RendererEvent, RendererHost, RendererSession, TocEntry};
use image::RgbaImage;
use kurbo::{Point, Size};
use reader_style::ReaderInteractionStyle;

use crate::paint::{GpuiCommandPainter, PaintCommand, paint_commands};
use crate::text::{GlyphStore, GpuiGlyphCache};

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
}

struct GpuiHost {
    repaint_pending: Cell<bool>,
    messages: async_channel::Sender<HostMessage>,
    executor: gpui::BackgroundExecutor,
    paint_revision: Arc<AtomicU64>,
    scheduled_repaint: Arc<AtomicBool>,
}

impl GpuiHost {
    fn new(messages: async_channel::Sender<HostMessage>, executor: gpui::BackgroundExecutor) -> Self {
        Self { repaint_pending: Cell::new(false), messages, executor, paint_revision: Arc::new(AtomicU64::new(1)), scheduled_repaint: Arc::new(AtomicBool::new(false)) }
    }

    fn send(&self, message: HostMessage) {
        let _ = self.messages.try_send(message);
    }
}

impl RendererHost for GpuiHost {
    fn request_repaint(&self) {
        self.paint_revision.fetch_add(1, Ordering::Relaxed);
        if !self.repaint_pending.replace(true) {
            self.send(HostMessage::Repaint);
        }
    }

    fn request_style(&self) {
        self.paint_revision.fetch_add(1, Ordering::Relaxed);
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
        let paint_revision = self.paint_revision.clone();
        let scheduled_repaint = self.scheduled_repaint.clone();
        self.executor
            .spawn(async move {
                timer.await;
                scheduled_repaint.store(false, Ordering::Relaxed);
                paint_revision.fetch_add(1, Ordering::Relaxed);
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

    fn emit(&self, event: RendererEvent) {
        self.send(HostMessage::Event(event));
    }
}

pub struct HtmlView {
    host: Rc<GpuiHost>,
    renderer: RendererSession<GpuiGlyphCache>,
    focus: FocusHandle,
    images: HashMap<Vec<u8>, Arc<RenderImage>>,
    document_origin: Cell<GpuiPoint<Pixels>>,
    prepared_paint: Option<CachedPaint>,
    pointer_active: bool,
}

#[derive(Clone)]
struct PreparedPaint {
    commands: Arc<[PaintCommand]>,
    glyph_store: GlyphStore,
}

struct CachedPaint {
    viewport: Size,
    revision: u64,
    paint: PreparedPaint,
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
        Some("gpui-html-renderer".into())
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
            paint_commands(prepaint.paint.commands.as_ref(), &prepaint.paint.glyph_store, bounds, window);
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
    pub fn from_epub_bytes(bytes: Vec<u8>, nav_state: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_renderer(window, cx, move |host, shaper| RendererSession::from_epub_bytes_with_nav(host, shaper, bytes, nav_state))
    }

    pub fn from_source_path(path: PathBuf, nav_state: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_renderer(window, cx, move |host, shaper| RendererSession::from_source_path_with_nav(host, shaper, path, nav_state))
    }

    fn new_with_renderer(window: &mut Window, cx: &mut Context<Self>, make_renderer: impl FnOnce(Rc<GpuiHost>, GpuiGlyphCache) -> RendererSession<GpuiGlyphCache>) -> Self {
        let (messages, receiver) = async_channel::unbounded();
        let host = Rc::new(GpuiHost::new(messages, cx.background_executor().clone()));
        let shaper = GpuiGlyphCache::new(window.text_system().clone());
        let mut renderer = make_renderer(host.clone(), shaper);
        renderer.emit_state_snapshot();
        let focus = cx.focus_handle();
        cx.spawn(async move |this, cx| {
            while let Ok(message) = receiver.recv().await {
                if this.update(cx, |this, cx| this.handle_host_message(message, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { host, renderer, focus, images: HashMap::new(), document_origin: Cell::new(GpuiPoint::default()), prepared_paint: None, pointer_active: false }
    }

    pub fn apply(&mut self, command: RendererCommand, _cx: &mut Context<Self>) {
        self.renderer.apply(command);
    }

    pub fn set_interaction_style(&mut self, style: ReaderInteractionStyle, _cx: &mut Context<Self>) {
        self.renderer.set_interaction_style(style);
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

    fn prepare_paint(&mut self, bounds: Bounds<Pixels>) -> PreparedPaint {
        self.document_origin.set(bounds.origin);
        let viewport = Size::new(f32::from(bounds.size.width) as f64, f32::from(bounds.size.height) as f64);
        let revision = self.host.paint_revision.load(Ordering::Relaxed);
        if let Some(cached) = &self.prepared_paint {
            if cached.viewport == viewport && cached.revision == revision {
                return cached.paint.clone();
            }
        }
        let frame = self.renderer.prepare_frame(viewport);
        let glyph_store = frame.glyph_shaper().store();
        let mut painter = GpuiCommandPainter::new(&mut self.images);
        frame.paint(&mut painter);
        let paint = PreparedPaint { commands: painter.commands.into(), glyph_store };
        self.prepared_paint = Some(CachedPaint { viewport, revision: self.host.paint_revision.load(Ordering::Relaxed), paint: paint.clone() });
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
        self.focus.focus(window, cx);
        let position = self.layout_point(event.position);
        let options = PointerDownOptions { select_whole_lines: event.modifiers.control, copy_image: event.modifiers.control };
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
        let handled = if modifiers.alt && !modifiers.control {
            match key {
                "left" => self.renderer.navigate_history_back(),
                "right" => self.renderer.navigate_history_forward(),
                "up" => {
                    self.renderer.change_root_font_size(1.0);
                    true
                }
                "down" => {
                    self.renderer.change_root_font_size(-1.0);
                    true
                }
                _ => false,
            }
        } else if modifiers.control || modifiers.platform {
            match key {
                "left" => {
                    self.renderer.previous_document();
                    true
                }
                "right" => {
                    self.renderer.next_document();
                    true
                }
                "c" => self.renderer.copy_selection(),
                _ => false,
            }
        } else {
            match key {
                "up" => {
                    self.renderer.previous_line();
                    true
                }
                "down" => {
                    self.renderer.next_line();
                    true
                }
                "left" | "pageup" => {
                    self.renderer.previous_page();
                    true
                }
                "right" | "pagedown" | "space" => {
                    self.renderer.next_page();
                    true
                }
                "l" => {
                    self.renderer.toggle_line_numbers();
                    true
                }
                _ => false,
            }
        };
        if handled {
            Self::stop_event(cx);
        }
    }
}

impl Render for HtmlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        HtmlDocumentElement { view: cx.entity(), focus: self.focus.clone(), link_hovered: self.renderer.link_cursor_active() }
    }
}
