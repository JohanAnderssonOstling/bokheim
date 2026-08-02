use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, ScrollStrategy, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState};
use html_view_core::{RendererCommand, RendererEvent, TocEntry};
use html_view_gpui::HtmlView;
use reader_style::ReaderInteractionStyle;

use crate::model::{LoadState, PendingReader, ReaderPreferences, ReaderSettings, ReaderView, ReaderWindowGeometry};

impl ReaderView {
    pub(crate) fn new(pending: PendingReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search in book"));
        let search_subscription = cx.subscribe_in(&search_input, window, |this, input, event: &InputEvent, _window, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                this.queue_search(query, cx);
            }
            InputEvent::PressEnter { shift, .. } => {
                if !this.apply_pending_search(cx) {
                    this.navigate_search(if *shift { -1 } else { 1 }, cx);
                }
            }
            _ => {}
        });
        let generation = 1;
        let bytes = pending.bytes;
        cx.spawn_in(window, {
            let nav_state = pending.nav_state.clone();
            async move |this, cx| {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.finish_load(generation, Ok(bytes), nav_state.as_deref(), window, cx);
                });
            }
        })
        .detach();

        let bounds_subscription = cx.observe_window_bounds(window, |_, window, cx| {
            let bounds = window.bounds();
            let geometry = ReaderWindowGeometry { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), width: f32::from(bounds.size.width), height: f32::from(bounds.size.height) };
            let (preferences, save) = {
                let settings = cx.global_mut::<ReaderSettings>();
                if settings.geometry == Some(geometry) {
                    return;
                }
                settings.geometry = Some(geometry);
                (settings.preferences.clone(), settings.save.clone())
            };
            if let Some(save) = save {
                save(preferences, Some(geometry), cx);
            }
        });
        let focus = cx.focus_handle();
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        focus.focus(window, cx);
        let (reading_position_tx, reading_position_rx) = async_channel::unbounded::<String>();
        let reading_library = pending.library.clone();
        let reading_book_id = pending.locator.book_id();
        let reading_worker = cx.global::<ReaderSettings>().persistence_worker.clone().expect("reader persistence worker is configured");
        cx.background_executor()
            .spawn(async move {
                while let Ok(mut cfi) = reading_position_rx.recv().await {
                    while let Ok(newer_cfi) = reading_position_rx.try_recv() {
                        cfi = newer_cfi;
                    }
                    let library = reading_library.clone();
                    let write = reading_worker.dispatch(move || library.update_reading_position(reading_book_id, &cfi).map_err(|error| error.to_string()));
                    let Ok(result) = write.recv().await else {
                        break;
                    };
                    if let Err(error) = result {
                        log::error!("failed to save reading position for book {reading_book_id}: {error}");
                    }
                }
            })
            .detach();

        Self {
            id: pending.id,
            locator: pending.locator,
            book_title: pending.title.clone(),
            title: pending.title,
            load_state: LoadState::Loading,
            load_generation: generation,
            toc_entries: Arc::new(Vec::new()),
            toc_tree,
            toc_resolutions: Default::default(),
            active_toc_link: None,
            toc_visible: true,
            search_input,
            search_query: String::new(),
            applied_search_query: String::new(),
            search_generation: 0,
            search_task: None,
            search_match: None,
            search_active: false,
            can_history_back: false,
            can_history_forward: false,
            unavailable: false,
            focus,
            renderer_subscription: None,
            reading_position_tx,
            _subscriptions: vec![bounds_subscription, search_subscription],
        }
    }

    fn finish_load(&mut self, generation: u64, result: std::io::Result<Vec<u8>>, nav_state: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.load_generation {
            return;
        }
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                log::error!("failed to load reader {} for book {}: {error}", self.id.0, self.locator.book_id());
                self.load_state = LoadState::Failed;
                cx.notify();
                return;
            }
        };

        let renderer = cx.new(|cx| HtmlView::from_epub_bytes(bytes, nav_state, window, cx));
        self.renderer_subscription = Some(cx.subscribe(&renderer, |this, _, event: &RendererEvent, cx| {
            this.handle_renderer_event(event, cx);
        }));
        let toc = renderer.read(cx).toc().unwrap_or_default();
        let resolutions = {
            let renderer = renderer.read(cx);
            Self::resolve_toc(&toc, |href| renderer.resolve_href(href))
        };
        self.set_toc(toc, resolutions, cx);

        let preferences = cx.global::<ReaderSettings>().preferences.clone();
        let search_query = self.search_query.clone();
        renderer.update(cx, |renderer, cx| {
            renderer.set_interaction_style(ReaderInteractionStyle::default(), cx);
            renderer.apply(RendererCommand::SetFontSize(preferences.font_size), cx);
            renderer.apply(RendererCommand::SetColumnWidth(preferences.column_width), cx);
            renderer.apply(RendererCommand::SetScale(preferences.scale), cx);
            renderer.apply(RendererCommand::SetLineNumbers(preferences.line_numbers), cx);
            renderer.apply(RendererCommand::SetSearchActive(self.search_active), cx);
            if !search_query.is_empty() {
                renderer.apply(RendererCommand::SetSearchQuery(search_query), cx);
            }
        });
        self.load_state = LoadState::Ready(renderer);
        cx.notify();
    }

    fn handle_renderer_event(&mut self, event: &RendererEvent, cx: &mut Context<Self>) {
        let should_notify = match event {
            RendererEvent::CfiChanged(Some(cfi)) => {
                if self.reading_position_tx.try_send(cfi.clone()).is_err() {
                    log::warn!("reading-position worker stopped for book {}", self.locator.book_id());
                }
                false
            }
            RendererEvent::TitleChanged(Some(title)) => {
                if self.title == *title {
                    false
                } else {
                    self.title = title.clone();
                    true
                }
            }
            RendererEvent::TocChanged(_) => false,
            RendererEvent::NavAnchorChanged { doc, anchor } => {
                let exact = self.toc_resolutions.iter().find(|(_, resolved)| resolved.0 == *doc && resolved.1 == *anchor).map(|(link, _)| link.clone());
                let next = exact.or_else(|| self.toc_resolutions.iter().find(|(_, resolved)| resolved.0 == *doc).map(|(link, _)| link.clone()));
                if self.active_toc_link == next {
                    false
                } else {
                    if let Some(link) = next.as_ref() {
                        let selected = TreeItem::new(link.clone(), link.clone());
                        self.toc_tree.update(cx, |tree, cx| {
                            tree.set_selected_item(Some(&selected), cx);
                            tree.reveal_item(&selected.id, ScrollStrategy::Center, cx);
                        });
                    }
                    self.active_toc_link = next;
                    true
                }
            }
            RendererEvent::HistoryAvailability { back, forward } => {
                if self.can_history_back == *back && self.can_history_forward == *forward {
                    false
                } else {
                    self.can_history_back = *back;
                    self.can_history_forward = *forward;
                    true
                }
            }
            RendererEvent::SearchActiveChanged(active) => {
                if self.search_active == *active {
                    false
                } else {
                    self.search_active = *active;
                    true
                }
            }
            RendererEvent::FontSizeChanged(font_size) => {
                self.update_preferences(cx, |preferences| {
                    preferences.font_size = *font_size;
                });
                true
            }
            RendererEvent::ColumnWidthChanged(column_width) => {
                self.update_preferences(cx, |preferences| {
                    preferences.column_width = *column_width;
                });
                false
            }
            RendererEvent::ScaleChanged(scale) => {
                self.update_preferences(cx, |preferences| {
                    preferences.scale = *scale;
                });
                false
            }
            RendererEvent::LineNumbersChanged(line_numbers) => {
                self.update_preferences(cx, |preferences| {
                    preferences.line_numbers = *line_numbers;
                });
                true
            }
            RendererEvent::MatchInfo { current, total } => {
                let next = Some((*current, *total));
                if self.search_match == next {
                    false
                } else {
                    self.search_match = next;
                    true
                }
            }
            RendererEvent::OperationFailed { operation, message } => {
                log::warn!("renderer failed to {operation}: {message}");
                false
            }
            _ => false,
        };
        if should_notify {
            cx.notify();
        }
    }

    pub(crate) fn update_preferences(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences)) {
        let Some((preferences, geometry, save)) = ({
            let settings = cx.global_mut::<ReaderSettings>();
            let previous = settings.preferences.clone();
            update(&mut settings.preferences);
            (previous != settings.preferences).then(|| (settings.preferences.clone(), settings.geometry, settings.save.clone()))
        }) else {
            return;
        };
        if let Some(save) = save {
            save(preferences, geometry, cx);
        }
    }

    fn set_toc(&mut self, entries: Vec<TocEntry>, resolutions: std::collections::HashMap<String, (usize, Option<String>)>, cx: &mut Context<Self>) {
        let tree_items = Self::toc_tree_items(&entries);
        self.toc_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
        });
        self.toc_entries = Arc::new(entries);
        self.toc_resolutions = resolutions;
    }

    fn toc_tree_items(entries: &[TocEntry]) -> Vec<TreeItem> {
        entries.iter().map(|entry| TreeItem::new(entry.link.clone(), entry.title.clone()).children(Self::toc_tree_items(&entry.children))).collect()
    }

    fn resolve_toc(entries: &[TocEntry], resolve: impl Fn(&str) -> Option<(usize, Option<String>)>) -> std::collections::HashMap<String, (usize, Option<String>)> {
        fn visit(entries: &[TocEntry], resolve: &impl Fn(&str) -> Option<(usize, Option<String>)>, resolutions: &mut std::collections::HashMap<String, (usize, Option<String>)>) {
            for entry in entries {
                if let Some(resolved) = resolve(&entry.link) {
                    resolutions.insert(entry.link.clone(), resolved);
                }
                visit(&entry.children, resolve, resolutions);
            }
        }

        let mut resolutions = std::collections::HashMap::new();
        visit(entries, &resolve, &mut resolutions);
        resolutions
    }

    pub(crate) fn with_renderer(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut HtmlView, &mut Context<HtmlView>)) {
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, update);
        }
    }

    pub(crate) fn previous_page(&self, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.previous_page(cx);
        });
    }

    pub(crate) fn next_page(&self, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.next_page(cx);
        });
    }

    pub(crate) fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        let current = cx.global::<ReaderSettings>().preferences.font_size;
        let font_size = (current + delta).clamp(8.0, 64.0);
        if font_size == current {
            return;
        }
        self.update_preferences(cx, |preferences| {
            preferences.font_size = font_size;
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetFontSize(font_size), cx);
        });
        cx.notify();
    }

    pub(crate) fn navigate_to(&self, href: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateToHref(href), cx);
        });
    }

    pub(crate) fn navigate_history(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateHistory(direction), cx);
        });
    }

    pub(crate) fn adjust_column_width(&self, delta: f64, cx: &mut Context<Self>) {
        let current = cx.global::<ReaderSettings>().preferences.column_width;
        let width = (current + delta).clamp(200.0, 1200.0);
        if width == current {
            return;
        }
        self.update_preferences(cx, |preferences| {
            preferences.column_width = width;
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetColumnWidth(width), cx);
        });
        cx.notify();
    }

    pub(crate) fn adjust_scale(&self, delta: f64, cx: &mut Context<Self>) {
        let current = cx.global::<ReaderSettings>().preferences.scale;
        let scale = (current + delta).clamp(0.5, 3.0);
        if scale == current {
            return;
        }
        self.update_preferences(cx, |preferences| {
            preferences.scale = scale;
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetScale(scale), cx);
        });
        cx.notify();
    }

    pub(crate) fn set_search_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.search_active = active;
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchActive(active), cx);
        });
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.search_query.clear();
        self.applied_search_query.clear();
        self.search_match = None;
        self.search_active = false;
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchQuery(String::new()), cx);
            renderer.apply(RendererCommand::SetSearchActive(false), cx);
        });
        cx.notify();
    }

    pub(crate) fn display_title(&self) -> String {
        let chapter = self.active_toc_link.as_ref().and_then(|active| {
            fn find(entries: &[TocEntry], active: &str) -> Option<String> {
                for entry in entries {
                    if entry.link == active {
                        return Some(entry.title.clone());
                    }
                    if let Some(found) = find(&entry.children, active) {
                        return Some(found);
                    }
                }
                None
            }
            find(&self.toc_entries, active)
        });
        let chapter = chapter.or_else(|| (self.title != self.book_title).then(|| self.title.clone()));
        match chapter {
            Some(chapter) => format!("{} • {chapter}", self.book_title),
            None => self.book_title.clone(),
        }
    }

    pub(crate) fn queue_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_query = query;
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;
        let timer = cx.background_executor().timer(Duration::from_millis(150));
        self.search_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.search_generation == generation {
                    this.apply_pending_search(cx);
                }
            });
        }));
    }

    fn apply_pending_search(&mut self, cx: &mut Context<Self>) -> bool {
        if self.applied_search_query == self.search_query {
            return false;
        }
        self.applied_search_query.clone_from(&self.search_query);
        let renderer_query = self.search_query.clone();
        self.with_renderer(cx, move |renderer, cx| {
            renderer.apply(RendererCommand::SetSearchQuery(renderer_query), cx);
        });
        true
    }

    pub(crate) fn navigate_search(&self, direction: i8, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::NavigateSearch(direction), cx);
        });
    }
}
