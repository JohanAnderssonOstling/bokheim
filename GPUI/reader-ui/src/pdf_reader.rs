use std::path::PathBuf;
use std::sync::Arc;

use bokheim_ui_components as components;
use epub_svelte_lib::BookLocator;
use epub_svelte_lib::library::Library;
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, KeyBinding, Render, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use pdf_view_gpui::{PdfView, PdfViewEvent};

use crate::model::{ReaderSettings, ReaderWindowGeometry, ReaderWindowId};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextPage, PreviousPage, ToggleSearch, ToggleToc};

pub(crate) struct PendingPdfReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: Arc<Library>,
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
    pub(crate) initial_page: usize,
}

pub(crate) struct PdfReaderView {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) unavailable: bool,
    title: String,
    pdf: Entity<PdfView>,
    search_input: Entity<InputState>,
    search_active: bool,
    search_match: (usize, usize),
    toc_visible: bool,
    current_page: usize,
    page_count: usize,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PdfReaderView {
    pub(crate) fn new(pending: PendingPdfReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pdf = cx.new(|cx| PdfView::from_bytes(pending.path, pending.bytes, pending.initial_page, window, cx));
        let position_library = pending.library.clone();
        let position_book_id = pending.locator.book_id();
        let persistence_worker = cx.global::<ReaderSettings>().persistence_worker.clone();
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search in PDF"));
        let pdf_subscription = cx.subscribe(&pdf, |this, _, event: &PdfViewEvent, cx| {
            match event {
                PdfViewEvent::PageChanged { current, total } => {
                    this.current_page = *current;
                    this.page_count = *total;
                }
                PdfViewEvent::SearchChanged { current, total } => {
                    this.search_match = (*current, *total);
                }
                PdfViewEvent::LoadFailed(error) => {
                    log::error!("failed to load PDF reader {}: {error}", this.id.0);
                }
            }
            cx.notify();
        });
        let position_subscription = cx.subscribe(&pdf, move |_, _, event: &PdfViewEvent, _| {
            let PdfViewEvent::PageChanged { current, .. } = event else {
                return;
            };
            let Some(worker) = persistence_worker.clone() else {
                return;
            };
            let library = position_library.clone();
            let page_index = current.saturating_sub(1);
            worker.dispatch(move || {
                if let Err(error) = library.update_pdf_reading_page(position_book_id, page_index as u32) {
                    log::warn!("failed to persist PDF page for book {position_book_id}: {error}");
                }
            });
        });
        let pdf_for_search = pdf.clone();
        let search_subscription = cx.subscribe_in(&search_input, window, move |this, input, event: &InputEvent, _window, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                if query.is_empty() || query.chars().count() >= 2 {
                    pdf_for_search.update(cx, |pdf, cx| {
                        pdf.search(query, cx);
                    });
                }
            }
            InputEvent::PressEnter { shift, .. } => {
                this.navigate_search(if *shift { -1 } else { 1 }, cx);
            }
            _ => {}
        });
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
        focus.focus(window, cx);
        Self {
            id: pending.id,
            locator: pending.locator,
            unavailable: false,
            title: pending.title,
            pdf,
            search_input,
            search_active: false,
            search_match: (0, 0),
            toc_visible: true,
            current_page: 0,
            page_count: 0,
            focus,
            _subscriptions: vec![pdf_subscription, position_subscription, search_subscription, bounds_subscription],
        }
    }

    fn previous_page(&self, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.previous_page(cx);
        });
    }

    fn next_page(&self, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.next_page(cx);
        });
    }

    fn navigate_search(&self, direction: i8, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.navigate_search(direction, cx);
        });
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.search_active = false;
        self.search_match = (0, 0);
        self.pdf.update(cx, |pdf, cx| {
            pdf.search(String::new(), cx);
        });
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn adjust_scale(&self, delta: f32, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.change_display_scale(delta, cx);
        });
    }

    fn render_outline_with_context(&self, entity: Entity<Self>, cx: &App) -> gpui::AnyElement {
        let outline = self.pdf.read(cx).info().map(|info| info.outline().to_vec()).unwrap_or_default();
        let current = self.current_page.saturating_sub(1);
        components::reader_toc()
            .children(outline.into_iter().enumerate().map(|(index, entry)| {
                let target = entity.clone();
                let page_index = entry.page_index();
                components::reader_toc_row(("pdf-outline", index), entry.depth(), page_index == current).child(entry.title().to_owned()).on_click(move |_, _, cx| {
                    target.update(cx, |this, cx| {
                        this.pdf.update(cx, |pdf, cx| {
                            pdf.set_page(page_index, cx);
                        });
                    });
                })
            }))
            .into_any_element()
    }

    fn render_settings(&self, entity: Entity<Self>, cx: &App) -> gpui::AnyElement {
        let scale = self.pdf.read(cx).display_scale();
        let decrease = entity.clone();
        let increase = entity;
        components::reader_settings_dropdown()
            .child(components::section_title("PDF settings"))
            .child(
                components::reader_setting_row("Zoom")
                    .child(components::reader_square_button("pdf-zoom-out", "−").tooltip("Zoom out (Ctrl+-)").on_click(move |_, _, cx| {
                        decrease.update(cx, |this, cx| {
                            this.adjust_scale(-0.1, cx);
                        });
                    }))
                    .child(components::reader_setting_value(format!("{:.0}%", scale * 100.0)))
                    .child(components::reader_square_button("pdf-zoom-in", "+").tooltip("Zoom in (Ctrl++)").on_click(move |_, _, cx| {
                        increase.update(cx, |this, cx| {
                            this.adjust_scale(0.1, cx);
                        });
                    })),
            )
            .child(
                components::reader_shortcuts()
                    .child(components::section_title("Keyboard shortcuts"))
                    .child(components::reader_shortcut("← / →: previous / next page"))
                    .child(components::reader_shortcut("Ctrl + F: toggle search"))
                    .child(components::reader_shortcut("Ctrl + C: copy current selection"))
                    .child(components::reader_shortcut("Ctrl + +/-: zoom")),
            )
            .into_any_element()
    }

    fn on_previous_page(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.previous_page(cx);
    }

    fn on_next_page(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.next_page(cx);
    }

    fn on_toggle_toc(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.toc_visible = !self.toc_visible;
        cx.notify();
    }

    fn on_focus_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search_active = true;
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_active {
            self.close_search(window, cx);
        } else {
            self.search_active = true;
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn on_close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.close_search(window, cx);
    }

    fn on_increase_scale(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(0.1, cx);
    }

    fn on_decrease_scale(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(-0.1, cx);
    }
}

impl Focusable for PdfReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PdfReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.title);
        let entity = cx.entity();
        let previous = entity.clone();
        let next = entity.clone();
        let toc = entity.clone();
        let search = entity.clone();
        let previous_match = entity.clone();
        let next_match = entity.clone();
        let close_search = entity.clone();
        let can_previous = self.current_page > 1;
        let can_next = self.current_page < self.page_count;
        let outline_available = self.pdf.read(cx).info().is_some_and(|info| !info.outline().is_empty());

        let mut body = components::reader_document_body().child(components::reader_content().child(self.pdf.clone()));
        if self.search_active {
            body = body.child(
                components::reader_search_bar()
                    .child(components::reader_search_box().key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{}/{}", self.search_match.0, self.search_match.1)))
                    .child(components::reader_square_button("pdf-search-previous", "‹").on_click(move |_, _, cx| {
                        previous_match.update(cx, |this, cx| {
                            this.navigate_search(-1, cx);
                        });
                    }))
                    .child(components::reader_square_button("pdf-search-next", "›").on_click(move |_, _, cx| {
                        next_match.update(cx, |this, cx| {
                            this.navigate_search(1, cx);
                        });
                    }))
                    .child(components::reader_square_button("pdf-search-close", "×").on_click(move |_, window, cx| {
                        close_search.update(cx, |this, cx| {
                            this.close_search(window, cx);
                        });
                    })),
            );
        }
        let content = if self.toc_visible && outline_available { components::reader_split(self.render_outline_with_context(entity.clone(), cx), body, 300.0).into_any_element() } else { body.into_any_element() };
        let settings = self.render_settings(entity.clone(), cx);
        let toolbar = components::reader_toolbar()
            .child(components::reader_control_button_state("pdf-previous-page", "←", can_previous).tooltip("Previous page").on_click(move |_, _, cx| {
                previous.update(cx, |this, cx| {
                    this.previous_page(cx);
                });
            }))
            .child(components::reader_match_count(format!("{}/{}", self.current_page, self.page_count)))
            .child(components::reader_control_button_state("pdf-next-page", "→", can_next).tooltip("Next page").on_click(move |_, _, cx| {
                next.update(cx, |this, cx| {
                    this.next_page(cx);
                });
            }))
            .when(outline_available, |toolbar| {
                toolbar.child(components::reader_control_button("pdf-contents", "Contents").on_click(move |_, _, cx| {
                    toc.update(cx, |this, cx| {
                        this.toc_visible = !this.toc_visible;
                        cx.notify();
                    });
                }))
            })
            .child(components::reader_title(self.title.clone()))
            .child(components::reader_control_button("pdf-search", "Search").on_click(move |_, window, cx| {
                search.update(cx, |this, cx| {
                    this.search_active = true;
                    this.search_input.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                });
            }))
            .child(components::reader_settings_popover().child(settings));

        components::page()
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_previous_page))
            .on_action(cx.listener(Self::on_next_page))
            .on_action(cx.listener(Self::on_toggle_toc))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_toggle_search))
            .on_action(cx.listener(Self::on_close_search))
            .on_action(cx.listener(Self::on_increase_scale))
            .on_action(cx.listener(Self::on_decrease_scale))
            .child(toolbar)
            .child(components::reader_content().child(content))
    }
}

pub(crate) fn bind_pdf_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("right", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("pageup", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("pagedown", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("PdfReaderDocument")),
    ]);
}
