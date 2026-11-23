//! Searchable font selection shared by interface and book settings.
use super::{FontCatalog, SystemFontFamilies};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Render, ScrollStrategy, SharedString, SystemFontsLoading, Task, TextSystem, UniformListScrollHandle, Window, div, px, uniform_list};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::{Scrollbar, ScrollbarAxis, ScrollbarShow};
use ui_text::text;

pub enum FontPickerEvent {
    Selected(String),
    Dismissed,
}

pub struct FontPicker {
    input: Entity<InputState>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    catalog: FontCatalog,
    scroll: UniformListScrollHandle,
    task: Option<Task<()>>,
    filter: fn(&TextSystem, &str) -> bool,
    selected: Option<String>,
    open: bool,
}

impl EventEmitter<FontPickerEvent> for FontPicker {}

impl FontPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(text!(common::search_fonts)));
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
            match event {
                InputEvent::Change => this.filter_font_catalog(cx),
                InputEvent::PressEnter { .. } => {
                    if let Some(family) = this.catalog.filtered.get(this.catalog.cursor).map(|index| this.catalog.families[*index].clone()) {
                        this.choose(family, cx);
                    }
                }
                _ => {}
            }
            cx.notify();
        })
        .detach();
        cx.observe_global::<SystemFontFamilies>(|this, cx| {
            if this.catalog.waiting_for_registration && cx.try_global::<SystemFontsLoading>().is_none_or(|loading| loading.0.is_closed()) {
                this.catalog.waiting_for_registration = false;
                this.catalog.loading = false;
                this.load(cx);
            }
        })
        .detach();
        Self { input, focus: cx.focus_handle(), previous_focus: None, catalog: FontCatalog::default(), scroll: UniformListScrollHandle::new(), task: None, filter: |_, _| true, selected: None, open: false }
    }

    pub fn catalog(&self) -> &FontCatalog {
        &self.catalog
    }
    pub fn scroll(&self) -> &UniformListScrollHandle {
        &self.scroll
    }
    pub fn query(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_selected(&mut self, selected: Option<String>, cx: &mut Context<Self>) {
        if self.selected != selected {
            self.selected = selected;
            cx.notify();
        }
    }

    pub fn show(&mut self, selected: Option<String>, filter: fn(&TextSystem, &str) -> bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.previous_focus = window.focused(cx);
        }
        self.open = true;
        self.selected = selected;
        self.filter = filter;
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.filter_font_catalog(cx);
        self.load(cx);
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.task = None;
        self.catalog = FontCatalog::default();
        self.previous_focus = None;
        cx.notify();
    }

    fn filter_font_catalog(&mut self, cx: &App) {
        self.catalog.filter(&self.query(cx));
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn navigate_font_catalog(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }
        let delta = match event.keystroke.key.as_str() {
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        self.catalog.cursor = self.catalog.cursor.saturating_add_signed(delta).min(self.catalog.filtered.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.catalog.cursor, ScrollStrategy::Nearest);
        cx.stop_propagation();
        cx.notify();
    }

    pub fn install_font_catalog(&mut self, families: Vec<String>, cx: &mut Context<Self>) {
        self.catalog.replace(families);
        self.filter_font_catalog(cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.catalog.loaded || self.catalog.loading {
            return;
        }
        // Register previews lazily; constructing the picker does not enumerate fonts.
        if let Err(error) = super::register_picker_fonts(cx) {
            log::error!("failed to install font picker previews: {error}");
        }
        self.catalog.loading = true;
        self.catalog.text_system = Some(cx.text_system().clone());
        let cached = cx.try_global::<SystemFontFamilies>().map(|fonts| fonts.0.clone());
        if cx.try_global::<SystemFontsLoading>().is_some_and(|loading| !loading.0.is_closed()) {
            self.catalog.waiting_for_registration = true;
            return;
        }
        let select = self.filter;
        let text_system = cx.text_system().clone();
        let fonts = cx.background_executor().spawn(async move { cached.unwrap_or_else(|| text_system.font_family_names()).into_iter().filter(|family| select(&text_system, family)).collect::<Vec<_>>() });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let selected = fonts.await;
            let _ = owner.update(cx, |owner, cx| owner.install_font_catalog(selected, cx));
        }));
    }

    fn choose(&mut self, family: String, cx: &mut Context<Self>) {
        self.selected = Some(family.clone());
        cx.emit(FontPickerEvent::Selected(family));
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous_focus = self.previous_focus.take();
        self.hide(cx);
        if let Some(focus) = previous_focus {
            focus.focus(window, cx);
        }
        cx.emit(FontPickerEvent::Dismissed);
    }

    fn panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = crate::browser_theme(cx);
        let selected = self.selected.clone();
        let catalog = self.catalog();
        let unavailable = catalog.loaded && selected.as_ref().is_some_and(|family| !catalog.families.contains(family));
        let rows = uniform_list(
            "font-picker-rows",
            catalog.filtered.len(),
            cx.processor(move |owner, range: std::ops::Range<usize>, _, cx| {
                let theme = crate::browser_theme(cx);
                let catalog = &owner.catalog;
                range
                    .map(|row| {
                        let index = catalog.filtered[row];
                        let family = catalog.families[index].clone();
                        let checked = selected.as_deref() == Some(family.as_str());
                        crate::font_picker_row(("font-picker-row", index), family.clone(), checked, theme).when(catalog.cursor == row && !checked, |row| row.bg(theme.hover)).debug_selector(move || format!("font-picker-row-{row}")).on_click(
                            cx.listener(move |owner, _, _, cx| {
                                owner.catalog.cursor = row;
                                owner.choose(family.clone(), cx);
                            }),
                        )
                    })
                    .collect()
            }),
        )
        .size_full()
        .track_scroll(self.scroll());
        let body = if catalog.loading || catalog.filtered.is_empty() {
            div().flex_1().min_h_0().text_color(theme.text_muted).child(if catalog.loading { text!(common::loading_fonts) } else { text!(common::no_matching_fonts) }).into_any_element()
        } else {
            div()
                .id("font-picker-list")
                .debug_selector(|| "font-picker-list".into())
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(rows)
                .child(Scrollbar::new(self.scroll()).axis(ScrollbarAxis::Vertical).scrollbar_show(ScrollbarShow::Always))
                .into_any_element()
        };
        let panel = crate::font_picker_surface(window, theme)
            .id("font-picker-panel")
            .debug_selector(|| "font-picker-panel".into())
            .tab_group()
            .capture_key_down(cx.listener(|owner, event, _, cx| owner.navigate_font_catalog(event, cx)))
            .when(unavailable, |panel| panel.child(div().debug_selector(|| "font-picker-unavailable".into()).text_color(theme.text_muted).child(text!(common::saved_font_unavailable_on_this_device))))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(crate::SPACE_SM))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.input)))
                    .child(crate::bottom_sheet_close_button("font-picker-close", theme).on_click(cx.listener(move |owner, _, window, cx| owner.dismiss(window, cx)))),
            )
            .child(body);
        crate::contain_modal_focus(panel, &self.focus, "FontPicker ModalFocus", cx).into_any_element()
    }
}

impl Render for FontPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div();
        if !self.open {
            return root;
        }
        let panel = self.panel(window, cx);
        let picker = cx.entity();
        root.child(crate::font_picker_overlay(panel, window, crate::browser_theme(cx), move |window, cx| picker.update(cx, |picker, cx| picker.dismiss(window, cx))))
    }
}
